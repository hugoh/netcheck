//! Shared trigger machinery for both the TUI's auto-refresh and the `watch`
//! CLI subcommand: a cheap event-driven recheck on OS config changes,
//! backed by an adaptive-interval poll as a safety net for failures that
//! produce no config-change event at all (e.g. a blackholed route).

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

/// How often the poll thread re-checks `enabled` while idle (matches the
/// TUI's pre-existing idle-poll cadence).
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub const HEALTHY: u8 = 0;
pub const DEGRADED: u8 = 1;

/// The poll thread's sleep duration in each health state.
#[derive(Debug, Clone, Copy)]
pub struct Intervals {
    /// No config change produces a blackholed route in isolation, but a
    /// check still needs to run occasionally to catch it.
    pub healthy: Duration,
    /// Used once a check has come back degraded, so recovery (or a
    /// worsening outage) is still noticed quickly without probing
    /// continuously.
    pub degraded: Duration,
}

impl Default for Intervals {
    fn default() -> Self {
        Intervals {
            healthy: Duration::from_secs(60),
            degraded: Duration::from_secs(10),
        }
    }
}

/// How much a triggered check should actually probe. A config change means
/// something in interfaces/VPN/proxy/Wi-Fi/DNS genuinely may have changed,
/// so it warrants a full recheck; a poll tick exists only to catch failures
/// that produce no config event at all (a blackholed route), so it only
/// needs the confidence-determining probes — re-running everything else on
/// every poll would be pure waste (notably `wifi_identity`'s
/// `system_profiler` shell-out, which nothing in a poll tick could change).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckScope {
    Full,
    ConfidenceOnly,
}

/// Keeps the config-change watcher and the adaptive-poll thread alive for
/// as long as this is held — drop only at process exit.
pub struct WatchTrigger {
    _sc_handle: netstatus::WatchHandle,
    _poll_thread: std::thread::JoinHandle<()>,
}

fn interval_for(intervals: Intervals, health: u8) -> Duration {
    if health == DEGRADED {
        intervals.degraded
    } else {
        intervals.healthy
    }
}

/// Spawns the config-change watcher and the adaptive-poll thread, both
/// gated on `enabled`: while `false`, neither one calls `fire`, matching the
/// existing "only manual refresh" behavior when auto-refresh is off. The
/// config-change watcher always fires with `CheckScope::Full`; the poll
/// thread fires `CheckScope::Full` the first time it runs after becoming
/// enabled (there's no config-change data to fall back on yet) and
/// `CheckScope::ConfidenceOnly` after that (see `CheckScope`).
///
/// `health` is written by the caller after each check completes (`HEALTHY`
/// or `DEGRADED`, from the latest `ConnectionConfidence`) and read here to
/// pick the poll thread's next sleep duration from `intervals`.
pub fn spawn_watch_trigger(
    enabled: Arc<AtomicBool>,
    health: Arc<AtomicU8>,
    intervals: Intervals,
    fire: impl Fn(CheckScope) + Send + Sync + 'static,
) -> io::Result<WatchTrigger> {
    let fire = Arc::new(fire);

    let sc_handle = {
        let enabled = enabled.clone();
        let fire = fire.clone();
        netstatus::watch_config_changes(move || {
            if enabled.load(Ordering::Relaxed) {
                fire(CheckScope::Full);
            }
        })?
    };

    let poll_thread = std::thread::spawn(move || {
        let mut first_fire = true;
        loop {
            if enabled.load(Ordering::Relaxed) {
                let scope = if first_fire {
                    CheckScope::Full
                } else {
                    CheckScope::ConfidenceOnly
                };
                first_fire = false;
                fire(scope);
                std::thread::sleep(interval_for(intervals, health.load(Ordering::Relaxed)));
            } else {
                std::thread::sleep(IDLE_POLL_INTERVAL);
            }
        }
    });

    Ok(WatchTrigger {
        _sc_handle: sc_handle,
        _poll_thread: poll_thread,
    })
}
