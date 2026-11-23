//! Node rates supplied by the user. The tool ships no price list.

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::quantity::{bytes_to_gib, nano_to_vcpu};
use crate::Error;

/// Price per vCPU-hour and per GiB-hour, in a currency the user chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    /// Price of one vCPU for one hour.
    pub cpu_hour: Decimal,
    /// Price of one GiB for one hour.
    pub mem_gib_hour: Decimal,
}

impl Rates {
    /// Rates from `--cpu-hour` and/or `--mem-gib-hour`; an omitted rate is zero,
    /// so a single flag prices only that resource.
    pub fn separate(cpu_hour: Option<Decimal>, mem_gib_hour: Option<Decimal>) -> Option<Rates> {
        if cpu_hour.is_none() && mem_gib_hour.is_none() {
            return None;
        }
        Some(Rates {
            cpu_hour: cpu_hour.unwrap_or_default(),
            mem_gib_hour: mem_gib_hour.unwrap_or_default(),
        })
    }

    /// Rates derived from one node price: `cpu_share` of the price is spread
    /// over the node's vCPUs, the rest over its GiB of memory.
    pub fn from_node(
        node_hour: Decimal,
        node_vcpu: Decimal,
        node_gib: Decimal,
        cpu_share: Decimal,
    ) -> Result<Rates, Error> {
        if node_vcpu <= Decimal::ZERO {
            return Err(Error::msg("--node-vcpu must be greater than zero"));
        }
        if node_gib <= Decimal::ZERO {
            return Err(Error::msg("--node-gib must be greater than zero"));
        }
        if cpu_share < Decimal::ZERO || cpu_share > Decimal::ONE {
            return Err(Error::msg("--cpu-share must be between 0 and 1"));
        }
        let overflow = || Error::msg("node rate is too large");
        let cpu_part = node_hour.checked_mul(cpu_share).ok_or_else(overflow)?;
        let mem_part = node_hour
            .checked_mul(Decimal::ONE - cpu_share)
            .ok_or_else(overflow)?;
        Ok(Rates {
            cpu_hour: cpu_part.checked_div(node_vcpu).ok_or_else(overflow)?,
            mem_gib_hour: mem_part.checked_div(node_gib).ok_or_else(overflow)?,
        })
    }

    /// Cost of `nanocores` and `bytes` held for `hours` hours.
    pub fn cost(&self, nanocores: u128, bytes: u128, hours: Decimal) -> Result<Decimal, Error> {
        let overflow = || Error::msg("cost is too large to represent");
        let cpu = nano_to_vcpu(nanocores)?
            .checked_mul(self.cpu_hour)
            .ok_or_else(overflow)?;
        let mem = bytes_to_gib(bytes)?
            .checked_mul(self.mem_gib_hour)
            .ok_or_else(overflow)?;
        cpu.checked_add(mem)
            .and_then(|sum| sum.checked_mul(hours))
            .ok_or_else(overflow)
    }
}

/// clap value parser: a non-negative decimal such as `0.031`.
pub fn parse_price(text: &str) -> Result<Decimal, String> {
    let value = Decimal::from_str(text.trim())
        .map_err(|_| format!("{text:?} is not a decimal number such as 0.031"))?;
    if value.is_sign_negative() && !value.is_zero() {
        return Err(format!("{text:?} must not be negative"));
    }
    Ok(value)
}

/// clap value parser: a decimal greater than zero.
pub fn parse_positive(text: &str) -> Result<Decimal, String> {
    let value = parse_price(text)?;
    if value.is_zero() {
        return Err(format!("{text:?} must be greater than zero"));
    }
    Ok(value)
}

/// clap value parser: a fraction between 0 and 1.
pub fn parse_fraction(text: &str) -> Result<Decimal, String> {
    let value = parse_price(text)?;
    if value > Decimal::ONE {
        return Err(format!("{text:?} must be between 0 and 1"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }

    #[test]
    fn separate_none_when_both_missing() {
        assert_eq!(Rates::separate(None, None), None);
    }

    #[test]
    fn separate_omitted_rate_is_zero() {
        let r = Rates::separate(Some(d("0.03")), None).unwrap();
        assert_eq!(r.cpu_hour, d("0.03"));
        assert_eq!(r.mem_gib_hour, Decimal::ZERO);
        let r = Rates::separate(None, Some(d("0.004"))).unwrap();
        assert_eq!(r.cpu_hour, Decimal::ZERO);
        assert_eq!(r.mem_gib_hour, d("0.004"));
    }

    #[test]
    fn node_rate_splits_half_and_half_by_default_share() {
        // 0.192 per hour, 4 vCPU, 16 GiB, 50/50 → 0.096/4 and 0.096/16
        let r = Rates::from_node(d("0.192"), d("4"), d("16"), d("0.5")).unwrap();
        assert_eq!(r.cpu_hour, d("0.024"));
        assert_eq!(r.mem_gib_hour, d("0.006"));
    }

    #[test]
    fn node_rate_cpu_share_extremes() {
        let all_cpu = Rates::from_node(d("1"), d("2"), d("8"), d("1")).unwrap();
        assert_eq!(all_cpu.cpu_hour, d("0.5"));
        assert_eq!(all_cpu.mem_gib_hour, Decimal::ZERO);
        let all_mem = Rates::from_node(d("1"), d("2"), d("8"), d("0")).unwrap();
        assert_eq!(all_mem.cpu_hour, Decimal::ZERO);
        assert_eq!(all_mem.mem_gib_hour, d("0.125"));
    }

    #[test]
    fn node_rate_reconstructs_the_node_price() {
        let r = Rates::from_node(d("0.384"), d("8"), d("32"), d("0.6")).unwrap();
        let node = r
            .cost(8_000_000_000, 32 * (1u128 << 30), Decimal::ONE)
            .unwrap();
        assert_eq!(node, d("0.384"));
    }

    #[test]
    fn node_rate_rejects_bad_shapes() {
        assert!(Rates::from_node(d("1"), d("0"), d("8"), d("0.5")).is_err());
        assert!(Rates::from_node(d("1"), d("2"), d("0"), d("0.5")).is_err());
        assert!(Rates::from_node(d("1"), d("2"), d("8"), d("1.5")).is_err());
    }

    #[test]
    fn cost_multiplies_by_hours() {
        let r = Rates {
            cpu_hour: d("0.04"),
            mem_gib_hour: d("0.005"),
        };
        // 2 vCPU + 4 GiB for 730 h = (0.08 + 0.02) * 730
        let c = r.cost(2_000_000_000, 4 * (1u128 << 30), d("730")).unwrap();
        assert_eq!(c, d("73"));
    }

    #[test]
    fn cost_zero_usage_is_zero() {
        let r = Rates {
            cpu_hour: d("1"),
            mem_gib_hour: d("1"),
        };
        assert_eq!(r.cost(0, 0, d("5")).unwrap(), Decimal::ZERO);
    }

    #[test]
    fn cost_overflow_is_an_error() {
        let r = Rates {
            cpu_hour: Decimal::MAX,
            mem_gib_hour: Decimal::ZERO,
        };
        assert!(r.cost(2_000_000_000, 0, Decimal::ONE).is_err());
    }

    #[test]
    fn price_parser() {
        assert_eq!(parse_price("0.031").unwrap(), d("0.031"));
        assert_eq!(parse_price(" 2 ").unwrap(), d("2"));
        assert!(parse_price("-1").is_err());
        assert!(parse_price("cheap").is_err());
        assert!(parse_price("").is_err());
    }

    #[test]
    fn positive_and_fraction_parsers() {
        assert!(parse_positive("0").is_err());
        assert!(parse_positive("4").is_ok());
        assert!(parse_fraction("0.5").is_ok());
        assert!(parse_fraction("1").is_ok());
        assert!(parse_fraction("1.01").is_err());
    }
}
