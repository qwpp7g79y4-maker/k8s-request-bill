# Quality

Dutch version: [QUALITY.nl.md](QUALITY.nl.md).

## Measured

Tested commit: `851805c` (the closed-stdout test limited to Linux), in a clean worktree.

Date (`date +%F`): 2026-11-25. Machine: linux x86_64, cargo 1.99.0, rustc
1.99.0, clippy 0.1.99, from `~/.cargo/bin`.

| Command | Result |
| --- | --- |
| `cargo fmt --check` | passed, exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | passed, exit 0, no warnings |
| `cargo test --locked` | passed, exit 0 |
| `cargo test --release --locked` | passed, exit 0 |

Result of each test run: 169 tests passed, 0 failed, 0 ignored (102 in the
library, 0 in the binary, 12 in `tests/claims.rs`, 55 in `tests/cli.rs`, 0
doc-tests).

`tests/claims.rs` counts the `#[test]` functions and fails if README or this
file state another number.

- Version 0.1.0, `rust-version` 1.85, licences `MIT OR Apache-2.0`.
- No network or Kubernetes client crates (checked against `Cargo.lock` by
  `claims::no_network_crates_in_the_lockfile`).

## Not run

This measurement ran the four commands above and nothing else. Not run:
Windows: not run. macOS: not run. The closed-stdout probe is active on Linux only; elsewhere it records nothing and the exit code stays 0. `cargo publish --dry-run`: not run. `cargo deny`: not run. `cargo audit`: not
run. A run on Rust 1.85: not run. A GitHub Actions run: not run (the earlier run 45698461182 on a6d3620 failed on macOS and macOS-intel on this test). Fuzzing: not
run. Mutation testing: not run. The CI file lists fmt, clippy, deny and audit;
the deny and audit jobs have not run for this tree.

This page records checks. It is not a guarantee and it is not a fuzz report.

## README claim to test

| README claim | Test |
| --- | --- |
| every flag in README exists, and every flag of `--help` is in README | `claims::every_flag_in_the_readme_exists`, `claims::every_flag_in_help_is_in_the_readme` |
| rate flags `--cpu-hour --mem-gib-hour --node-hour --node-vcpu --node-gib --cpu-share --hours --currency` documented | `claims::the_rate_flags_are_documented_in_the_readme` |
| README commands run on the examples, both rate styles | `claims::readme_commands_run_on_the_examples`, `claims::readme_commands_cover_both_rate_styles` |
| README output block is the real output | `claims::readme_output_block_is_the_golden_table`, `cli::golden_table_matches_examples`, `cli::golden_json_matches_examples` |
| `--cpu-hour` + `--mem-gib-hour` price requests and overprovisioning | `cli::cpu_hour_and_mem_gib_hour_price_requests`, `cli::cost_columns_cover_requests_and_overprovisioning`, `rates::cost_multiplies_by_hours` |
| a single rate flag prices only that resource | `cli::cpu_hour_alone_prices_only_cpu`, `cli::mem_gib_hour_alone_prices_only_memory`, `rates::separate_omitted_rate_is_zero` |
| `--node-hour` rate formula and `--cpu-share` | `cli::node_hour_derives_both_rates_from_the_node_shape`, `cli::node_hour_equals_the_equivalent_separate_rates`, `cli::cpu_share_moves_the_split`, `rates::node_rate_reconstructs_the_node_price`, `rates::node_rate_cpu_share_extremes` |
| `--node-hour` needs `--node-vcpu` and `--node-gib`, excludes separate rates, values validated | `cli::node_hour_needs_vcpu_and_gib`, `cli::node_hour_conflicts_with_separate_rates`, `cli::negative_rate_is_a_usage_error`, `cli::zero_node_vcpu_is_a_usage_error`, `cli::tests::*` |
| `--hours` scales the cost; `--currency` is a label | `cli::hours_scale_the_cost`, `cli::currency_is_a_label` |
| no rate flag, no cost | `cli::no_rate_flags_means_no_cost_and_null_in_json` |
| requests/limits summed per namespace in vCPU and GiB | `cli::requests_are_summed_per_namespace_in_vcpu_and_gib`, `cli::limits_flag_adds_limit_columns` |
| init containers, sidecars, overhead | `cli::init_sidecar_and_overhead_rules`, `model::tests::init_*`, `model::tests::sidecar_*`, `model::tests::overhead_*` |
| finished pods and non-pod items left out, with a note | `cli::finished_pods_are_left_out`, `cli::non_pod_items_are_skipped_with_a_note` |
| overprovisioning is clamped per pod; pods without metrics noted | `report::tests::over_is_clamped_per_pod_*`, `cli::overprovisioning_is_request_minus_usage`, `cli::pod_without_metrics_does_not_count_as_overprovisioned` |
| usage quantities (`n`, `Ki`, …) and rounding | `cli::usage_with_nanocores_and_kibibytes`, `quantity::tests::*` |
| `--namespace` filter (repeat / commas) | `cli::namespace_filter`, `cli::namespace_filter_takes_commas_and_repeats` |
| `--sort` keys, and which need usage / a rate | `claims::readme_sort_keys_are_accepted`, `cli::sort_by_*`, `report::tests::sort_*` |
| JSON fields, numbers, `null` when input is missing | `cli::json_*`, `report::tests::json_*` |
| stdin as `-`, only one input from stdin | `cli::pods_can_be_read_from_stdin`, `cli::usage_can_be_read_from_stdin`, `cli::two_stdin_inputs_fail` |
| exit codes 0 / 1 / 2 | `claims::readme_exit_codes_hold`, `cli::missing_file_is_exit_1_and_names_the_path`, `cli::unknown_flag_is_a_usage_error` |
| errors name file, pod and container | `cli::bad_quantity_names_pod_container_and_value`, `cli::metrics_file_as_pods_file_is_explained` |
| offline, no kubeconfig | `cli::works_without_network_or_cluster_config`, `claims::no_network_crates_in_the_lockfile` |
| licences, CHANGELOG 0.1.0 | `claims::licences_are_present_and_declared`, `claims::changelog_has_the_package_version` |
| test count in README and QUALITY equals the suite | `claims::test_count_in_readme_and_quality_matches_the_suite` |

## Known limits

- Only pods are read; controllers are not resolved.
- Usage is a point-in-time sample from metrics-server, not a time series.
- Memory quantities with the milli suffix are accepted and rounded to bytes.
