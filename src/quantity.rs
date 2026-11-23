//! Kubernetes Quantity parsing (cpu and memory).
//!
//! Format: <https://kubernetes.io/docs/reference/kubernetes-api/common-definitions/quantity/>
//! (geraadpleegd 2026-11-09).
//!
//! * CPU is returned in nanocores (`u64`). Suffixes `n` (nano), `u` (micro) and
//!   `m` (milli) are understood, as are plain cores (`2`, `0.5`).
//! * Memory is returned in bytes (`u64`). Binary suffixes (`Ki` … `Ei`), decimal
//!   suffixes (`k`, `M` … `E`), the milli suffix `m` and exponent notation
//!   (`129e6`) are understood.
//!
//! Negative values are rejected: a request, limit or usage can not be negative.

use rust_decimal::{Decimal, RoundingStrategy};
use std::str::FromStr;

use crate::Error;

/// Nanocores in one vCPU.
pub const NANO_PER_CORE: u64 = 1_000_000_000;
/// Bytes in one GiB.
pub const GIB: u64 = 1024 * 1024 * 1024;

/// Parse a decimal number, with optional exponent. Only digits, `.`, `+`, `-`,
/// `e` and `E` are accepted, so `1_000` and ` 1` are rejected.
fn parse_number(number: &str) -> Option<Decimal> {
    if number.is_empty()
        || !number
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E'))
    {
        return None;
    }
    Decimal::from_str(number)
        .ok()
        .or_else(|| Decimal::from_scientific(number).ok())
}

/// Scale a parsed number by `factor`, round to a whole number and convert to `u64`.
fn scale(number: &str, factor: Decimal, kind: &str, text: &str) -> Result<u64, Error> {
    let value = parse_number(number).ok_or_else(|| {
        Error::msg(format!(
            "invalid {kind} quantity {text:?}: expected a number with an optional suffix"
        ))
    })?;
    if value.is_sign_negative() && !value.is_zero() {
        return Err(Error::msg(format!(
            "invalid {kind} quantity {text:?}: {kind} must not be negative"
        )));
    }
    let too_large = || {
        Error::msg(format!(
            "invalid {kind} quantity {text:?}: value is too large"
        ))
    };
    let scaled = value.checked_mul(factor).ok_or_else(too_large)?;
    let rounded = scaled.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
    u64::try_from(rounded).map_err(|_| too_large())
}

/// Parse a CPU Quantity into nanocores.
pub fn parse_cpu(text: &str) -> Result<u64, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::msg(
            "cpu quantity is empty; use a value such as 100m or 1",
        ));
    }
    let (number, factor) = if let Some(n) = text.strip_suffix('n') {
        (n, Decimal::from(1u64))
    } else if let Some(n) = text.strip_suffix('u') {
        (n, Decimal::from(1_000u64))
    } else if let Some(n) = text.strip_suffix('m') {
        (n, Decimal::from(1_000_000u64))
    } else {
        (text, Decimal::from(NANO_PER_CORE))
    };
    scale(number, factor, "cpu", text)
}

/// Parse a memory Quantity into bytes.
pub fn parse_memory(text: &str) -> Result<u64, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::msg(
            "memory quantity is empty; use a value such as 128Mi or 1Gi",
        ));
    }
    let (number, factor) = split_memory(text);
    scale(number, factor, "memory", text)
}

fn split_memory(text: &str) -> (&str, Decimal) {
    const SUFFIXES: &[(&str, u64)] = &[
        ("Ki", 1 << 10),
        ("Mi", 1 << 20),
        ("Gi", 1 << 30),
        ("Ti", 1 << 40),
        ("Pi", 1 << 50),
        ("Ei", 1 << 60),
        ("k", 1_000),
        ("M", 1_000_000),
        ("G", 1_000_000_000),
        ("T", 1_000_000_000_000),
        ("P", 1_000_000_000_000_000),
        ("E", 1_000_000_000_000_000_000),
    ];
    for (suffix, factor) in SUFFIXES {
        if let Some(number) = text.strip_suffix(suffix) {
            return (number, Decimal::from(*factor));
        }
    }
    if let Some(number) = text.strip_suffix('m') {
        // milli-bytes; legal in the grammar, rare in practice
        return (number, Decimal::new(1, 3));
    }
    (text, Decimal::from(1u64))
}

/// Exact conversion of a count to `Decimal`.
pub fn to_decimal(value: u128) -> Result<Decimal, Error> {
    i128::try_from(value)
        .ok()
        .and_then(|v| Decimal::try_from_i128_with_scale(v, 0).ok())
        .ok_or_else(|| Error::msg("total is too large to represent"))
}

/// Nanocores → vCPU as a decimal (exact).
pub fn nano_to_vcpu(nanocores: u128) -> Result<Decimal, Error> {
    i128::try_from(nanocores)
        .ok()
        .and_then(|v| Decimal::try_from_i128_with_scale(v, 9).ok())
        .ok_or_else(|| Error::msg("total is too large to represent"))
}

/// Bytes → GiB as a decimal (rounded at the 28th decimal by `Decimal`).
pub fn bytes_to_gib(bytes: u128) -> Result<Decimal, Error> {
    to_decimal(bytes)?
        .checked_div(Decimal::from(GIB))
        .ok_or_else(|| Error::msg("total is too large to represent"))
}

/// Round half away from zero to `places` decimals and print all of them
/// (`Decimal`'s own `{:.N}` truncates).
pub fn fixed(value: Decimal, places: u32) -> String {
    let rounded = value.round_dp_with_strategy(places, RoundingStrategy::MidpointAwayFromZero);
    format!("{rounded:.prec$}", prec = places as usize)
}

/// Format nanocores as vCPU with three decimals.
pub fn format_vcpu(nanocores: u128) -> String {
    match nano_to_vcpu(nanocores) {
        Ok(v) => fixed(v, 3),
        Err(_) => "overflow".to_string(),
    }
}

/// Format bytes as GiB with three decimals.
pub fn format_gib(bytes: u128) -> String {
    match bytes_to_gib(bytes) {
        Ok(v) => fixed(v, 3),
        Err(_) => "overflow".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_millicores() {
        assert_eq!(parse_cpu("100m").unwrap(), 100_000_000);
        assert_eq!(parse_cpu("1").unwrap(), NANO_PER_CORE);
        assert_eq!(parse_cpu("0.5").unwrap(), 500_000_000);
        assert_eq!(parse_cpu(" 250m ").unwrap(), 250_000_000);
    }

    #[test]
    fn cpu_nano_and_micro() {
        assert_eq!(parse_cpu("12345678n").unwrap(), 12_345_678);
        assert_eq!(parse_cpu("250u").unwrap(), 250_000);
        assert_eq!(parse_cpu("0n").unwrap(), 0);
    }

    #[test]
    fn cpu_fractional_millicores_round() {
        assert_eq!(parse_cpu("0.5m").unwrap(), 500_000);
        assert_eq!(parse_cpu("1.5n").unwrap(), 2); // midpoint away from zero
    }

    #[test]
    fn cpu_exponent() {
        assert_eq!(parse_cpu("1e3m").unwrap(), NANO_PER_CORE);
        assert_eq!(parse_cpu("2E0").unwrap(), 2 * NANO_PER_CORE);
    }

    #[test]
    fn cpu_large_cores() {
        assert_eq!(parse_cpu("16").unwrap(), 16 * NANO_PER_CORE);
    }

    #[test]
    fn cpu_zero_is_fine_even_when_signed() {
        assert_eq!(parse_cpu("0").unwrap(), 0);
        assert_eq!(parse_cpu("-0").unwrap(), 0);
    }

    #[test]
    fn cpu_rejects_empty_negative_and_garbage() {
        assert!(parse_cpu("").is_err());
        assert!(parse_cpu("   ").is_err());
        assert!(parse_cpu("-1").is_err());
        assert!(parse_cpu("-5m").is_err());
        assert!(parse_cpu("nope").is_err());
        assert!(parse_cpu("m").is_err());
        assert!(parse_cpu("1 m").is_err());
        assert!(parse_cpu("1_0").is_err());
        assert!(parse_cpu("1.2.3").is_err());
    }

    #[test]
    fn cpu_rejects_overflow() {
        assert!(parse_cpu("99999999999999999999").is_err());
        assert!(parse_cpu("1e40").is_err());
    }

    #[test]
    fn cpu_error_names_the_value() {
        let err = parse_cpu("abc").unwrap_err().to_string();
        assert!(err.contains("abc"), "{err}");
        assert!(err.contains("cpu"), "{err}");
    }

    #[test]
    fn memory_binary_and_decimal() {
        assert_eq!(parse_memory("1Ki").unwrap(), 1024);
        assert_eq!(parse_memory("1Mi").unwrap(), 1024 * 1024);
        assert_eq!(parse_memory("1Gi").unwrap(), GIB);
        assert_eq!(parse_memory("128Mi").unwrap(), 128 * 1024 * 1024);
        assert_eq!(parse_memory("1000").unwrap(), 1000);
        assert_eq!(parse_memory("1k").unwrap(), 1000);
        assert_eq!(parse_memory("1M").unwrap(), 1_000_000);
        assert_eq!(parse_memory("2G").unwrap(), 2_000_000_000);
    }

    #[test]
    fn memory_large_suffixes() {
        assert_eq!(parse_memory("1Ti").unwrap(), 1024u64.pow(4));
        assert_eq!(parse_memory("1Pi").unwrap(), 1024u64.pow(5));
        assert_eq!(parse_memory("1Ei").unwrap(), 1024u64.pow(6));
        assert_eq!(parse_memory("1T").unwrap(), 1_000_000_000_000);
        assert_eq!(parse_memory("1E").unwrap(), 1_000_000_000_000_000_000);
    }

    #[test]
    fn memory_decimals_and_exponent() {
        assert_eq!(parse_memory("1.5Gi").unwrap(), GIB + GIB / 2);
        assert_eq!(parse_memory("129e6").unwrap(), 129_000_000);
        assert_eq!(parse_memory("1E3").unwrap(), 1000);
    }

    #[test]
    fn memory_milli_bytes() {
        assert_eq!(parse_memory("1500m").unwrap(), 2); // 1.5 bytes rounds up
        assert_eq!(parse_memory("2000m").unwrap(), 2);
    }

    #[test]
    fn memory_rejects_bad() {
        assert!(parse_memory("").is_err());
        assert!(parse_memory("-1Mi").is_err());
        assert!(parse_memory("Mi").is_err());
        assert!(parse_memory("1Xi").is_err());
        assert!(parse_memory("1 Mi").is_err());
    }

    #[test]
    fn memory_rejects_overflow() {
        assert!(parse_memory("1e30Ei").is_err());
        assert!(parse_memory("100Ei").is_err());
    }

    #[test]
    fn format_helpers() {
        assert_eq!(format_vcpu(1_000_000_000), "1.000");
        assert_eq!(format_vcpu(500_000_000), "0.500");
        assert_eq!(format_vcpu(1), "0.000");
        assert_eq!(format_gib(u128::from(GIB)), "1.000");
        assert_eq!(format_gib(u128::from(GIB) / 2), "0.500");
    }

    #[test]
    fn formatting_rounds_instead_of_truncating() {
        assert_eq!(format_gib(100 * 1024 * 1024), "0.098"); // 0.09765625
        assert_eq!(format_vcpu(1_999_500_000), "2.000");
        assert_eq!(format_vcpu(1_234_500_000), "1.235"); // midpoint away from zero
        assert_eq!(fixed(Decimal::from_str("0.07756").unwrap(), 4), "0.0776");
        assert_eq!(fixed(Decimal::from(2), 4), "2.0000");
    }

    #[test]
    fn conversions_are_exact_for_cpu() {
        assert_eq!(
            nano_to_vcpu(1_250_000_000).unwrap(),
            Decimal::from_str("1.25").unwrap()
        );
        assert_eq!(bytes_to_gib(u128::from(GIB) * 3).unwrap(), Decimal::from(3));
    }

    #[test]
    fn conversions_reject_huge_totals() {
        assert!(nano_to_vcpu(u128::MAX).is_err());
        assert!(to_decimal(u128::MAX).is_err());
        assert_eq!(format_gib(u128::MAX), "overflow");
        assert_eq!(format_vcpu(u128::MAX), "overflow");
    }
}
