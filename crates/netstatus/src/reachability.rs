use serde::Serialize;
use std::process::Command;
use std::time::Duration;

/// Result of a single ping probe against a target host.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PingResult {
    pub target: String,
    pub reachable: bool,
    pub rtt_ms: Option<f64>,
}

pub(crate) fn parse_ping_output(text: &str) -> Option<Duration> {
    let time_str = text
        .lines()
        .find_map(|line| line.split_whitespace().find(|w| w.starts_with("time=")))?
        .strip_prefix("time=")?;
    let ms: f64 = time_str.parse().ok()?;
    Some(Duration::from_secs_f64(ms / 1000.0))
}

/// Returns `true` if `target` is an IPv6 literal (contains a `:`), `false`
/// for IPv4.
pub(crate) fn is_ipv6(target: &str) -> bool {
    target.contains(':')
}

/// Pings `target` once with a 1 second timeout.
///
/// IPv6 targets are shelled to `/sbin/ping6` rather than `/sbin/ping`, which
/// cannot resolve IPv6 literals and rejects `-6`. `ping6` has no `-t
/// <timeout>` equivalent (its `-t` is an unrelated bare flag), so v6 probes
/// run with `-c 1` alone and no explicit timeout bound.
pub fn ping(target: &str) -> PingResult {
    let output = if is_ipv6(target) {
        Command::new("/sbin/ping6").args(["-c", "1", target]).output()
    } else {
        Command::new("/sbin/ping")
            .args(["-c", "1", "-t", "1", target])
            .output()
    };

    let rtt = output
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_ping_output(&String::from_utf8_lossy(&o.stdout)));

    PingResult {
        target: target.to_string(),
        reachable: rtt.is_some(),
        rtt_ms: rtt.map(|d| d.as_secs_f64() * 1000.0),
    }
}

/// Pings each target concurrently and returns results in the same order.
pub fn ping_all(targets: &[&str]) -> Vec<PingResult> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|target| scope.spawn(move || ping(target)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_round_trip_time_from_successful_ping() {
        let output = "PING 1.1.1.1 (1.1.1.1): 56 data bytes\n\
                       64 bytes from 1.1.1.1: icmp_seq=0 ttl=47 time=28.211 ms\n\
                       \n\
                       --- 1.1.1.1 ping statistics ---\n\
                       1 packets transmitted, 1 packets received, 0.0% packet loss\n";
        let rtt = parse_ping_output(output).expect("should parse rtt");
        assert!((rtt.as_secs_f64() * 1000.0 - 28.211).abs() < 1e-6);
    }

    #[test]
    fn returns_none_when_no_reply_line_present() {
        let output = "PING 10.255.255.1 (10.255.255.1): 56 data bytes\n\
                       \n\
                       --- 10.255.255.1 ping statistics ---\n\
                       1 packets transmitted, 0 packets received, 100.0% packet loss\n";
        assert_eq!(parse_ping_output(output), None);
    }

    #[test]
    fn detects_ipv4_targets() {
        assert!(!is_ipv6("1.1.1.1"));
        assert!(!is_ipv6("8.8.4.4"));
        assert!(!is_ipv6("208.67.222.222"));
    }

    #[test]
    fn detects_ipv6_targets() {
        assert!(is_ipv6("2606:4700:4700::1111"));
        assert!(is_ipv6("2001:4860:4860::8888"));
        assert!(is_ipv6("::1"));
    }
}
