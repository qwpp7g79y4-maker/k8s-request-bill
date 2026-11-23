//! Command line: arguments and the `execute` entry point (no process I/O except reading files).

use std::io::Read;

use clap::{Parser, ValueEnum};
use rust_decimal::Decimal;

use crate::model::{parse_pods, parse_usage};
use crate::rates::{parse_fraction, parse_positive, parse_price, Rates};
use crate::report::{aggregate, render_json, render_table, sort_rows, RenderOptions, SortKey};
use crate::Error;

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Aligned text table (default).
    Table,
    /// One JSON document.
    Json,
}

/// Arguments of `k8s-request-bill`.
#[derive(Debug, Parser)]
#[command(
    name = "k8s-request-bill",
    version,
    about = "Offline overprovisioning of Kubernetes requests vs usage, per namespace, from saved kubectl JSON",
    long_about = "Reads saved kubectl JSON and shows requests, limits, usage and overprovisioning \
per namespace in vCPU and GiB, plus a cost at a node rate you supply. \
It never talks to a cluster and ships no price list.",
    after_help = "Examples:\n  \
kubectl get pods -A -o json > pods.json\n  \
kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods > metrics.json\n  \
k8s-request-bill pods.json --usage metrics.json --cpu-hour 0.031 --mem-gib-hour 0.004\n  \
k8s-request-bill pods.json --usage metrics.json --node-hour 0.192 --node-vcpu 4 --node-gib 16 --hours 730"
)]
pub struct Cli {
    /// Pods JSON from `kubectl get pods -A -o json` (`-` reads stdin).
    #[arg(value_name = "PODS")]
    pub pods: String,

    /// Metrics JSON (`PodMetricsList`) from `kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods`
    /// (`-` reads stdin). Enables the usage and overprovisioning columns.
    #[arg(short, long, value_name = "FILE")]
    pub usage: Option<String>,

    /// Only these namespaces (repeat the flag or separate with commas).
    #[arg(short, long, value_name = "NS", value_delimiter = ',')]
    pub namespace: Vec<String>,

    /// Price of one vCPU for one hour.
    #[arg(long, value_name = "PRICE", value_parser = parse_price,
          conflicts_with_all = ["node_hour"])]
    pub cpu_hour: Option<Decimal>,

    /// Price of one GiB of memory for one hour.
    #[arg(long, value_name = "PRICE", value_parser = parse_price,
          conflicts_with_all = ["node_hour"])]
    pub mem_gib_hour: Option<Decimal>,

    /// One combined rate: the hourly price of a whole node. Needs --node-vcpu and --node-gib.
    #[arg(long, value_name = "PRICE", value_parser = parse_price,
          requires_all = ["node_vcpu", "node_gib"])]
    pub node_hour: Option<Decimal>,

    /// vCPUs of that node.
    #[arg(long, value_name = "N", value_parser = parse_positive, requires = "node_hour")]
    pub node_vcpu: Option<Decimal>,

    /// GiB of memory of that node.
    #[arg(long, value_name = "N", value_parser = parse_positive, requires = "node_hour")]
    pub node_gib: Option<Decimal>,

    /// Share of the node price attributed to CPU (the rest goes to memory).
    #[arg(long, value_name = "0..1", value_parser = parse_fraction,
          default_value = "0.5", requires = "node_hour")]
    pub cpu_share: Decimal,

    /// Hours the cost covers (730 is about one month).
    #[arg(long, value_name = "H", value_parser = parse_price, default_value = "1")]
    pub hours: Decimal,

    /// Currency label printed with the cost; no conversion is done.
    #[arg(long, value_name = "CODE", default_value = "USD")]
    pub currency: String,

    /// Add CPU_LIM and MEM_LIM columns.
    #[arg(long)]
    pub limits: bool,

    /// Row order.
    #[arg(long, value_enum, default_value = "name")]
    pub sort: SortKey,

    /// Output format.
    #[arg(long, value_enum, default_value = "table")]
    pub format: Format,
}

impl Cli {
    /// The rates the flags describe, if any.
    pub fn rates(&self) -> Result<Option<Rates>, Error> {
        match self.node_hour {
            Some(price) => {
                // clap guarantees these are present when --node-hour is
                let vcpu = self
                    .node_vcpu
                    .ok_or_else(|| Error::msg("--node-hour needs --node-vcpu"))?;
                let gib = self
                    .node_gib
                    .ok_or_else(|| Error::msg("--node-hour needs --node-gib"))?;
                Ok(Some(Rates::from_node(price, vcpu, gib, self.cpu_share)?))
            }
            None => Ok(Rates::separate(self.cpu_hour, self.mem_gib_hour)),
        }
    }
}

fn read_input(path: &str, stdin: &mut dyn Read) -> Result<String, Error> {
    if path == "-" {
        let mut text = String::new();
        stdin
            .read_to_string(&mut text)
            .map_err(|e| Error::msg(format!("cannot read stdin: {e}")))?;
        Ok(text)
    } else {
        std::fs::read_to_string(path).map_err(|e| Error::msg(format!("cannot read {path}: {e}")))
    }
}

/// Run the tool and return the text to print.
pub fn execute(cli: &Cli, stdin: &mut dyn Read) -> Result<String, Error> {
    if cli.pods == "-" && cli.usage.as_deref() == Some("-") {
        return Err(Error::msg("only one input can be read from stdin"));
    }
    let rates = cli.rates()?;
    let pods = parse_pods(&read_input(&cli.pods, stdin)?)
        .map_err(|e| Error::msg(format!("{}: {e}", cli.pods)))?;
    let usage = match &cli.usage {
        Some(path) => Some(
            parse_usage(&read_input(path, stdin)?)
                .map_err(|e| Error::msg(format!("{path}: {e}")))?,
        ),
        None => None,
    };
    let opts = RenderOptions {
        rates,
        hours: cli.hours,
        currency: cli.currency.clone(),
        show_limits: cli.limits,
    };
    let mut report = aggregate(&pods, usage.as_ref(), &cli.namespace);
    sort_rows(&mut report, cli.sort, &opts)?;
    match cli.format {
        Format::Table => render_table(&report, &opts),
        Format::Json => render_json(&report, &opts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut full = vec!["k8s-request-bill"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full)
    }

    fn d(t: &str) -> Decimal {
        Decimal::from_str(t).unwrap()
    }

    #[test]
    fn minimal_invocation() {
        let cli = parse(&["pods.json"]).unwrap();
        assert_eq!(cli.pods, "pods.json");
        assert_eq!(cli.format, Format::Table);
        assert_eq!(cli.sort, SortKey::Name);
        assert_eq!(cli.hours, Decimal::ONE);
        assert_eq!(cli.currency, "USD");
        assert!(cli.rates().unwrap().is_none());
    }

    #[test]
    fn pods_argument_is_required() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn separate_rate_flags() {
        let cli = parse(&["p", "--cpu-hour", "0.031", "--mem-gib-hour", "0.004"]).unwrap();
        let r = cli.rates().unwrap().unwrap();
        assert_eq!(r.cpu_hour, d("0.031"));
        assert_eq!(r.mem_gib_hour, d("0.004"));
    }

    #[test]
    fn a_single_rate_flag_prices_only_that_resource() {
        let cli = parse(&["p", "--cpu-hour", "0.05"]).unwrap();
        let r = cli.rates().unwrap().unwrap();
        assert_eq!(r.mem_gib_hour, Decimal::ZERO);
    }

    #[test]
    fn combined_node_rate() {
        let cli = parse(&[
            "p",
            "--node-hour",
            "0.192",
            "--node-vcpu",
            "4",
            "--node-gib",
            "16",
        ])
        .unwrap();
        let r = cli.rates().unwrap().unwrap();
        assert_eq!(r.cpu_hour, d("0.024"));
        assert_eq!(r.mem_gib_hour, d("0.006"));
    }

    #[test]
    fn combined_node_rate_with_cpu_share() {
        let cli = parse(&[
            "p",
            "--node-hour",
            "1",
            "--node-vcpu",
            "2",
            "--node-gib",
            "8",
            "--cpu-share",
            "0.75",
        ])
        .unwrap();
        let r = cli.rates().unwrap().unwrap();
        assert_eq!(r.cpu_hour, d("0.375"));
        assert_eq!(r.mem_gib_hour, d("0.03125"));
    }

    #[test]
    fn node_hour_needs_node_shape() {
        assert!(parse(&["p", "--node-hour", "1"]).is_err());
        assert!(parse(&["p", "--node-hour", "1", "--node-vcpu", "2"]).is_err());
        assert!(parse(&["p", "--node-hour", "1", "--node-gib", "2"]).is_err());
    }

    #[test]
    fn node_shape_flags_need_node_hour() {
        assert!(parse(&["p", "--node-vcpu", "2"]).is_err());
        assert!(parse(&["p", "--node-gib", "2"]).is_err());
        assert!(parse(&["p", "--cpu-share", "0.3"]).is_err());
    }

    #[test]
    fn node_hour_conflicts_with_separate_rates() {
        let node = ["--node-hour", "1", "--node-vcpu", "2", "--node-gib", "8"];
        let mut a = vec!["p", "--cpu-hour", "1"];
        a.extend(node);
        assert!(parse(&a).is_err());
        let mut b = vec!["p", "--mem-gib-hour", "1"];
        b.extend(node);
        assert!(parse(&b).is_err());
    }

    #[test]
    fn bad_numbers_are_rejected() {
        assert!(parse(&["p", "--cpu-hour", "-1"]).is_err());
        assert!(parse(&["p", "--cpu-hour", "x"]).is_err());
        assert!(parse(&["p", "--hours", "-2"]).is_err());
        assert!(parse(&[
            "p",
            "--node-hour",
            "1",
            "--node-vcpu",
            "0",
            "--node-gib",
            "1"
        ])
        .is_err());
        assert!(parse(&[
            "p",
            "--node-hour",
            "1",
            "--node-vcpu",
            "1",
            "--node-gib",
            "1",
            "--cpu-share",
            "2"
        ])
        .is_err());
    }

    #[test]
    fn namespaces_repeat_and_split_on_commas() {
        let cli = parse(&["p", "-n", "a,b", "--namespace", "c"]).unwrap();
        assert_eq!(cli.namespace, ["a", "b", "c"]);
    }

    #[test]
    fn short_flags() {
        let cli = parse(&["p", "-u", "m.json"]).unwrap();
        assert_eq!(cli.usage.as_deref(), Some("m.json"));
    }

    #[test]
    fn sort_and_format_values() {
        let cli = parse(&["p", "--sort", "over-cost", "--format", "json"]).unwrap();
        assert_eq!(cli.sort, SortKey::OverCost);
        assert_eq!(cli.format, Format::Json);
        assert!(parse(&["p", "--sort", "price"]).is_err());
        assert!(parse(&["p", "--format", "xml"]).is_err());
    }

    #[test]
    fn two_stdin_inputs_are_refused() {
        let cli = parse(&["-", "--usage", "-"]).unwrap();
        let err = execute(&cli, &mut std::io::empty()).unwrap_err();
        assert!(err.message().contains("stdin"));
    }

    #[test]
    fn missing_file_names_the_path() {
        let cli = parse(&["/nonexistent/pods.json"]).unwrap();
        let err = execute(&cli, &mut std::io::empty()).unwrap_err();
        assert!(err.message().contains("/nonexistent/pods.json"));
    }

    #[test]
    fn stdin_input_is_read() {
        let cli = parse(&["-"]).unwrap();
        let mut input: &[u8] = br#"{"kind":"List","items":[]}"#;
        let out = execute(&cli, &mut input).unwrap();
        assert!(out.contains("TOTAL"));
    }
}
