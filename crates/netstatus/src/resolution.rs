use crate::concurrent::{for_each_concurrent, map_all};
use serde::Serialize;
use std::io;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Well-known domains used to sanity-check that DNS resolution actually works.
pub const DEFAULT_RESOLUTION_TARGETS: &[&str] = &[
    "amazon.com",
    "apple.com",
    "cloudflare.com",
    "github.com",
    "google.com",
    "microsoft.com",
    "wikipedia.org",
];

/// Result of resolving a single domain name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolutionResult {
    pub domain: String,
    pub resolved: bool,
    pub addresses: Vec<String>,
    pub duration_ms: Option<f64>,
}

fn build_result(
    domain: &str,
    lookup: io::Result<Vec<IpAddr>>,
    elapsed: Duration,
) -> ResolutionResult {
    match lookup {
        Ok(addrs) if !addrs.is_empty() => ResolutionResult {
            domain: domain.to_string(),
            resolved: true,
            addresses: addrs.iter().map(ToString::to_string).collect(),
            duration_ms: Some(elapsed.as_secs_f64() * 1000.0),
        },
        _ => ResolutionResult {
            domain: domain.to_string(),
            resolved: false,
            addresses: Vec::new(),
            duration_ms: None,
        },
    }
}

/// Resolves a single domain name, timing how long the lookup took.
pub fn resolve(domain: &str) -> ResolutionResult {
    let start = Instant::now();
    let lookup = (domain, 0u16)
        .to_socket_addrs()
        .map(|addrs| addrs.map(|a| a.ip()).collect::<Vec<_>>());
    build_result(domain, lookup, start.elapsed())
}

/// Resolves each domain concurrently and returns results in the same order.
pub fn resolve_all(domains: &[&str]) -> Vec<ResolutionResult> {
    map_all(domains, resolve)
}

/// Resolves each domain concurrently, calling `on_result` as each one
/// completes rather than waiting for the slowest domain before any result
/// is visible.
pub fn resolve_each(domains: &[&str], on_result: impl Fn(ResolutionResult) + Sync) {
    for_each_concurrent(domains, resolve, on_result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn build_result_reports_success_with_addresses_and_duration() {
        let addrs = vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))];
        let result = build_result("example.com", Ok(addrs), Duration::from_millis(12));

        assert_eq!(
            result,
            ResolutionResult {
                domain: "example.com".to_string(),
                resolved: true,
                addresses: vec!["93.184.216.34".to_string()],
                duration_ms: Some(12.0),
            }
        );
    }

    #[test]
    fn build_result_reports_failure_on_lookup_error() {
        let err = io::Error::other("nodename nor servname provided");
        let result = build_result("nonexistent.invalid", Err(err), Duration::from_millis(50));

        assert_eq!(
            result,
            ResolutionResult {
                domain: "nonexistent.invalid".to_string(),
                resolved: false,
                addresses: Vec::new(),
                duration_ms: None,
            }
        );
    }

    #[test]
    fn build_result_reports_failure_when_no_addresses_returned() {
        let result = build_result("empty.invalid", Ok(Vec::new()), Duration::from_millis(5));
        assert!(!result.resolved);
        assert!(result.addresses.is_empty());
    }
}
