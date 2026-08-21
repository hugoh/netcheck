use serde::Serialize;
use std::net::IpAddr;
use std::time::Duration;

/// Result of a single ping probe against a target host.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PingResult {
    pub target: String,
    pub reachable: bool,
    pub rtt_ms: Option<f64>,
}

const PING_PAYLOAD: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
const PING_TIMEOUT: Duration = Duration::from_secs(1);

/// Sends one ICMP echo request via a native, unprivileged ICMP/DGRAM socket
/// (no `ping`/`ping6` subprocess), bounded to `PING_TIMEOUT`. `surge_ping`
/// picks ICMPv4 or ICMPv6 automatically based on `addr`'s family. Spins up a
/// throwaway single-threaded tokio runtime per call so this stays a plain
/// synchronous function — `ping_all`'s thread::scope-based concurrency is
/// unchanged.
fn ping_via_icmp(addr: IpAddr) -> Option<Duration> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        tokio::time::timeout(PING_TIMEOUT, surge_ping::ping(addr, &PING_PAYLOAD))
            .await
            .ok()?
            .ok()
            .map(|(_packet, duration)| duration)
    })
}

/// Pings `target` once with a 1 second timeout. `target` must be a literal
/// IPv4 or IPv6 address; an unparsable target is reported as unreachable.
pub fn ping(target: &str) -> PingResult {
    let rtt = target.parse::<IpAddr>().ok().and_then(ping_via_icmp);

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
    fn unparsable_target_is_reported_unreachable() {
        let result = ping("not-an-ip-address");
        assert_eq!(
            result,
            PingResult {
                target: "not-an-ip-address".to_string(),
                reachable: false,
                rtt_ms: None,
            }
        );
    }
}
