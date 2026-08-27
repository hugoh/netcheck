//! Shared trigger machinery for both the TUI's auto-refresh and the `watch`
//! CLI subcommand: a cheap event-driven recheck on OS config changes,
//! backed by an adaptive-interval poll as a safety net for failures that
//! produce no config-change event at all (e.g. a blackholed route).

use netstatus::StatusField;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
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

/// Runs one collection pass — full or confidence-only per `scope` — calling
/// `on_field` for every field as it arrives (e.g. to fold it into a running
/// `PartialStatus`) and then forwarding it to `tx`, but only as long as
/// `generation` still matches `this_gen` — if a newer run (manual or auto)
/// has started in the meantime, this run's remaining fields are dropped
/// from `tx` instead of overwriting fresher data. `on_field` always sees
/// every field regardless of generation, since a stale run's data is still
/// the most recent thing known until a newer run's fields replace it.
///
/// Shared by the CLI `watch` subcommand and the TUI's auto-refresh, which
/// both run this same fire/generation-guard dance but differ in what they
/// do with each field (the CLI folds it into a `PartialStatus` to derive
/// `health`; the TUI merges it into the screen's own `PartialStatus` in its
/// render loop instead).
pub fn run_and_forward(
    tx: &mpsc::Sender<StatusField>,
    generation: &Arc<AtomicU64>,
    this_gen: u64,
    scope: CheckScope,
    mut on_field: impl FnMut(&StatusField),
) {
    let (inner_tx, inner_rx) = mpsc::channel();
    std::thread::spawn(move || match scope {
        CheckScope::Full => netstatus::collect_streaming(inner_tx),
        CheckScope::ConfidenceOnly => netstatus::collect_confidence_streaming(inner_tx),
    });
    for field in inner_rx {
        on_field(&field);
        if generation.load(Ordering::SeqCst) == this_gen {
            let _ = tx.send(field);
        }
    }
}

/// Keeps the config-change watcher (if it started) and the adaptive-poll
/// thread alive for as long as this is held — drop only at process exit.
pub struct WatchTrigger {
    _sc_handle: Option<netstatus::WatchHandle>,
    _poll_thread: std::thread::JoinHandle<()>,
}

fn interval_for(intervals: Intervals, health: u8) -> Duration {
    if health == DEGRADED {
        intervals.degraded
    } else {
        intervals.healthy
    }
}

/// What the poll thread should fire (if anything) on one loop iteration,
/// given whether it was enabled on the previous iteration and is enabled on
/// this one. `None` means don't fire (and idle-sleep instead). Pulled out of
/// the poll loop so the false -> true transition (which must always produce
/// `Full`, not just the thread's very first iteration) is unit-testable
/// without spinning up real threads/timers.
fn scope_for_poll_tick(was_enabled: bool, is_enabled: bool) -> Option<CheckScope> {
    if !is_enabled {
        return None;
    }
    Some(if was_enabled {
        CheckScope::ConfidenceOnly
    } else {
        CheckScope::Full
    })
}

/// Spawns the config-change watcher and the adaptive-poll thread, both
/// gated on `enabled`: while `false`, neither one calls `fire`, matching the
/// existing "only manual refresh" behavior when auto-refresh is off. The
/// config-change watcher always fires with `CheckScope::Full`; the poll
/// thread fires `CheckScope::Full` the first time it runs after becoming
/// enabled (there's no config-change data to fall back on yet) and
/// `CheckScope::ConfidenceOnly` after that (see `CheckScope`).
///
/// If the config-change watcher fails to start (e.g. `SCDynamicStore` setup
/// rejected in a sandboxed environment), this doesn't fail outright — it
/// still returns a `WatchTrigger` running the poll thread alone, so `watch`
/// degrades to poll-only instead of refusing to run at all.
///
/// `health` is written by the caller after each check completes (`HEALTHY`
/// or `DEGRADED`, from the latest `ConnectionConfidence`) and read here to
/// pick the poll thread's next sleep duration from `intervals`.
pub fn spawn_watch_trigger(
    enabled: Arc<AtomicBool>,
    health: Arc<AtomicU8>,
    intervals: Intervals,
    fire: impl Fn(CheckScope) + Send + Sync + 'static,
) -> WatchTrigger {
    let fire = Arc::new(fire);

    let sc_handle = {
        let enabled = enabled.clone();
        let fire = fire.clone();
        netstatus::watch_config_changes(move || {
            if enabled.load(Ordering::Relaxed) {
                fire(CheckScope::Full);
            }
        })
        .inspect_err(|err| {
            eprintln!("netcheck: config-change watcher unavailable, falling back to poll-only: {err}");
        })
        .ok()
    };

    let poll_thread = std::thread::spawn(move || {
        // Reset whenever `enabled` transitions false -> true, not just once
        // for the thread's whole lifetime — a user toggling auto-refresh
        // off and back on needs a fresh `Full` fire, since a real network
        // change during the "off" window (dropped by both this thread and
        // the config-change watcher, which are gated on `enabled` too)
        // would otherwise go unnoticed until the next poll tick after that.
        let mut was_enabled = false;
        loop {
            let is_enabled = enabled.load(Ordering::Relaxed);
            match scope_for_poll_tick(was_enabled, is_enabled) {
                Some(scope) => {
                    fire(scope);
                    std::thread::sleep(interval_for(intervals, health.load(Ordering::Relaxed)));
                }
                None => std::thread::sleep(IDLE_POLL_INTERVAL),
            }
            was_enabled = is_enabled;
        }
    });

    WatchTrigger {
        _sc_handle: sc_handle,
        _poll_thread: poll_thread,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_iteration_fires_full() {
        assert_eq!(scope_for_poll_tick(false, true), Some(CheckScope::Full));
    }

    #[test]
    fn subsequent_enabled_iterations_fire_confidence_only() {
        assert_eq!(
            scope_for_poll_tick(true, true),
            Some(CheckScope::ConfidenceOnly)
        );
    }

    #[test]
    fn disabled_iterations_dont_fire() {
        assert_eq!(scope_for_poll_tick(false, false), None);
        assert_eq!(scope_for_poll_tick(true, false), None);
    }

    #[test]
    fn re_enabling_after_being_disabled_fires_full_again() {
        // The bug this guards against: a stale "already fired once" flag
        // that never resets on re-enable would give ConfidenceOnly here
        // instead of Full, silently dropping the one-time full recheck a
        // user re-enabling auto-refresh expects.
        assert_eq!(scope_for_poll_tick(false, true), Some(CheckScope::Full));
    }
}
