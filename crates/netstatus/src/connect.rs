use serde::Serialize;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// Result of a single TCP connect probe against a target host and port.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConnectResult {
    pub target: String,
    pub port: u16,
    pub reachable: bool,
    pub rtt_ms: Option<f64>,
}

fn build_connect_result(
    target: &str,
    port: u16,
    success: bool,
    elapsed: Duration,
) -> ConnectResult {
    ConnectResult {
        target: target.to_string(),
        port,
        reachable: success,
        rtt_ms: success.then_some(elapsed.as_secs_f64() * 1000.0),
    }
}

/// Attempts a TCP connection to `target:port`, timing how long it took.
/// Reachability here means "a TCP handshake completed" — unlike ICMP ping,
/// this isn't affected by hosts that filter or rate-limit ICMP.
pub fn connect(target: &str, port: u16) -> ConnectResult {
    let start = Instant::now();
    let success = (target, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .is_some_and(|addr| TcpStream::connect_timeout(&addr, DEFAULT_TIMEOUT).is_ok());
    build_connect_result(target, port, success, start.elapsed())
}

/// Connects to each target concurrently and returns results in the same order.
pub fn connect_all(targets: &[&str], port: u16) -> Vec<ConnectResult> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|target| scope.spawn(move || connect(target, port)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_connect_result_reports_success_with_rtt() {
        let result = build_connect_result("example.com", 443, true, Duration::from_millis(20));
        assert_eq!(
            result,
            ConnectResult {
                target: "example.com".to_string(),
                port: 443,
                reachable: true,
                rtt_ms: Some(20.0),
            }
        );
    }

    #[test]
    fn build_connect_result_reports_failure_without_rtt() {
        let result = build_connect_result(
            "unreachable.invalid",
            443,
            false,
            Duration::from_millis(2000),
        );
        assert_eq!(
            result,
            ConnectResult {
                target: "unreachable.invalid".to_string(),
                port: 443,
                reachable: false,
                rtt_ms: None,
            }
        );
    }
}
