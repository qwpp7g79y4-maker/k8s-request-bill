# k8s-request-bill

Offline overprovisioning report for Kubernetes. It reads saved `kubectl` JSON
(pod requests and limits, and optionally usage) and shows, **per namespace**,
what is requested, what is used, and what is left over, in **vCPU** and **GiB**,
plus a **cost at a node rate you supply**.

The tests and the measured numbers are in `QUALITY.md`.

It reads files (or stdin) only. It makes no network calls, never talks to a
cluster, needs no kubeconfig, and ships no price list: the tariff is yours.

## Installation

Version 0.1.0. Rust 1.85 or newer. From a checkout:

```sh
cargo install --path .
```

Once the crate is on crates.io, `cargo install k8s-request-bill` will work too.

## Quick start

Save the two inputs once (this is the only step that touches a cluster, and you
run it yourself):

```sh
kubectl get pods -A -o json > pods.json
kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods > metrics.json   # optional
```

Then run the tool on the saved files:

```sh
k8s-request-bill pods.json --usage metrics.json
```

## Aan de slag in 5 minuten

You do not need a cluster to try it. The repository holds a small sample in
`examples/`. The commands below only read it and write nothing.

1. Build and install from the checkout, as under Installation.
2. Requests only:

```sh
k8s-request-bill examples/pods.json
```

3. Add usage, to see what is left over:

```sh
k8s-request-bill examples/pods.json --usage examples/metrics.json
```

4. Add a price, to see what the leftover costs:

```sh
k8s-request-bill examples/pods.json --usage examples/metrics.json --cpu-hour 0.031 --mem-gib-hour 0.004
```

5. Point it at your own `pods.json` and `metrics.json`, and use your own rates.

## Usage

```sh
# requests only
k8s-request-bill pods.json

# requests vs usage, priced per vCPU-hour and per GiB-hour
k8s-request-bill pods.json --usage metrics.json --cpu-hour 0.031 --mem-gib-hour 0.004

# one combined rate: the hourly price of a node, split by its shape
k8s-request-bill pods.json --usage metrics.json \
  --node-hour 0.192 --node-vcpu 4 --node-gib 16 --hours 730
```

## Examples

`examples/` holds a small `pods.json` and `metrics.json`. `examples/expected.txt`
and `examples/expected.json` are the exact output of

```sh
k8s-request-bill examples/pods.json --usage examples/metrics.json \
  --cpu-hour 0.031 --mem-gib-hour 0.004 --limits
```

```text
NAMESPACE    PODS  CPU_REQ  MEM_REQ  CPU_LIM  MEM_LIM  CPU_USE  MEM_USE  CPU_OVER  MEM_OVER  REQ_COST  OVER_COST
batch           1    1.000    2.000    0.000    0.000    0.050    0.098     0.950     1.902    0.0390     0.0371
kube-system     1    0.100    0.068    0.000    0.166    0.003    0.020     0.097     0.049    0.0034     0.0032
monitoring      1    2.000    4.000    0.000    0.000    0.000    0.000     0.000     0.000    0.0780     0.0000
shop            3    3.000    9.000    6.000   10.000    0.810    6.566     2.190     2.434    0.1290     0.0776
TOTAL           6    6.100   15.068    6.000   10.166    0.863    6.684     3.237     4.385    0.2494     0.1179
```

## Options

### Rates

Prices are plain decimals in whatever currency you use; `--currency` only sets
the label printed next to them. Without any rate flag no cost is shown.

| Flag | Meaning |
| --- | --- |
| `--cpu-hour PRICE` | price of one vCPU for one hour |
| `--mem-gib-hour PRICE` | price of one GiB of memory for one hour |
| `--node-hour PRICE` | one combined rate: the hourly price of a whole node. Needs `--node-vcpu` and `--node-gib`. Cannot be combined with `--cpu-hour` / `--mem-gib-hour` |
| `--node-vcpu N` | vCPUs of that node (greater than zero) |
| `--node-gib N` | GiB of memory of that node (greater than zero) |
| `--cpu-share F` | share of the node price attributed to CPU, `0` to `1`, default `0.5`; the rest goes to memory |
| `--hours H` | hours the cost covers, default `1` (`730` is about a month) |
| `--currency CODE` | label only, default `USD`; no conversion |

With `--node-hour`, the CPU rate is `node-hour x cpu-share / node-vcpu` and the
memory rate is `node-hour x (1 - cpu-share) / node-gib`. With only one of
`--cpu-hour` and `--mem-gib-hour`, the other resource is priced at zero.

`REQ_COST` is the cost of everything requested; `OVER_COST` (with `--usage`) is
the cost of the overprovisioned part.

### Other options

| Flag | Meaning |
| --- | --- |
| `PODS` | pods JSON file, `-` for stdin |
| `-u, --usage FILE` | metrics JSON file, `-` for stdin (not both) |
| `-n, --namespace NS` | only these namespaces; repeat or separate with commas |
| `--limits` | add `CPU_LIM` and `MEM_LIM` columns |
| `--sort KEY` | `name` (default), `cpu-request`, `memory-request`, `cpu-over`, `memory-over`, `over-cost`; the last three need `--usage`, `over-cost` also needs a rate |
| `--format table\|json` | output format, default `table` |

## How it counts

- **Requests per pod** follow the scheduler rule: the larger of the
  init-container peak and the sum of the app containers, where restartable init
  containers (sidecars) stay running, plus `spec.overhead`. Limits use the same
  rule; a container without a limit adds nothing to `CPU_LIM` / `MEM_LIM`.
- Pods in phase `Succeeded` or `Failed` are left out (they hold no requests).
  Items in a list that are not pods are skipped. Both are counted in a note.
- **Usage** is the sum of the container usage in the metrics file, matched to
  pods by namespace and name.
- **Overprovisioning** is the sum over pods of `max(request - usage, 0)`. It is
  clamped per pod, so a pod that bursts above its request does not hide the
  slack of another pod. Pods without a metrics entry count in the requests but
  not in `USE` or `OVER`; a note says how many.
- Quantities: `m`, `u`, `n` for CPU; `Ki` to `Ei`, `k` to `E`, `m` and exponent
  notation for memory. Negative values are rejected.
- Table numbers are rounded to three decimals (costs to four); JSON carries
  more precision (vCPU exact, GiB and costs to six decimals) as numbers.

## JSON

`--format json` prints one document with `tool`, `version`, `hours`,
`currency`, `rates` (or `null`), `namespaces`, `total` and `notes`. Fields that
need usage or a rate are `null` when that input was not given.

## Exit codes

`0` success, `1` unreadable or invalid input (the message names the file, pod
and container), `2` bad command line.

## Limitations

No live cluster access, no price list, no controllers (deployments, jobs), no
VPA or HPA advice, no node-level bin-packing. Only the pods you give it.

## Maintenance

Small, single-purpose tool, maintained by one person. Bugs and patches come in
through Issues; see `CONTRIBUTING.md`. Security reports follow `SECURITY.md`.
Changes are listed in `CHANGELOG.md`. A patch should come with a test.

## Quality

167 tests; formatting and clippy with `-D warnings` are clean.
See [QUALITY.md](QUALITY.md) for the claim-to-test map.

## License

MIT OR Apache-2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`.
