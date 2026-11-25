# Kwaliteit

Engelse versie: [QUALITY.md](QUALITY.md).

## Gemeten

Geteste commit: `db1e7bb` (`origin/main`), in een schone worktree.

Datum (`date +%F`): 2026-11-25. Machine: linux x86_64, cargo 1.99.0, rustc
1.99.0, clippy 0.1.99, uit `~/.cargo/bin`.

| Opdracht | Uitkomst |
| --- | --- |
| `cargo fmt --check` | geslaagd, exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | geslaagd, exit 0, geen waarschuwingen |
| `cargo test --locked` | geslaagd, exit 0 |
| `cargo test --release --locked` | geslaagd, exit 0 |

Uitkomst van elke testrun: 169 tests geslaagd, 0 mislukt, 0 genegeerd (102 in
de bibliotheek, 0 in het binary, 12 in `tests/claims.rs`, 53 in
`tests/cli.rs`, 0 doctests).

`tests/claims.rs` telt de `#[test]`-functies en faalt als README of dit
bestand een ander getal noemt.

- Versie 0.1.0, `rust-version` 1.85, licenties `MIT OR Apache-2.0`.
- Geen netwerk- of Kubernetes-clientcrates (gecontroleerd tegen `Cargo.lock`
  door `claims::no_network_crates_in_the_lockfile`).

## Niet uitgevoerd

Deze meting draaide alleen de vier opdrachten hierboven. Niet uitgevoerd:
`cargo publish --dry-run`: not run. `cargo deny`: not run. `cargo audit`: not
run. Een run op Rust 1.85: not run. Een GitHub Actions-run: not run. Fuzzing:
not run. Mutatietests: not run. Het CI-bestand noemt fmt, clippy, deny en
audit; de deny- en audit-jobs zijn voor deze boom niet gedraaid.

Deze pagina legt controles vast. Het is geen garantie en geen fuzzrapport.

## Bewering uit de README naar test

| Bewering in de README | Test |
| --- | --- |
| elke vlag in de README bestaat, en elke vlag van `--help` staat in de README | `claims::every_flag_in_the_readme_exists`, `claims::every_flag_in_help_is_in_the_readme` |
| tariefvlaggen `--cpu-hour --mem-gib-hour --node-hour --node-vcpu --node-gib --cpu-share --hours --currency` beschreven | `claims::the_rate_flags_are_documented_in_the_readme` |
| README-opdrachten draaien op de voorbeelden, beide tariefstijlen | `claims::readme_commands_run_on_the_examples`, `claims::readme_commands_cover_both_rate_styles` |
| het uitvoerblok in de README is de echte uitvoer | `claims::readme_output_block_is_the_golden_table`, `cli::golden_table_matches_examples`, `cli::golden_json_matches_examples` |
| `--cpu-hour` + `--mem-gib-hour` prijzen requests en overprovisioning | `cli::cpu_hour_and_mem_gib_hour_price_requests`, `cli::cost_columns_cover_requests_and_overprovisioning`, `rates::cost_multiplies_by_hours` |
| één tariefvlag prijst alleen die resource | `cli::cpu_hour_alone_prices_only_cpu`, `cli::mem_gib_hour_alone_prices_only_memory`, `rates::separate_omitted_rate_is_zero` |
| formule van `--node-hour` en `--cpu-share` | `cli::node_hour_derives_both_rates_from_the_node_shape`, `cli::node_hour_equals_the_equivalent_separate_rates`, `cli::cpu_share_moves_the_split`, `rates::node_rate_reconstructs_the_node_price`, `rates::node_rate_cpu_share_extremes` |
| `--node-hour` vraagt `--node-vcpu` en `--node-gib`, sluit losse tarieven uit, waarden gecontroleerd | `cli::node_hour_needs_vcpu_and_gib`, `cli::node_hour_conflicts_with_separate_rates`, `cli::negative_rate_is_a_usage_error`, `cli::zero_node_vcpu_is_a_usage_error`, `cli::tests::*` |
| `--hours` schaalt de kost; `--currency` is een label | `cli::hours_scale_the_cost`, `cli::currency_is_a_label` |
| geen tariefvlag, geen kost | `cli::no_rate_flags_means_no_cost_and_null_in_json` |
| requests en limits per namespace opgeteld in vCPU en GiB | `cli::requests_are_summed_per_namespace_in_vcpu_and_gib`, `cli::limits_flag_adds_limit_columns` |
| init-containers, sidecars, overhead | `cli::init_sidecar_and_overhead_rules`, `model::tests::init_*`, `model::tests::sidecar_*`, `model::tests::overhead_*` |
| afgeronde pods en niet-pods weggelaten, met notitie | `cli::finished_pods_are_left_out`, `cli::non_pod_items_are_skipped_with_a_note` |
| overprovisioning per pod afgekapt; pods zonder metrics gemeld | `report::tests::over_is_clamped_per_pod_*`, `cli::overprovisioning_is_request_minus_usage`, `cli::pod_without_metrics_does_not_count_as_overprovisioned` |
| hoeveelheden (`n`, `Ki`, ...) en afronding | `cli::usage_with_nanocores_and_kibibytes`, `quantity::tests::*` |
| `--namespace`-filter (herhalen of komma's) | `cli::namespace_filter`, `cli::namespace_filter_takes_commas_and_repeats` |
| `--sort`-sleutels, en welke gebruik of tarief vragen | `claims::readme_sort_keys_are_accepted`, `cli::sort_by_*`, `report::tests::sort_*` |
| JSON-velden, getallen, `null` bij ontbrekende invoer | `cli::json_*`, `report::tests::json_*` |
| stdin als `-`, maar één invoer van stdin | `cli::pods_can_be_read_from_stdin`, `cli::usage_can_be_read_from_stdin`, `cli::two_stdin_inputs_fail` |
| afsluitcodes 0 / 1 / 2 | `claims::readme_exit_codes_hold`, `cli::missing_file_is_exit_1_and_names_the_path`, `cli::unknown_flag_is_a_usage_error` |
| fouten noemen bestand, pod en container | `cli::bad_quantity_names_pod_container_and_value`, `cli::metrics_file_as_pods_file_is_explained` |
| offline, geen kubeconfig | `cli::works_without_network_or_cluster_config`, `claims::no_network_crates_in_the_lockfile` |
| licenties, CHANGELOG 0.1.0 | `claims::licences_are_present_and_declared`, `claims::changelog_has_the_package_version` |
| aantal tests in README en QUALITY gelijk aan de suite | `claims::test_count_in_readme_and_quality_matches_the_suite` |

## Bekende grenzen

- Alleen pods worden gelezen; controllers worden niet uitgezocht.
- Gebruik is een momentopname van metrics-server, geen tijdreeks.
- Geheugenhoeveelheden met het milli-achtervoegsel worden geaccepteerd en op
  bytes afgerond.
