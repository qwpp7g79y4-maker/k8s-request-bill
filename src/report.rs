//! Aggregation per namespace and rendering (text table and JSON).

use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

use clap::ValueEnum;
use rust_decimal::Decimal;
use serde_json::{Map, Number, Value};

use crate::model::{PodInput, Resources, UsageMap};
use crate::quantity::{bytes_to_gib, fixed, format_gib, format_vcpu, nano_to_vcpu};
use crate::rates::Rates;
use crate::Error;

/// Totals for one namespace (or for everything).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    /// Namespace name (`TOTAL` for the grand total).
    pub namespace: String,
    /// Pods counted.
    pub pods: usize,
    /// Pods counted that have no entry in the metrics file.
    pub pods_without_usage: usize,
    /// Sum of effective requests.
    pub requests: Resources,
    /// Sum of effective limits (containers without a limit add nothing).
    pub limits: Resources,
    /// Sum of usage over pods that have metrics.
    pub usage: Resources,
    /// Sum over pods with metrics of `max(request - usage, 0)`.
    pub over: Resources,
}

/// The aggregated result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// One row per namespace.
    pub rows: Vec<Stats>,
    /// Sum of all rows.
    pub total: Stats,
    /// Whether a metrics file was given.
    pub has_usage: bool,
    /// List items that were not pods.
    pub skipped_non_pod: usize,
    /// Pods in phase `Succeeded` or `Failed`, left out.
    pub skipped_finished: usize,
    /// Metrics entries without a matching pod in the pods file.
    pub usage_without_pod: usize,
}

/// Row order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortKey {
    /// Namespace name, A to Z (default).
    Name,
    /// Largest CPU request first.
    CpuRequest,
    /// Largest memory request first.
    MemoryRequest,
    /// Largest CPU overprovisioning first (needs `--usage`).
    CpuOver,
    /// Largest memory overprovisioning first (needs `--usage`).
    MemoryOver,
    /// Largest overprovisioning cost first (needs `--usage` and a rate).
    OverCost,
}

/// How to present the numbers.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Node rates, when given.
    pub rates: Option<Rates>,
    /// Hours the cost covers.
    pub hours: Decimal,
    /// Currency label, shown as-is.
    pub currency: String,
    /// Add limit columns.
    pub show_limits: bool,
}

fn add_into(total: &mut Stats, row: &Stats) {
    total.pods += row.pods;
    total.pods_without_usage += row.pods_without_usage;
    for (t, r) in [
        (&mut total.requests, row.requests),
        (&mut total.limits, row.limits),
        (&mut total.usage, row.usage),
        (&mut total.over, row.over),
    ] {
        t.cpu = t.cpu.saturating_add(r.cpu);
        t.mem = t.mem.saturating_add(r.mem);
    }
}

/// Aggregate pods per namespace. `namespaces` restricts the result (empty = all).
pub fn aggregate(input: &PodInput, usage: Option<&UsageMap>, namespaces: &[String]) -> Report {
    let wanted = |ns: &str| namespaces.is_empty() || namespaces.iter().any(|n| n == ns);
    let mut by_ns: BTreeMap<&str, Stats> = BTreeMap::new();
    let mut skipped_finished = 0;
    for pod in &input.pods {
        if !wanted(&pod.namespace) {
            continue;
        }
        if pod.is_finished() {
            skipped_finished += 1;
            continue;
        }
        let row = by_ns.entry(&pod.namespace).or_insert_with(|| Stats {
            namespace: pod.namespace.clone(),
            ..Stats::default()
        });
        row.pods += 1;
        row.requests.cpu = row.requests.cpu.saturating_add(pod.requests.cpu);
        row.requests.mem = row.requests.mem.saturating_add(pod.requests.mem);
        row.limits.cpu = row.limits.cpu.saturating_add(pod.limits.cpu);
        row.limits.mem = row.limits.mem.saturating_add(pod.limits.mem);
        if let Some(map) = usage {
            match map.get(&(pod.namespace.clone(), pod.name.clone())) {
                Some(u) => {
                    row.usage.cpu = row.usage.cpu.saturating_add(u.cpu);
                    row.usage.mem = row.usage.mem.saturating_add(u.mem);
                    row.over.cpu = row
                        .over
                        .cpu
                        .saturating_add(pod.requests.cpu.saturating_sub(u.cpu));
                    row.over.mem = row
                        .over
                        .mem
                        .saturating_add(pod.requests.mem.saturating_sub(u.mem));
                }
                None => row.pods_without_usage += 1,
            }
        }
    }
    let rows: Vec<Stats> = by_ns.into_values().collect();
    let mut total = Stats {
        namespace: "TOTAL".to_string(),
        ..Stats::default()
    };
    for row in &rows {
        add_into(&mut total, row);
    }
    let known: HashSet<(&str, &str)> = input
        .pods
        .iter()
        .map(|p| (p.namespace.as_str(), p.name.as_str()))
        .collect();
    let usage_without_pod = usage.map_or(0, |map| {
        map.keys()
            .filter(|(ns, name)| wanted(ns) && !known.contains(&(ns.as_str(), name.as_str())))
            .count()
    });
    Report {
        rows,
        total,
        has_usage: usage.is_some(),
        skipped_non_pod: input.skipped_non_pod,
        skipped_finished,
        usage_without_pod,
    }
}

/// Request cost of a row (needs rates).
fn request_cost(s: &Stats, rates: &Rates, hours: Decimal) -> Result<Decimal, Error> {
    rates.cost(s.requests.cpu, s.requests.mem, hours)
}

/// Overprovisioning cost of a row (needs rates).
fn over_cost(s: &Stats, rates: &Rates, hours: Decimal) -> Result<Decimal, Error> {
    rates.cost(s.over.cpu, s.over.mem, hours)
}

/// Sort the rows. Keys that need usage or rates fail with a clear message.
pub fn sort_rows(report: &mut Report, key: SortKey, opts: &RenderOptions) -> Result<(), Error> {
    let needs_usage = matches!(
        key,
        SortKey::CpuOver | SortKey::MemoryOver | SortKey::OverCost
    );
    if needs_usage && !report.has_usage {
        return Err(Error::msg("this --sort key needs --usage"));
    }
    match key {
        SortKey::Name => report.rows.sort_by(|a, b| a.namespace.cmp(&b.namespace)),
        SortKey::CpuRequest => report.rows.sort_by(|a, b| {
            b.requests
                .cpu
                .cmp(&a.requests.cpu)
                .then_with(|| a.namespace.cmp(&b.namespace))
        }),
        SortKey::MemoryRequest => report.rows.sort_by(|a, b| {
            b.requests
                .mem
                .cmp(&a.requests.mem)
                .then_with(|| a.namespace.cmp(&b.namespace))
        }),
        SortKey::CpuOver => report.rows.sort_by(|a, b| {
            b.over
                .cpu
                .cmp(&a.over.cpu)
                .then_with(|| a.namespace.cmp(&b.namespace))
        }),
        SortKey::MemoryOver => report.rows.sort_by(|a, b| {
            b.over
                .mem
                .cmp(&a.over.mem)
                .then_with(|| a.namespace.cmp(&b.namespace))
        }),
        SortKey::OverCost => {
            let rates = opts.rates.ok_or_else(|| {
                Error::msg(
                    "--sort over-cost needs a rate (--cpu-hour, --mem-gib-hour or --node-hour)",
                )
            })?;
            let mut keyed = Vec::with_capacity(report.rows.len());
            for row in report.rows.drain(..) {
                let cost = over_cost(&row, &rates, opts.hours)?;
                keyed.push((cost, row));
            }
            keyed.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| a.1.namespace.cmp(&b.1.namespace))
            });
            report.rows = keyed.into_iter().map(|(_, row)| row).collect();
        }
    }
    Ok(())
}

fn money(d: Decimal) -> String {
    fixed(d, 4)
}

fn rate_text(d: Decimal) -> String {
    d.normalize().to_string()
}

/// Render the text table.
pub fn render_table(report: &Report, opts: &RenderOptions) -> Result<String, Error> {
    let mut header: Vec<String> = ["NAMESPACE", "PODS", "CPU_REQ", "MEM_REQ"]
        .map(String::from)
        .into();
    if opts.show_limits {
        header.extend(["CPU_LIM", "MEM_LIM"].map(String::from));
    }
    if report.has_usage {
        header.extend(["CPU_USE", "MEM_USE", "CPU_OVER", "MEM_OVER"].map(String::from));
    }
    if opts.rates.is_some() {
        header.push("REQ_COST".to_string());
        if report.has_usage {
            header.push("OVER_COST".to_string());
        }
    }
    let mut lines: Vec<Vec<String>> = vec![header];
    for s in report.rows.iter().chain(std::iter::once(&report.total)) {
        let mut cells = vec![
            s.namespace.clone(),
            s.pods.to_string(),
            format_vcpu(s.requests.cpu),
            format_gib(s.requests.mem),
        ];
        if opts.show_limits {
            cells.push(format_vcpu(s.limits.cpu));
            cells.push(format_gib(s.limits.mem));
        }
        if report.has_usage {
            cells.push(format_vcpu(s.usage.cpu));
            cells.push(format_gib(s.usage.mem));
            cells.push(format_vcpu(s.over.cpu));
            cells.push(format_gib(s.over.mem));
        }
        if let Some(rates) = &opts.rates {
            cells.push(money(request_cost(s, rates, opts.hours)?));
            if report.has_usage {
                cells.push(money(over_cost(s, rates, opts.hours)?));
            }
        }
        lines.push(cells);
    }
    let columns = lines[0].len();
    let widths: Vec<usize> = (0..columns)
        .map(|i| {
            lines
                .iter()
                .map(|l| l[i].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for line in &lines {
        let mut parts = Vec::with_capacity(columns);
        for (i, cell) in line.iter().enumerate() {
            if i == 0 {
                parts.push(format!("{cell:<w$}", w = widths[i]));
            } else {
                parts.push(format!("{cell:>w$}", w = widths[i]));
            }
        }
        out.push_str(parts.join("  ").trim_end());
        out.push('\n');
    }
    out.push('\n');
    out.push_str("CPU in vCPU, memory in GiB.\n");
    if let Some(rates) = &opts.rates {
        out.push_str(&format!(
            "Cost for {} h at {} {cur}/vCPU-h and {} {cur}/GiB-h.\n",
            rate_text(opts.hours),
            rate_text(rates.cpu_hour),
            rate_text(rates.mem_gib_hour),
            cur = opts.currency,
        ));
    }
    if report.has_usage {
        out.push_str("OVER = sum over pods of max(request - usage, 0).\n");
        if report.total.pods_without_usage > 0 {
            out.push_str(&format!(
                "note: {} pod(s) have no metrics; they count in REQ but not in USE or OVER.\n",
                report.total.pods_without_usage
            ));
        }
        if report.usage_without_pod > 0 {
            out.push_str(&format!(
                "note: {} metrics entr(y/ies) match no pod and were ignored.\n",
                report.usage_without_pod
            ));
        }
    }
    if report.skipped_finished > 0 {
        out.push_str(&format!(
            "note: {} Succeeded/Failed pod(s) left out.\n",
            report.skipped_finished
        ));
    }
    if report.skipped_non_pod > 0 {
        out.push_str(&format!(
            "note: {} non-Pod item(s) skipped.\n",
            report.skipped_non_pod
        ));
    }
    Ok(out)
}

fn number(d: Decimal) -> Value {
    let text = d.normalize().to_string();
    match Number::from_str(&text) {
        Ok(n) => Value::Number(n),
        Err(_) => Value::String(text),
    }
}

fn vcpu_value(nanocores: u128) -> Result<Value, Error> {
    Ok(number(nano_to_vcpu(nanocores)?))
}

fn gib_value(bytes: u128) -> Result<Value, Error> {
    Ok(number(bytes_to_gib(bytes)?.round_dp(6)))
}

fn cost_value(d: Decimal) -> Value {
    number(d.round_dp(6))
}

fn stats_json(
    s: &Stats,
    has_usage: bool,
    opts: &RenderOptions,
    with_name: bool,
) -> Result<Value, Error> {
    let mut m = Map::new();
    if with_name {
        m.insert("namespace".into(), Value::String(s.namespace.clone()));
    }
    m.insert("pods".into(), Value::from(s.pods));
    m.insert(
        "pods_without_usage".into(),
        if has_usage {
            Value::from(s.pods_without_usage)
        } else {
            Value::Null
        },
    );
    m.insert("cpu_request_vcpu".into(), vcpu_value(s.requests.cpu)?);
    m.insert("memory_request_gib".into(), gib_value(s.requests.mem)?);
    m.insert("cpu_limit_vcpu".into(), vcpu_value(s.limits.cpu)?);
    m.insert("memory_limit_gib".into(), gib_value(s.limits.mem)?);
    let usage = |f: &dyn Fn() -> Result<Value, Error>| -> Result<Value, Error> {
        if has_usage {
            f()
        } else {
            Ok(Value::Null)
        }
    };
    m.insert("cpu_usage_vcpu".into(), usage(&|| vcpu_value(s.usage.cpu))?);
    m.insert(
        "memory_usage_gib".into(),
        usage(&|| gib_value(s.usage.mem))?,
    );
    m.insert("cpu_over_vcpu".into(), usage(&|| vcpu_value(s.over.cpu))?);
    m.insert("memory_over_gib".into(), usage(&|| gib_value(s.over.mem))?);
    let (req, over) = match &opts.rates {
        Some(rates) => (
            cost_value(request_cost(s, rates, opts.hours)?),
            if has_usage {
                cost_value(over_cost(s, rates, opts.hours)?)
            } else {
                Value::Null
            },
        ),
        None => (Value::Null, Value::Null),
    };
    m.insert("request_cost".into(), req);
    m.insert("over_cost".into(), over);
    Ok(Value::Object(m))
}

/// Render the JSON document (pretty-printed, trailing newline).
pub fn render_json(report: &Report, opts: &RenderOptions) -> Result<String, Error> {
    let mut root = Map::new();
    root.insert("tool".into(), Value::String("k8s-request-bill".into()));
    root.insert(
        "version".into(),
        Value::String(env!("CARGO_PKG_VERSION").into()),
    );
    root.insert("hours".into(), number(opts.hours));
    root.insert("currency".into(), Value::String(opts.currency.clone()));
    root.insert(
        "rates".into(),
        match &opts.rates {
            Some(r) => {
                let mut m = Map::new();
                m.insert("cpu_hour".into(), number(r.cpu_hour));
                m.insert("mem_gib_hour".into(), number(r.mem_gib_hour));
                Value::Object(m)
            }
            None => Value::Null,
        },
    );
    let mut rows = Vec::with_capacity(report.rows.len());
    for s in &report.rows {
        rows.push(stats_json(s, report.has_usage, opts, true)?);
    }
    root.insert("namespaces".into(), Value::Array(rows));
    root.insert(
        "total".into(),
        stats_json(&report.total, report.has_usage, opts, false)?,
    );
    let mut notes = Map::new();
    notes.insert(
        "skipped_non_pod_items".into(),
        Value::from(report.skipped_non_pod),
    );
    notes.insert(
        "skipped_finished_pods".into(),
        Value::from(report.skipped_finished),
    );
    notes.insert(
        "usage_without_pod".into(),
        Value::from(report.usage_without_pod),
    );
    root.insert("notes".into(), Value::Object(notes));
    let mut text = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| Error::msg(format!("cannot write JSON: {e}")))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Pod;

    const GI: u128 = 1 << 30;

    fn pod(ns: &str, name: &str, cpu_m: u128, mem_gi: u128) -> Pod {
        Pod {
            namespace: ns.into(),
            name: name.into(),
            phase: Some("Running".into()),
            requests: Resources {
                cpu: cpu_m * 1_000_000,
                mem: mem_gi * GI,
            },
            limits: Resources::default(),
        }
    }

    fn input(pods: Vec<Pod>) -> PodInput {
        PodInput {
            pods,
            skipped_non_pod: 0,
        }
    }

    fn usage(entries: &[(&str, &str, u128, u128)]) -> UsageMap {
        entries
            .iter()
            .map(|(ns, n, cpu_m, mem_gi)| {
                (
                    (ns.to_string(), n.to_string()),
                    Resources {
                        cpu: cpu_m * 1_000_000,
                        mem: mem_gi * GI,
                    },
                )
            })
            .collect()
    }

    fn opts(rates: Option<Rates>) -> RenderOptions {
        RenderOptions {
            rates,
            hours: Decimal::ONE,
            currency: "USD".into(),
            show_limits: false,
        }
    }

    fn rates() -> Rates {
        Rates {
            cpu_hour: Decimal::from_str("0.04").unwrap(),
            mem_gib_hour: Decimal::from_str("0.01").unwrap(),
        }
    }

    #[test]
    fn groups_by_namespace_sorted_by_name() {
        let r = aggregate(
            &input(vec![
                pod("b", "1", 100, 1),
                pod("a", "2", 200, 2),
                pod("b", "3", 300, 3),
            ]),
            None,
            &[],
        );
        let names: Vec<_> = r.rows.iter().map(|s| s.namespace.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(r.rows[1].pods, 2);
        assert_eq!(r.rows[1].requests.cpu, 400_000_000);
        assert_eq!(r.total.pods, 3);
        assert_eq!(r.total.requests.mem, 6 * GI);
    }

    #[test]
    fn namespace_filter_restricts_rows() {
        let r = aggregate(
            &input(vec![
                pod("a", "1", 100, 1),
                pod("b", "2", 100, 1),
                pod("c", "3", 100, 1),
            ]),
            None,
            &["a".to_string(), "c".to_string()],
        );
        assert_eq!(r.rows.len(), 2);
        assert_eq!(r.total.pods, 2);
    }

    #[test]
    fn finished_pods_are_left_out_and_counted() {
        let mut done = pod("a", "done", 500, 1);
        done.phase = Some("Succeeded".into());
        let mut failed = pod("a", "failed", 500, 1);
        failed.phase = Some("Failed".into());
        let r = aggregate(
            &input(vec![pod("a", "ok", 100, 1), done, failed]),
            None,
            &[],
        );
        assert_eq!(r.total.pods, 1);
        assert_eq!(r.skipped_finished, 2);
        assert_eq!(r.total.requests.cpu, 100_000_000);
    }

    #[test]
    fn no_usage_means_no_over() {
        let r = aggregate(&input(vec![pod("a", "1", 100, 1)]), None, &[]);
        assert!(!r.has_usage);
        assert_eq!(r.total.over, Resources::default());
        assert_eq!(r.total.pods_without_usage, 0);
    }

    #[test]
    fn over_is_request_minus_usage_per_pod() {
        let r = aggregate(
            &input(vec![pod("a", "1", 1000, 4)]),
            Some(&usage(&[("a", "1", 250, 1)])),
            &[],
        );
        assert_eq!(r.total.over.cpu, 750_000_000);
        assert_eq!(r.total.over.mem, 3 * GI);
        assert_eq!(r.total.usage.cpu, 250_000_000);
    }

    #[test]
    fn over_is_clamped_per_pod_so_bursting_pods_do_not_offset_others() {
        let r = aggregate(
            &input(vec![pod("a", "idle", 1000, 1), pod("a", "hot", 100, 1)]),
            Some(&usage(&[("a", "idle", 0, 0), ("a", "hot", 900, 3)])),
            &[],
        );
        assert_eq!(r.total.over.cpu, 1_000_000_000); // idle only
        assert_eq!(r.total.over.mem, GI);
        assert_eq!(r.total.usage.cpu, 900_000_000);
    }

    #[test]
    fn pods_without_metrics_are_counted_but_excluded_from_over() {
        let r = aggregate(
            &input(vec![pod("a", "has", 1000, 1), pod("a", "none", 1000, 1)]),
            Some(&usage(&[("a", "has", 500, 0)])),
            &[],
        );
        assert_eq!(r.total.pods_without_usage, 1);
        assert_eq!(r.total.over.cpu, 500_000_000);
        assert_eq!(r.total.requests.cpu, 2_000_000_000);
    }

    #[test]
    fn metrics_without_pod_are_counted() {
        let r = aggregate(
            &input(vec![pod("a", "1", 100, 1)]),
            Some(&usage(&[
                ("a", "1", 10, 0),
                ("a", "ghost", 10, 0),
                ("z", "ghost", 10, 0),
            ])),
            &[],
        );
        assert_eq!(r.usage_without_pod, 2);
    }

    #[test]
    fn metrics_without_pod_respect_namespace_filter() {
        let r = aggregate(
            &input(vec![pod("a", "1", 100, 1)]),
            Some(&usage(&[("z", "ghost", 10, 0)])),
            &["a".to_string()],
        );
        assert_eq!(r.usage_without_pod, 0);
    }

    #[test]
    fn empty_input_gives_empty_report() {
        let r = aggregate(&input(vec![]), None, &[]);
        assert!(r.rows.is_empty());
        assert_eq!(r.total.pods, 0);
    }

    #[test]
    fn sort_by_cpu_request_desc_with_name_tiebreak() {
        let mut r = aggregate(
            &input(vec![
                pod("a", "1", 100, 1),
                pod("b", "2", 300, 1),
                pod("c", "3", 300, 1),
            ]),
            None,
            &[],
        );
        sort_rows(&mut r, SortKey::CpuRequest, &opts(None)).unwrap();
        let names: Vec<_> = r.rows.iter().map(|s| s.namespace.as_str()).collect();
        assert_eq!(names, ["b", "c", "a"]);
    }

    #[test]
    fn sort_by_memory_request_desc() {
        let mut r = aggregate(
            &input(vec![pod("a", "1", 100, 1), pod("b", "2", 100, 5)]),
            None,
            &[],
        );
        sort_rows(&mut r, SortKey::MemoryRequest, &opts(None)).unwrap();
        assert_eq!(r.rows[0].namespace, "b");
    }

    #[test]
    fn sort_by_over_needs_usage() {
        let mut r = aggregate(&input(vec![pod("a", "1", 100, 1)]), None, &[]);
        assert!(sort_rows(&mut r, SortKey::CpuOver, &opts(None)).is_err());
        assert!(sort_rows(&mut r, SortKey::MemoryOver, &opts(None)).is_err());
        assert!(sort_rows(&mut r, SortKey::OverCost, &opts(Some(rates()))).is_err());
    }

    #[test]
    fn sort_by_cpu_and_memory_over() {
        let mut r = aggregate(
            &input(vec![pod("a", "1", 1000, 1), pod("b", "2", 500, 8)]),
            Some(&usage(&[("a", "1", 0, 0), ("b", "2", 0, 0)])),
            &[],
        );
        sort_rows(&mut r, SortKey::CpuOver, &opts(None)).unwrap();
        assert_eq!(r.rows[0].namespace, "a");
        sort_rows(&mut r, SortKey::MemoryOver, &opts(None)).unwrap();
        assert_eq!(r.rows[0].namespace, "b");
    }

    #[test]
    fn sort_by_over_cost_needs_a_rate() {
        let mut r = aggregate(
            &input(vec![pod("a", "1", 100, 1)]),
            Some(&usage(&[("a", "1", 0, 0)])),
            &[],
        );
        assert!(sort_rows(&mut r, SortKey::OverCost, &opts(None)).is_err());
    }

    #[test]
    fn sort_by_over_cost_uses_the_rate_weights() {
        // a: 1 vCPU over; b: 2 GiB over. With a memory-only rate, b costs more.
        let mut r = aggregate(
            &input(vec![pod("a", "1", 1000, 0), pod("b", "2", 0, 2)]),
            Some(&usage(&[("a", "1", 0, 0), ("b", "2", 0, 0)])),
            &[],
        );
        let mem_only = Rates {
            cpu_hour: Decimal::ZERO,
            mem_gib_hour: Decimal::ONE,
        };
        sort_rows(&mut r, SortKey::OverCost, &opts(Some(mem_only))).unwrap();
        assert_eq!(r.rows[0].namespace, "b");
    }

    #[test]
    fn table_without_usage_or_rates_has_basic_columns() {
        let r = aggregate(&input(vec![pod("a", "1", 500, 2)]), None, &[]);
        let text = render_table(&r, &opts(None)).unwrap();
        let first = text.lines().next().unwrap();
        assert_eq!(
            first.split_whitespace().collect::<Vec<_>>(),
            ["NAMESPACE", "PODS", "CPU_REQ", "MEM_REQ"]
        );
        assert!(text.contains("TOTAL"));
        assert!(!text.contains("CPU_USE"));
        assert!(!text.contains("REQ_COST"));
        assert!(text.contains("CPU in vCPU, memory in GiB."));
    }

    #[test]
    fn table_with_limits_adds_columns() {
        let r = aggregate(&input(vec![pod("a", "1", 500, 2)]), None, &[]);
        let mut o = opts(None);
        o.show_limits = true;
        let text = render_table(&r, &o).unwrap();
        assert!(text.lines().next().unwrap().contains("CPU_LIM  MEM_LIM"));
    }

    #[test]
    fn table_with_usage_and_rates_shows_costs() {
        let r = aggregate(
            &input(vec![pod("a", "1", 1000, 4)]),
            Some(&usage(&[("a", "1", 0, 0)])),
            &[],
        );
        let text = render_table(&r, &opts(Some(rates()))).unwrap();
        // 1 vCPU * 0.04 + 4 GiB * 0.01 = 0.08 for both request and over
        let row = text.lines().find(|l| l.starts_with("a ")).unwrap();
        let cells: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(&cells[cells.len() - 2..], ["0.0800", "0.0800"], "{row}");
        assert!(
            text.contains("Cost for 1 h at 0.04 USD/vCPU-h and 0.01 USD/GiB-h."),
            "{text}"
        );
        assert!(text.contains("OVER = sum over pods"));
    }

    #[test]
    fn table_notes_for_missing_metrics_and_skips() {
        let mut done = pod("a", "done", 1, 1);
        done.phase = Some("Failed".into());
        let mut inp = input(vec![pod("a", "1", 100, 1), done]);
        inp.skipped_non_pod = 3;
        let r = aggregate(&inp, Some(&usage(&[("q", "ghost", 1, 1)])), &[]);
        let text = render_table(&r, &opts(None)).unwrap();
        assert!(text.contains("1 pod(s) have no metrics"), "{text}");
        assert!(text.contains("1 metrics entr"), "{text}");
        assert!(
            text.contains("1 Succeeded/Failed pod(s) left out"),
            "{text}"
        );
        assert!(text.contains("3 non-Pod item(s) skipped"), "{text}");
    }

    #[test]
    fn table_columns_are_aligned() {
        let r = aggregate(
            &input(vec![
                pod("short", "1", 100, 1),
                pod("a-much-longer-namespace", "2", 100, 1),
            ]),
            None,
            &[],
        );
        let text = render_table(&r, &opts(None)).unwrap();
        let widths: Vec<usize> = text.lines().take(4).map(|l| l.len()).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{text}");
    }

    #[test]
    fn json_shape_without_usage() {
        let r = aggregate(&input(vec![pod("a", "1", 250, 2)]), None, &[]);
        let text = render_json(&r, &opts(None)).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["tool"], "k8s-request-bill");
        assert_eq!(v["namespaces"][0]["namespace"], "a");
        assert_eq!(v["namespaces"][0]["cpu_request_vcpu"].to_string(), "0.25");
        assert_eq!(v["namespaces"][0]["memory_request_gib"].to_string(), "2");
        assert!(v["namespaces"][0]["cpu_over_vcpu"].is_null());
        assert!(v["namespaces"][0]["request_cost"].is_null());
        assert!(v["rates"].is_null());
        assert!(v["total"].get("namespace").is_none());
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn json_numbers_are_numbers_not_strings() {
        let r = aggregate(
            &input(vec![pod("a", "1", 1000, 4)]),
            Some(&usage(&[("a", "1", 0, 0)])),
            &[],
        );
        let text = render_json(&r, &opts(Some(rates()))).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v["total"]["over_cost"].is_number());
        assert_eq!(v["total"]["over_cost"].to_string(), "0.08");
        assert_eq!(v["rates"]["cpu_hour"].to_string(), "0.04");
        assert_eq!(v["total"]["pods_without_usage"], 0);
    }

    #[test]
    fn json_notes_are_filled() {
        let mut inp = input(vec![pod("a", "1", 1, 1)]);
        inp.skipped_non_pod = 2;
        let r = aggregate(&inp, None, &[]);
        let v: Value = serde_json::from_str(&render_json(&r, &opts(None)).unwrap()).unwrap();
        assert_eq!(v["notes"]["skipped_non_pod_items"], 2);
    }

    #[test]
    fn hours_scale_cost() {
        let r = aggregate(&input(vec![pod("a", "1", 1000, 0)]), None, &[]);
        let mut o = opts(Some(rates()));
        o.hours = Decimal::from(730);
        let v: Value = serde_json::from_str(&render_json(&r, &o).unwrap()).unwrap();
        assert_eq!(v["total"]["request_cost"].to_string(), "29.2");
    }
}
