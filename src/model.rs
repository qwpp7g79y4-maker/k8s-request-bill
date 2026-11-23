//! Reading saved `kubectl` JSON: pods (requests/limits) and metrics-server
//! `PodMetricsList` (usage).
//!
//! Pods come from `kubectl get pods -A -o json` (a `List`), the raw API
//! `PodList`, or a single `Pod`. Usage comes from
//! `kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods`.

use std::collections::HashMap;

use serde_json::Value;

use crate::quantity::{parse_cpu, parse_memory};
use crate::Error;

/// CPU (nanocores) and memory (bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Resources {
    /// Nanocores.
    pub cpu: u128,
    /// Bytes.
    pub mem: u128,
}

impl Resources {
    fn add(self, other: Resources) -> Resources {
        Resources {
            cpu: self.cpu.saturating_add(other.cpu),
            mem: self.mem.saturating_add(other.mem),
        }
    }

    fn max(self, other: Resources) -> Resources {
        Resources {
            cpu: self.cpu.max(other.cpu),
            mem: self.mem.max(other.mem),
        }
    }
}

/// One pod with its effective requests and limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pod {
    /// Namespace (`default` when the JSON has none).
    pub namespace: String,
    /// Pod name.
    pub name: String,
    /// `status.phase`, when present.
    pub phase: Option<String>,
    /// Effective requests (init containers, sidecars and overhead included).
    pub requests: Resources,
    /// Effective limits; containers without a limit contribute nothing.
    pub limits: Resources,
}

impl Pod {
    /// Pods in phase `Succeeded` or `Failed` no longer hold their requests.
    pub fn is_finished(&self) -> bool {
        matches!(self.phase.as_deref(), Some("Succeeded") | Some("Failed"))
    }
}

/// Parsed pod input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodInput {
    /// All pods found, finished ones included.
    pub pods: Vec<Pod>,
    /// List items whose `kind` was not `Pod`.
    pub skipped_non_pod: usize,
}

/// Usage per `(namespace, name)`.
pub type UsageMap = HashMap<(String, String), Resources>;

fn parse_json(text: &str) -> Result<Value, Error> {
    serde_json::from_str(text).map_err(|e| Error::msg(format!("invalid JSON: {e}")))
}

/// A quantity in JSON is a string (`"100m"`) or, in hand-written files, a number.
fn quantity_text(value: &Value) -> Result<String, Error> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(Error::msg(format!(
            "quantity must be a string or number, found {other}"
        ))),
    }
}

/// Read `{ "cpu": .., "memory": .. }`; missing keys count as zero.
fn read_resources(map: Option<&Value>) -> Result<Resources, Error> {
    let Some(map) = map else {
        return Ok(Resources::default());
    };
    let obj = map
        .as_object()
        .ok_or_else(|| Error::msg("resource list must be an object"))?;
    let cpu = match obj.get("cpu") {
        Some(v) => u128::from(parse_cpu(&quantity_text(v)?)?),
        None => 0,
    };
    let mem = match obj.get("memory") {
        Some(v) => u128::from(parse_memory(&quantity_text(v)?)?),
        None => 0,
    };
    Ok(Resources { cpu, mem })
}

struct Container {
    requests: Resources,
    limits: Resources,
    sidecar: bool,
}

fn read_container(value: &Value) -> Result<Container, Error> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>");
    let wrap = |e: Error| Error::msg(format!("container {name}: {e}"));
    let resources = value.get("resources");
    let requests = read_resources(resources.and_then(|r| r.get("requests"))).map_err(wrap)?;
    let limits = read_resources(resources.and_then(|r| r.get("limits"))).map_err(wrap)?;
    let sidecar = value.get("restartPolicy").and_then(Value::as_str) == Some("Always");
    Ok(Container {
        requests,
        limits,
        sidecar,
    })
}

fn read_containers(spec: &Value, key: &str) -> Result<Vec<Container>, Error> {
    match spec.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(read_container).collect(),
        Some(_) => Err(Error::msg(format!("spec.{key} must be an array"))),
    }
}

/// Effective resource of a pod for one kind (requests or limits), following the
/// scheduler rule: the larger of the init-phase peak and the running total,
/// where restartable init containers (sidecars) stay running, plus overhead.
fn effective(
    init: &[Container],
    main: &[Container],
    overhead: Resources,
    pick: fn(&Container) -> Resources,
) -> Resources {
    let mut sidecars = Resources::default();
    let mut peak = Resources::default();
    for c in init {
        let r = pick(c);
        if c.sidecar {
            sidecars = sidecars.add(r);
            peak = peak.max(sidecars);
        } else {
            peak = peak.max(sidecars.add(r));
        }
    }
    let running = main.iter().fold(sidecars, |acc, c| acc.add(pick(c)));
    peak.max(running).add(overhead)
}

fn read_pod(value: &Value) -> Result<Pod, Error> {
    let meta = value.get("metadata");
    let name = meta
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::msg("pod without metadata.name"))?
        .to_string();
    let namespace = meta
        .and_then(|m| m.get("namespace"))
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    let wrap = |e: Error| Error::msg(format!("pod {namespace}/{name}: {e}"));
    let spec = value.get("spec").filter(|s| s.is_object()).ok_or_else(|| {
        Error::msg(format!(
            "pod {namespace}/{name}: no `spec` (is this a metrics file? pass it to --usage)"
        ))
    })?;
    let init = read_containers(spec, "initContainers").map_err(wrap)?;
    let main = read_containers(spec, "containers").map_err(wrap)?;
    let overhead = read_resources(spec.get("overhead")).map_err(wrap)?;
    let phase = value
        .get("status")
        .and_then(|s| s.get("phase"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Pod {
        requests: effective(&init, &main, overhead, |c| c.requests),
        limits: effective(&init, &main, Resources::default(), |c| c.limits),
        namespace,
        name,
        phase,
    })
}

fn is_kind(value: &Value, kind: &str) -> bool {
    value.get("kind").and_then(Value::as_str) == Some(kind)
}

/// Parse pod JSON: a `Pod`, or a list (`List`, `PodList`) with `items`.
/// Items without a `kind` are taken as pods; items of another kind are skipped.
pub fn parse_pods(text: &str) -> Result<PodInput, Error> {
    let root = parse_json(text)?;
    if is_kind(&root, "Pod") {
        return Ok(PodInput {
            pods: vec![read_pod(&root)?],
            skipped_non_pod: 0,
        });
    }
    if is_kind(&root, "PodMetricsList") || is_kind(&root, "PodMetrics") {
        return Err(Error::msg(
            "this is a metrics file, not a pods file; pass it to --usage",
        ));
    }
    let items = root
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::msg("expected a Pod, or a List/PodList with an `items` array"))?;
    let mut pods = Vec::new();
    let mut skipped = 0;
    for item in items {
        let kind = item.get("kind").and_then(Value::as_str);
        if kind.is_some_and(|k| k != "Pod") {
            skipped += 1;
            continue;
        }
        pods.push(read_pod(item)?);
    }
    if pods.is_empty() && skipped > 0 {
        return Err(Error::msg(format!(
            "no Pod items found ({skipped} items of other kinds); \
             pass `kubectl get pods -A -o json` as the pods file"
        )));
    }
    Ok(PodInput {
        pods,
        skipped_non_pod: skipped,
    })
}

fn read_usage_item(item: &Value) -> Result<((String, String), Resources), Error> {
    let meta = item.get("metadata");
    let name = meta
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::msg("metrics item without metadata.name"))?
        .to_string();
    let namespace = meta
        .and_then(|m| m.get("namespace"))
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    let containers = item
        .get("containers")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::msg(format!(
                "metrics for {namespace}/{name}: `containers` array is missing \
                 (expected a PodMetricsList from \
                 `kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods`)"
            ))
        })?;
    let mut total = Resources::default();
    for c in containers {
        let usage = read_resources(c.get("usage"))
            .map_err(|e| Error::msg(format!("metrics for {namespace}/{name}: {e}")))?;
        total = total.add(usage);
    }
    Ok(((namespace, name), total))
}

/// Parse a metrics-server `PodMetricsList` (or a single `PodMetrics`) into usage per pod.
pub fn parse_usage(text: &str) -> Result<UsageMap, Error> {
    let root = parse_json(text)?;
    let mut map = UsageMap::new();
    if is_kind(&root, "PodMetrics") {
        let (key, usage) = read_usage_item(&root)?;
        map.insert(key, usage);
        return Ok(map);
    }
    let items = root.get("items").and_then(Value::as_array).ok_or_else(|| {
        Error::msg("expected a PodMetricsList (or PodMetrics) with an `items` array")
    })?;
    for item in items {
        let (key, usage) = read_usage_item(item)?;
        map.insert(key, usage);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pod_json(spec: &str) -> String {
        format!(
            r#"{{"kind":"Pod","metadata":{{"name":"p","namespace":"ns"}},"spec":{spec},"status":{{"phase":"Running"}}}}"#
        )
    }

    fn one(spec: &str) -> Pod {
        parse_pods(&pod_json(spec)).unwrap().pods.remove(0)
    }

    #[test]
    fn single_container_requests_and_limits() {
        let p = one(
            r#"{"containers":[{"name":"a","resources":{"requests":{"cpu":"250m","memory":"128Mi"},"limits":{"cpu":"1","memory":"256Mi"}}}]}"#,
        );
        assert_eq!(p.namespace, "ns");
        assert_eq!(p.name, "p");
        assert_eq!(p.requests.cpu, 250_000_000);
        assert_eq!(p.requests.mem, 128 * 1024 * 1024);
        assert_eq!(p.limits.cpu, 1_000_000_000);
        assert_eq!(p.limits.mem, 256 * 1024 * 1024);
    }

    #[test]
    fn containers_are_summed() {
        let p = one(r#"{"containers":[
                {"name":"a","resources":{"requests":{"cpu":"100m","memory":"1Gi"}}},
                {"name":"b","resources":{"requests":{"cpu":"200m","memory":"2Gi"}}}]}"#);
        assert_eq!(p.requests.cpu, 300_000_000);
        assert_eq!(p.requests.mem, 3 * (1 << 30));
    }

    #[test]
    fn container_without_resources_counts_zero() {
        let p = one(r#"{"containers":[{"name":"a"}]}"#);
        assert_eq!(p.requests, Resources::default());
        assert_eq!(p.limits, Resources::default());
    }

    #[test]
    fn missing_limit_contributes_nothing() {
        let p = one(r#"{"containers":[
                {"name":"a","resources":{"requests":{"cpu":"1"},"limits":{"cpu":"2"}}},
                {"name":"b","resources":{"requests":{"cpu":"1"}}}]}"#);
        assert_eq!(p.requests.cpu, 2_000_000_000);
        assert_eq!(p.limits.cpu, 2_000_000_000);
    }

    #[test]
    fn numeric_quantities_are_accepted() {
        let p = one(
            r#"{"containers":[{"name":"a","resources":{"requests":{"cpu":2,"memory":1024}}}]}"#,
        );
        assert_eq!(p.requests.cpu, 2_000_000_000);
        assert_eq!(p.requests.mem, 1024);
    }

    #[test]
    fn init_container_peak_wins_when_larger() {
        let p = one(
            r#"{"initContainers":[{"name":"i","resources":{"requests":{"cpu":"2","memory":"1Gi"}}}],
                "containers":[{"name":"a","resources":{"requests":{"cpu":"500m","memory":"2Gi"}}}]}"#,
        );
        assert_eq!(p.requests.cpu, 2_000_000_000); // init cpu is larger
        assert_eq!(p.requests.mem, 2 * (1 << 30)); // main memory is larger
    }

    #[test]
    fn sidecar_init_container_adds_to_running_total() {
        let p = one(
            r#"{"initContainers":[{"name":"s","restartPolicy":"Always","resources":{"requests":{"cpu":"100m"}}}],
                "containers":[{"name":"a","resources":{"requests":{"cpu":"400m"}}}]}"#,
        );
        assert_eq!(p.requests.cpu, 500_000_000);
    }

    #[test]
    fn sidecar_counts_towards_later_init_peak() {
        let p = one(r#"{"initContainers":[
                {"name":"s","restartPolicy":"Always","resources":{"requests":{"cpu":"1"}}},
                {"name":"i","resources":{"requests":{"cpu":"3"}}}],
                "containers":[{"name":"a","resources":{"requests":{"cpu":"500m"}}}]}"#);
        assert_eq!(p.requests.cpu, 4_000_000_000); // 1 sidecar + 3 init > 1.5 running
    }

    #[test]
    fn overhead_is_added_to_requests_only() {
        let p = one(r#"{"overhead":{"cpu":"250m","memory":"120Mi"},
                "containers":[{"name":"a","resources":{"requests":{"cpu":"1"},"limits":{"cpu":"2"}}}]}"#);
        assert_eq!(p.requests.cpu, 1_250_000_000);
        assert_eq!(p.requests.mem, 120 * 1024 * 1024);
        assert_eq!(p.limits.cpu, 2_000_000_000);
    }

    #[test]
    fn missing_namespace_is_default() {
        let input =
            parse_pods(r#"{"kind":"Pod","metadata":{"name":"x"},"spec":{"containers":[]}}"#)
                .unwrap();
        assert_eq!(input.pods[0].namespace, "default");
    }

    #[test]
    fn list_of_pods() {
        let text = r#"{"kind":"List","items":[
            {"kind":"Pod","metadata":{"name":"a","namespace":"x"},"spec":{"containers":[]}},
            {"kind":"Pod","metadata":{"name":"b","namespace":"y"},"spec":{"containers":[]}}]}"#;
        let input = parse_pods(text).unwrap();
        assert_eq!(input.pods.len(), 2);
        assert_eq!(input.skipped_non_pod, 0);
    }

    #[test]
    fn podlist_items_without_kind_are_pods() {
        let text = r#"{"kind":"PodList","items":[
            {"metadata":{"name":"a","namespace":"x"},"spec":{"containers":[]}}]}"#;
        assert_eq!(parse_pods(text).unwrap().pods.len(), 1);
    }

    #[test]
    fn non_pod_items_are_skipped_and_counted() {
        let text = r#"{"kind":"List","items":[
            {"kind":"Pod","metadata":{"name":"a"},"spec":{"containers":[]}},
            {"kind":"Service","metadata":{"name":"s"}}]}"#;
        let input = parse_pods(text).unwrap();
        assert_eq!(input.pods.len(), 1);
        assert_eq!(input.skipped_non_pod, 1);
    }

    #[test]
    fn only_non_pod_items_is_an_error() {
        let text = r#"{"kind":"List","items":[{"kind":"Service","metadata":{"name":"a"}}]}"#;
        let err = parse_pods(text).unwrap_err().to_string();
        assert!(err.contains("no Pod items"), "{err}");
    }

    #[test]
    fn metrics_list_as_pods_is_refused() {
        let text =
            r#"{"kind":"PodMetricsList","items":[{"metadata":{"name":"a"},"containers":[]}]}"#;
        let err = parse_pods(text).unwrap_err().to_string();
        assert!(err.contains("--usage"), "{err}");
        let single = r#"{"kind":"PodMetrics","metadata":{"name":"a"},"containers":[]}"#;
        assert!(parse_pods(single).is_err());
    }

    #[test]
    fn items_without_kind_and_spec_are_refused() {
        let text = r#"{"items":[{"metadata":{"name":"a","namespace":"x"},"containers":[]}]}"#;
        let err = parse_pods(text).unwrap_err().to_string();
        assert!(err.contains("x/a") && err.contains("spec"), "{err}");
    }

    #[test]
    fn empty_list_is_fine() {
        let input = parse_pods(r#"{"kind":"List","items":[]}"#).unwrap();
        assert!(input.pods.is_empty());
    }

    #[test]
    fn finished_phase_detection() {
        let mk = |phase: Option<&str>| Pod {
            namespace: "n".into(),
            name: "p".into(),
            phase: phase.map(str::to_string),
            requests: Resources::default(),
            limits: Resources::default(),
        };
        assert!(mk(Some("Succeeded")).is_finished());
        assert!(mk(Some("Failed")).is_finished());
        assert!(!mk(Some("Running")).is_finished());
        assert!(!mk(Some("Pending")).is_finished());
        assert!(!mk(None).is_finished());
    }

    #[test]
    fn phase_is_read() {
        assert_eq!(
            one(r#"{"containers":[]}"#).phase.as_deref(),
            Some("Running")
        );
    }

    #[test]
    fn bad_json_is_reported() {
        let err = parse_pods("{not json").unwrap_err().to_string();
        assert!(err.starts_with("invalid JSON"), "{err}");
    }

    #[test]
    fn wrong_shape_is_reported() {
        assert!(parse_pods(r#"{"kind":"Node"}"#).is_err());
        assert!(parse_pods("[]").is_err());
        assert!(parse_pods("42").is_err());
    }

    #[test]
    fn pod_without_name_is_an_error() {
        assert!(parse_pods(r#"{"kind":"Pod","spec":{"containers":[]}}"#).is_err());
    }

    #[test]
    fn bad_quantity_error_names_pod_and_container() {
        let err = parse_pods(&pod_json(
            r#"{"containers":[{"name":"web","resources":{"requests":{"cpu":"lots"}}}]}"#,
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("ns/p"), "{err}");
        assert!(err.contains("web"), "{err}");
        assert!(err.contains("lots"), "{err}");
    }

    #[test]
    fn bad_quantity_type_is_an_error() {
        assert!(parse_pods(&pod_json(
            r#"{"containers":[{"name":"a","resources":{"requests":{"cpu":true}}}]}"#
        ))
        .is_err());
    }

    #[test]
    fn containers_must_be_an_array() {
        assert!(parse_pods(&pod_json(r#"{"containers":{}}"#)).is_err());
    }

    #[test]
    fn usage_list_sums_containers() {
        let text = r#"{"kind":"PodMetricsList","items":[
            {"metadata":{"name":"a","namespace":"x"},"containers":[
                {"name":"c1","usage":{"cpu":"12345678n","memory":"100Ki"}},
                {"name":"c2","usage":{"cpu":"1m","memory":"28Ki"}}]}]}"#;
        let map = parse_usage(text).unwrap();
        let u = map[&("x".to_string(), "a".to_string())];
        assert_eq!(u.cpu, 13_345_678);
        assert_eq!(u.mem, 128 * 1024);
    }

    #[test]
    fn single_pod_metrics() {
        let text = r#"{"kind":"PodMetrics","metadata":{"name":"a","namespace":"x"},"containers":[{"name":"c","usage":{"cpu":"5m","memory":"1Mi"}}]}"#;
        assert_eq!(parse_usage(text).unwrap().len(), 1);
    }

    #[test]
    fn usage_with_no_containers_is_zero() {
        let text = r#"{"items":[{"metadata":{"name":"a","namespace":"x"},"containers":[]}]}"#;
        let map = parse_usage(text).unwrap();
        assert_eq!(
            map[&("x".to_string(), "a".to_string())],
            Resources::default()
        );
    }

    #[test]
    fn usage_errors() {
        assert!(parse_usage("{").is_err());
        assert!(parse_usage(r#"{"kind":"Node"}"#).is_err());
        assert!(
            parse_usage(r#"{"items":[{"metadata":{"namespace":"x"},"containers":[]}]}"#).is_err()
        );
        assert!(parse_usage(r#"{"items":[{"metadata":{"name":"a"}}]}"#).is_err());
        let bad = r#"{"items":[{"metadata":{"name":"a","namespace":"x"},"containers":[{"name":"c","usage":{"cpu":"zz"}}]}]}"#;
        let err = parse_usage(bad).unwrap_err().to_string();
        assert!(err.contains("x/a"), "{err}");
    }
}
