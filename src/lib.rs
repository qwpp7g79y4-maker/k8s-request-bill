//! `k8s-request-bill`: offline overprovisioning of Kubernetes requests vs usage.
//!
//! Reads saved `kubectl` JSON (pods, and optionally a metrics-server
//! `PodMetricsList`) and reports requests, limits, usage and overprovisioning
//! per namespace in vCPU and GiB, plus a cost at a node rate you supply.
//! Nothing here talks to a cluster.

use std::fmt;

pub mod cli;
pub mod model;
pub mod quantity;
pub mod rates;
pub mod report;

/// Error type for the whole crate: a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    /// Build an error from any message.
    pub fn msg(message: impl Into<String>) -> Self {
        Error(message.into())
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_displays_its_message() {
        let e = Error::msg("boom");
        assert_eq!(e.to_string(), "boom");
        assert_eq!(e.message(), "boom");
    }

    #[test]
    fn error_from_string_and_str() {
        assert_eq!(Error::msg(String::from("a")), Error::msg("a"));
    }
}
