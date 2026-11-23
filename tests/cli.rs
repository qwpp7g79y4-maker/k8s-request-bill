//! End-to-end tests of the `k8s-request-bill` binary on saved kubectl JSON.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example(name: &str) -> String {
    root().join("examples").join(name).display().to_string()
}

fn fixture(name: &str) -> String {
    root()
        .join("tests/fixtures")
        .join(name)
        .display()
        .to_string()
}

fn bin() -> Command {
    Command::cargo_bin("k8s-request-bill").unwrap()
}

/// stdout of a successful run.
fn ok(args: &[&str]) -> String {
    let out = bin().args(args).assert().success().get_output().clone();
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn ok_json(args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.extend(["--format", "json"]);
    serde_json::from_str(&ok(&all)).unwrap()
}

/// The row of the table that starts with `name`, split on whitespace.
fn row(table: &str, name: &str) -> Vec<String> {
    table
        .lines()
        .find(|l| l.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("no row {name} in\n{table}"))
        .split_whitespace()
        .map(String::from)
        .collect()
}

fn pods() -> String {
    example("pods.json")
}

fn metrics() -> String {
    example("metrics.json")
}

// ---- help, version, golden output ------------------------------------------------

#[test]
fn help_lists_every_flag() {
    let out = ok(&["--help"]);
    for flag in [
        "--usage",
        "--namespace",
        "--cpu-hour",
        "--mem-gib-hour",
        "--node-hour",
        "--node-vcpu",
        "--node-gib",
        "--cpu-share",
        "--hours",
        "--currency",
        "--limits",
        "--sort",
        "--format",
    ] {
        assert!(out.contains(flag), "help lacks {flag}:\n{out}");
    }
}

#[test]
fn version_prints_name_and_number() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("k8s-request-bill 0.1.0"));
}

#[test]
fn golden_table_matches_examples() {
    let got = ok(&[
        &pods(),
        "--usage",
        &metrics(),
        "--cpu-hour",
        "0.031",
        "--mem-gib-hour",
        "0.004",
        "--limits",
    ]);
    let want = std::fs::read_to_string(root().join("examples/expected.txt")).unwrap();
    assert_eq!(got, want);
}

#[test]
fn golden_json_matches_examples() {
    let got = ok(&[
        &pods(),
        "--usage",
        &metrics(),
        "--cpu-hour",
        "0.031",
        "--mem-gib-hour",
        "0.004",
        "--limits",
        "--format",
        "json",
    ]);
    let want = std::fs::read_to_string(root().join("examples/expected.json")).unwrap();
    assert_eq!(got, want);
}

// ---- quantities per namespace ------------------------------------------------------

#[test]
fn without_usage_only_request_columns_are_shown() {
    let out = ok(&[&pods()]);
    let header = out.lines().next().unwrap();
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        ["NAMESPACE", "PODS", "CPU_REQ", "MEM_REQ"]
    );
    assert_eq!(row(&out, "shop"), ["shop", "3", "3.000", "9.000"]);
}

#[test]
fn requests_are_summed_per_namespace_in_vcpu_and_gib() {
    let out = ok(&[&pods()]);
    assert_eq!(row(&out, "batch")[2..], ["1.000", "2.000"]);
    assert_eq!(row(&out, "kube-system")[2..], ["0.100", "0.068"]);
    assert_eq!(row(&out, "TOTAL")[2..], ["6.100", "15.068"]);
}

#[test]
fn overprovisioning_is_request_minus_usage() {
    let out = ok(&[&pods(), "--usage", &metrics()]);
    // header: NAMESPACE PODS CPU_REQ MEM_REQ CPU_USE MEM_USE CPU_OVER MEM_OVER
    let shop = row(&out, "shop");
    assert_eq!(shop[4], "0.810"); // 120m + 90m + 600m
    assert_eq!(shop[6], "2.190"); // 380m + 410m + 1400m
    assert_eq!(shop[7], "2.434"); // 212Mi + 232Mi + 2Gi
}

#[test]
fn pod_without_metrics_does_not_count_as_overprovisioned() {
    let out = ok(&[&pods(), "--usage", &metrics()]);
    let mon = row(&out, "monitoring");
    assert_eq!(mon[2], "2.000"); // init container peak wins
    assert_eq!(mon[6], "0.000");
    assert!(out.contains("note: 1 pod(s) have no metrics"), "{out}");
}

#[test]
fn limits_flag_adds_limit_columns() {
    let plain = ok(&[&pods()]);
    assert!(!plain.contains("CPU_LIM"));
    let out = ok(&[&pods(), "--limits"]);
    assert_eq!(row(&out, "shop")[4..], ["6.000", "10.000"]);
}

#[test]
fn usage_with_nanocores_and_kibibytes() {
    let out = ok(&[
        &fixture("single-pod.json"),
        "--usage",
        &fixture("metrics-nano.json"),
    ]);
    let solo = row(&out, "solo");
    assert_eq!(solo[4..], ["0.250", "1.000", "1.750", "3.000"]);
}

#[test]
fn init_sidecar_and_overhead_rules() {
    let v = ok_json(&[&fixture("init.json")]);
    let ns = &v["namespaces"][0];
    assert_eq!(ns["pods"], 3);
    assert_eq!(ns["cpu_request_vcpu"].to_string(), "5"); // 3 + 1 + 1
    assert_eq!(ns["memory_request_gib"].to_string(), "4.25"); // 2 + 1.25 + 1
}

#[test]
fn finished_pods_are_left_out() {
    let out = ok(&[&fixture("mixed.json")]);
    assert_eq!(row(&out, "a")[1..], ["1", "1.000", "1.000"]);
    assert!(!out.lines().any(|l| l.starts_with("b ")), "{out}");
    assert!(out.contains("2 Succeeded/Failed pod(s) left out"), "{out}");
}

#[test]
fn non_pod_items_are_skipped_with_a_note() {
    let out = ok(&[&fixture("mixed.json")]);
    assert!(out.contains("2 non-Pod item(s) skipped"), "{out}");
}

#[test]
fn metrics_without_matching_pod_are_noted() {
    let out = ok(&[
        &fixture("single-pod.json"),
        "--usage",
        &fixture("metrics-extra.json"),
    ]);
    assert!(out.contains("1 metrics entr"), "{out}");
}

#[test]
fn single_pod_json_is_accepted() {
    let out = ok(&[&fixture("single-pod.json")]);
    assert_eq!(row(&out, "solo")[1..], ["1", "2.000", "4.000"]);
}

#[test]
fn empty_list_gives_a_total_of_zero() {
    let out = ok(&[&fixture("empty.json")]);
    assert_eq!(row(&out, "TOTAL")[1..], ["0", "0.000", "0.000"]);
}

// ---- rates -------------------------------------------------------------------------

#[test]
fn cpu_hour_and_mem_gib_hour_price_requests() {
    let out = ok(&[&pods(), "--cpu-hour", "0.031", "--mem-gib-hour", "0.004"]);
    // shop: 3 vCPU * 0.031 + 9 GiB * 0.004 = 0.129
    assert_eq!(row(&out, "shop").last().unwrap(), "0.1290");
    assert!(
        out.contains("0.031 USD/vCPU-h and 0.004 USD/GiB-h"),
        "{out}"
    );
}

#[test]
fn cost_columns_cover_requests_and_overprovisioning() {
    let out = ok(&[
        &pods(),
        "--usage",
        &metrics(),
        "--cpu-hour",
        "0.031",
        "--mem-gib-hour",
        "0.004",
    ]);
    let header = out.lines().next().unwrap();
    assert!(header.ends_with("REQ_COST  OVER_COST"), "{header}");
    let shop = row(&out, "shop");
    assert_eq!(shop[shop.len() - 2..], ["0.1290", "0.0776"]);
}

#[test]
fn cpu_hour_alone_prices_only_cpu() {
    let out = ok(&[&pods(), "--cpu-hour", "0.031"]);
    assert_eq!(row(&out, "shop").last().unwrap(), "0.0930");
}

#[test]
fn mem_gib_hour_alone_prices_only_memory() {
    let out = ok(&[&pods(), "--mem-gib-hour", "0.004"]);
    assert_eq!(row(&out, "shop").last().unwrap(), "0.0360");
}

#[test]
fn node_hour_derives_both_rates_from_the_node_shape() {
    let out = ok(&[
        &pods(),
        "--node-hour",
        "0.192",
        "--node-vcpu",
        "4",
        "--node-gib",
        "16",
    ]);
    assert!(
        out.contains("0.024 USD/vCPU-h and 0.006 USD/GiB-h"),
        "{out}"
    );
}

#[test]
fn node_hour_equals_the_equivalent_separate_rates() {
    let node = ok(&[
        &pods(),
        "--usage",
        &metrics(),
        "--node-hour",
        "0.192",
        "--node-vcpu",
        "4",
        "--node-gib",
        "16",
    ]);
    let separate = ok(&[
        &pods(),
        "--usage",
        &metrics(),
        "--cpu-hour",
        "0.024",
        "--mem-gib-hour",
        "0.006",
    ]);
    assert_eq!(node, separate);
}

#[test]
fn cpu_share_moves_the_split() {
    let all_cpu = ok(&[
        &pods(),
        "--node-hour",
        "0.4",
        "--node-vcpu",
        "4",
        "--node-gib",
        "16",
        "--cpu-share",
        "1",
    ]);
    assert!(
        all_cpu.contains("0.1 USD/vCPU-h and 0 USD/GiB-h"),
        "{all_cpu}"
    );
    let all_mem = ok(&[
        &pods(),
        "--node-hour",
        "0.4",
        "--node-vcpu",
        "4",
        "--node-gib",
        "16",
        "--cpu-share",
        "0",
    ]);
    assert!(
        all_mem.contains("0 USD/vCPU-h and 0.025 USD/GiB-h"),
        "{all_mem}"
    );
}

#[test]
fn hours_scale_the_cost() {
    let out = ok(&[
        &pods(),
        "--cpu-hour",
        "0.031",
        "--mem-gib-hour",
        "0.004",
        "--hours",
        "730",
    ]);
    assert_eq!(row(&out, "shop").last().unwrap(), "94.1700");
    assert!(out.contains("Cost for 730 h"), "{out}");
}

#[test]
fn currency_is_a_label() {
    let out = ok(&[&pods(), "--cpu-hour", "1", "--currency", "EUR"]);
    assert!(out.contains("EUR/vCPU-h"), "{out}");
    assert!(!out.contains("USD"), "{out}");
}

#[test]
fn no_rate_flags_means_no_cost_and_null_in_json() {
    let out = ok(&[&pods()]);
    assert!(!out.contains("COST"));
    let v = ok_json(&[&pods()]);
    assert!(v["rates"].is_null());
    assert!(v["total"]["request_cost"].is_null());
}

#[test]
fn json_carries_rates_and_costs_as_numbers() {
    let v = ok_json(&[
        &pods(),
        "--usage",
        &metrics(),
        "--cpu-hour",
        "0.031",
        "--mem-gib-hour",
        "0.004",
    ]);
    assert_eq!(v["rates"]["cpu_hour"].to_string(), "0.031");
    assert_eq!(v["rates"]["mem_gib_hour"].to_string(), "0.004");
    assert!(v["total"]["request_cost"].is_number());
    assert!(v["total"]["over_cost"].is_number());
    assert_eq!(v["currency"], "USD");
    assert_eq!(v["hours"].to_string(), "1");
}

// ---- JSON --------------------------------------------------------------------------

#[test]
fn json_lists_namespaces_and_total() {
    let v = ok_json(&[&pods(), "--usage", &metrics()]);
    let names: Vec<_> = v["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["namespace"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["batch", "kube-system", "monitoring", "shop"]);
    assert_eq!(v["total"]["pods"], 6);
    assert_eq!(v["total"]["pods_without_usage"], 1);
    assert_eq!(v["notes"]["skipped_finished_pods"], 1);
}

#[test]
fn json_without_usage_has_null_usage_fields() {
    let v = ok_json(&[&pods()]);
    assert!(v["total"]["cpu_usage_vcpu"].is_null());
    assert!(v["total"]["cpu_over_vcpu"].is_null());
    assert!(v["total"]["pods_without_usage"].is_null());
}

// ---- filters, sorting, stdin -------------------------------------------------------

#[test]
fn namespace_filter() {
    let out = ok(&[&pods(), "--namespace", "shop"]);
    assert!(out.lines().any(|l| l.starts_with("shop ")));
    assert!(!out.lines().any(|l| l.starts_with("batch ")));
    assert_eq!(row(&out, "TOTAL")[1], "3");
}

#[test]
fn namespace_filter_takes_commas_and_repeats() {
    let out = ok(&[&pods(), "-n", "shop,batch", "-n", "monitoring"]);
    assert_eq!(row(&out, "TOTAL")[1], "5");
    assert!(!out.lines().any(|l| l.starts_with("kube-system ")));
}

#[test]
fn unknown_namespace_gives_an_empty_report() {
    let out = ok(&[&pods(), "-n", "nope"]);
    assert_eq!(row(&out, "TOTAL")[1], "0");
}

#[test]
fn sort_by_cpu_request_puts_the_largest_first() {
    let out = ok(&[&pods(), "--sort", "cpu-request"]);
    let first = out.lines().nth(1).unwrap();
    assert!(first.starts_with("shop "), "{first}");
}

#[test]
fn sort_by_memory_request() {
    let out = ok(&[&pods(), "--sort", "memory-request"]);
    assert!(out.lines().nth(1).unwrap().starts_with("shop "));
}

#[test]
fn sort_by_over_cost_with_usage_and_rates() {
    let out = ok(&[
        &pods(),
        "--usage",
        &metrics(),
        "--cpu-hour",
        "0.031",
        "--mem-gib-hour",
        "0.004",
        "--sort",
        "over-cost",
    ]);
    let names: Vec<_> = out
        .lines()
        .skip(1)
        .take(4)
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(names, ["shop", "batch", "kube-system", "monitoring"]);
}

#[test]
fn sort_by_cpu_over_with_usage() {
    let out = ok(&[&pods(), "--usage", &metrics(), "--sort", "cpu-over"]);
    assert!(out.lines().nth(1).unwrap().starts_with("shop "));
}

#[test]
fn sort_by_over_without_usage_fails() {
    bin()
        .args([&pods(), "--sort", "cpu-over"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("needs --usage"));
}

#[test]
fn pods_can_be_read_from_stdin() {
    let text = std::fs::read_to_string(root().join("examples/pods.json")).unwrap();
    let out = bin()
        .arg("-")
        .write_stdin(text)
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&out.stdout).contains("shop"));
}

#[test]
fn usage_can_be_read_from_stdin() {
    let text = std::fs::read_to_string(root().join("examples/metrics.json")).unwrap();
    let out = bin()
        .args([&pods(), "--usage", "-"])
        .write_stdin(text)
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&out.stdout).contains("CPU_OVER"));
}

#[test]
fn two_stdin_inputs_fail() {
    bin()
        .args(["-", "--usage", "-"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("only one input"));
}

// ---- errors ------------------------------------------------------------------------

#[test]
fn missing_file_is_exit_1_and_names_the_path() {
    bin()
        .arg("/no/such/pods.json")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("/no/such/pods.json"));
}

#[test]
fn invalid_json_is_exit_1() {
    bin()
        .arg("-")
        .write_stdin("{nope")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid JSON"));
}

#[test]
fn metrics_file_as_pods_file_is_explained() {
    bin()
        .arg(metrics())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("metrics file, not a pods file"));
}

#[test]
fn bad_quantity_names_pod_container_and_value() {
    bin()
        .arg(fixture("bad-quantity.json"))
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicate::str::contains("ns/bad")
                .and(predicate::str::contains("web"))
                .and(predicate::str::contains("lots")),
        );
}

#[test]
fn bad_usage_file_names_the_file() {
    bin()
        .args([&pods(), "--usage", &pods()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("PodMetricsList"));
}

#[test]
fn errors_leave_stdout_empty() {
    bin()
        .arg("-")
        .write_stdin("[")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty());
}

#[test]
fn missing_pods_argument_is_a_usage_error() {
    bin().assert().failure().code(2);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    bin().args([&pods(), "--nope"]).assert().failure().code(2);
}

#[test]
fn node_hour_conflicts_with_separate_rates() {
    bin()
        .args([
            &pods(),
            "--cpu-hour",
            "1",
            "--node-hour",
            "1",
            "--node-vcpu",
            "2",
            "--node-gib",
            "8",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn node_hour_needs_vcpu_and_gib() {
    bin()
        .args([&pods(), "--node-hour", "1"])
        .assert()
        .failure()
        .code(2);
    bin()
        .args([&pods(), "--node-hour", "1", "--node-vcpu", "2"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn negative_rate_is_a_usage_error() {
    bin()
        .args([&pods(), "--cpu-hour", "-0.5"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn zero_node_vcpu_is_a_usage_error() {
    bin()
        .args([
            &pods(),
            "--node-hour",
            "1",
            "--node-vcpu",
            "0",
            "--node-gib",
            "8",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn works_without_network_or_cluster_config() {
    // No kubeconfig, no proxy: the tool only reads the files it is given.
    bin()
        .arg(pods())
        .env_remove("KUBECONFIG")
        .env("HOME", "/nonexistent")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success();
}
