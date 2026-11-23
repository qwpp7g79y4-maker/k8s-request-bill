# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-09

### Added

- Offline reader for `kubectl get pods -A -o json` (`List`, `PodList`, single
  `Pod`) and for metrics-server `PodMetricsList` JSON, from a file or stdin.
- Per-namespace requests, limits, usage and overprovisioning in vCPU and GiB;
  effective pod requests follow the scheduler rule for init containers,
  sidecars and overhead; finished pods are left out.
- Cost at a rate you supply: `--cpu-hour` and `--mem-gib-hour`, or one combined
  node rate with `--node-hour`, `--node-vcpu`, `--node-gib` and `--cpu-share`;
  `--hours` and `--currency`.
- `--namespace` filter, `--limits` columns, `--sort`, `--format table|json`.
- Kubernetes Quantity parsing for CPU (`n`, `u`, `m`) and memory (binary,
  decimal, milli and exponent forms).
- Dual licence MIT OR Apache-2.0.
