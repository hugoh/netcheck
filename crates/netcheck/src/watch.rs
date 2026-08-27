//! Shared trigger machinery for both the TUI's auto-refresh and the `watch`
//! CLI subcommand: a cheap event-driven recheck on OS config changes,
//! backed by an adaptive-interval poll as a safety net for failures that
//! produce no config-change event at all (e.g. a blackholed route).

use netstatus::{StatusField, Trigger, WatcherState};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
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
/// so it warrants a full recheck. Most poll ticks only need the
/// confidence-determining probes — re-running everything else every 60s
/// would be pure waste (notably `wifi_identity`'s `system_profiler`
/// shell-out). But every `POLL_FULL_EVERY` ticks the poll thread does fire
/// a `Full` anyway, to catch a descriptive-config change that produced no
/// OS config event (a captive-portal sign-in on unchanged Wi-Fi, a silent
/// resolver swap) — see `poll_tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckScope {
    Full,
    ConfidenceOnly,
}

impl From<CheckScope> for netstatus::Scope {
    fn from(scope: CheckScope) -> Self {
        match scope {
            CheckScope::Full => netstatus::Scope::Full,
            CheckScope::ConfidenceOnly => netstatus::Scope::Confidence,
        }
    }
}

/// Runs one collection pass — full or confidence-only per `scope` — calling
/// `on_field` for every field as it arrives (e.g. to fold it into a running
/// `PartialStatus`) and then passing it to `forward`, but only as long as
/// `generation` still matches `this_gen` — if a newer run (manual or auto)
/// has started in the meantime, this run's remaining fields are not
/// forwarded, instead of overwriting fresher data. `on_field` always sees
/// every field regardless of generation, since a stale run's data is still
/// the most recent thing known until a newer run's fields replace it.
/// `forward` is where the caller sends downstream — a raw `StatusField` for
/// the TUI's channel, a `StreamEvent::Field` wrapper for the CLI's wire.
///
/// Shared by the CLI `watch` subcommand and the TUI's auto-refresh, which
/// both run this same fire/generation-guard dance but differ in what they
/// do with each field (the CLI folds it into a `PartialStatus` to derive
/// `health`; the TUI merges it into the screen's own `PartialStatus` in its
/// render loop instead).
pub fn run_and_forward(
    generation: &Arc<AtomicU64>,
    this_gen: u64,
    scope: CheckScope,
    mut on_field: impl FnMut(&StatusField),
    mut forward: impl FnMut(StatusField),
) {
    let (inner_tx, inner_rx) = mpsc::channel();
    std::thread::spawn(move || match scope {
        CheckScope::Full => netstatus::collect_streaming(inner_tx),
        CheckScope::ConfidenceOnly => netstatus::collect_confidence_streaming(inner_tx),
    });
    for field in inner_rx {
        on_field(&field);
        if generation.load(Ordering::SeqCst) == this_gen {
            forward(field);
        }
    }
}

/// One coalesced request queued behind an in-flight collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFire {
    pub scope: CheckScope,
    pub trigger: Trigger,
    pub token: Option<String>,
}

/// Merge a newly-requested fire into whatever is already queued behind the
/// in-flight one. Scope escalates to `Full` if either side is `Full`; a
/// `Command` fire wins the trigger/token slot (so a manual refresh that
/// lands mid-check still gets its `CheckStarted` token echoed), otherwise
/// the newer request wins.
fn coalesce(pending: Option<PendingFire>, incoming: PendingFire) -> PendingFire {
    let Some(pending) = pending else {
        return incoming;
    };
    let scope = if pending.scope == CheckScope::Full || incoming.scope == CheckScope::Full {
        CheckScope::Full
    } else {
        CheckScope::ConfidenceOnly
    };
    let (trigger, token) = if incoming.trigger == Trigger::Command {
        (incoming.trigger, incoming.token)
    } else if pending.trigger == Trigger::Command {
        (pending.trigger, pending.token)
    } else {
        (incoming.trigger, incoming.token)
    };
    PendingFire {
        scope,
        trigger,
        token,
    }
}

struct SingleFlightState {
    in_flight: bool,
    pending: Option<PendingFire>,
}

/// Serializes the three `fire` callers (config-change watcher, poll thread,
/// stdin command thread) onto one collection at a time. A request that
/// arrives while a collection is running is coalesced into a single
/// follow-up (see `coalesce`) rather than launching a concurrent
/// `collect_streaming` — which would waste probes and race the caller's
/// post-run `health` write.
pub struct SingleFlight<F> {
    raw: F,
    state: Mutex<SingleFlightState>,
}

impl<F: Fn(CheckScope, Trigger, Option<String>)> SingleFlight<F> {
    pub fn new(raw: F) -> Self {
        SingleFlight {
            raw,
            state: Mutex::new(SingleFlightState {
                in_flight: false,
                pending: None,
            }),
        }
    }

    pub fn fire(&self, scope: CheckScope, trigger: Trigger, token: Option<String>) {
        let incoming = PendingFire {
            scope,
            trigger,
            token,
        };
        {
            let mut state = self.state.lock().unwrap();
            if state.in_flight {
                state.pending = Some(coalesce(state.pending.take(), incoming));
                return;
            }
            state.in_flight = true;
        }
        let mut current = incoming;
        loop {
            (self.raw)(current.scope, current.trigger, current.token);
            let mut state = self.state.lock().unwrap();
            match state.pending.take() {
                Some(next) => {
                    drop(state);
                    current = next;
                }
                None => {
                    state.in_flight = false;
                    return;
                }
            }
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

/// Confidence-only poll ticks between the poll thread's periodic `Full`
/// rechecks. The confidence-only tick is the cheap common case; a `Full`
/// every `POLL_FULL_EVERY` ticks is the safety net for a descriptive-config
/// change that produced no OS config event — most importantly a
/// captive-portal sign-in (unchanged Wi-Fi association, so no
/// `SCDynamicStore` notification), but also e.g. a DHCP renewal that swaps
/// resolvers without macOS flagging it. At the 60s healthy interval that's
/// a full recheck about every 5 minutes; at the 10s degraded interval,
/// roughly every 50s.
const POLL_FULL_EVERY: u32 = 5;

/// The scope + trigger for one poll-thread iteration (or `None` — don't
/// fire, idle-sleep instead). `confidence_streak` is how many
/// confidence-only ticks have fired since the last `Full` one. Pulled out
/// of the poll loop so its three cases — disabled, the false -> true
/// transition (always `Full`, not just the thread's very first iteration),
/// and the periodic-`Full` cadence — are unit-testable without real
/// threads/timers.
fn poll_tick(
    was_enabled: bool,
    is_enabled: bool,
    confidence_streak: u32,
) -> Option<(CheckScope, Trigger)> {
    if !is_enabled {
        return None;
    }
    if !was_enabled {
        return Some((CheckScope::Full, Trigger::Initial));
    }
    if confidence_streak >= POLL_FULL_EVERY {
        return Some((CheckScope::Full, Trigger::Poll));
    }
    Some((CheckScope::ConfidenceOnly, Trigger::Poll))
}

/// Spawns the config-change watcher and the adaptive-poll thread, both
/// gated on `enabled`: while `false`, neither one calls `fire`, matching the
/// existing "only manual refresh" behavior when auto-refresh is off. The
/// config-change watcher always fires with `CheckScope::Full`; the poll
/// thread fires `CheckScope::Full` the first time it runs after becoming
/// enabled and every `POLL_FULL_EVERY` ticks thereafter,
/// `CheckScope::ConfidenceOnly` on the ticks in between (see `poll_tick`).
///
/// If the config-change watcher fails to start (e.g. `SCDynamicStore` setup
/// rejected in a sandboxed environment), this doesn't fail outright — it
/// still returns a `WatchTrigger` running the poll thread alone (and a
/// `WatcherState::Unavailable` for the caller to report in `Hello`), so
/// `watch` degrades to poll-only instead of refusing to run at all.
///
/// `fire` is called with the check's `scope`, the `Trigger` that caused it
/// (`ConfigChange` from the watcher; `Initial` for the poll thread's first
/// fire after becoming enabled, `Poll` thereafter), and no token (only a
/// stdin `WatchCommand` carries one).
///
/// `health` is written by the caller after each check completes (`HEALTHY`
/// or `DEGRADED`, from the latest `ConnectionConfidence`) and read here to
/// pick the poll thread's next sleep duration from `intervals`.
pub fn spawn_watch_trigger(
    enabled: Arc<AtomicBool>,
    health: Arc<AtomicU8>,
    intervals: Intervals,
    fire: impl Fn(CheckScope, Trigger, Option<String>) + Send + Sync + 'static,
) -> (WatchTrigger, WatcherState) {
    let fire = Arc::new(fire);

    let sc_handle = {
        let enabled = enabled.clone();
        let fire = fire.clone();
        netstatus::watch_config_changes(move || {
            if enabled.load(Ordering::Relaxed) {
                fire(CheckScope::Full, Trigger::ConfigChange, None);
            }
        })
        .inspect_err(|err| {
            eprintln!("netcheck: config-change watcher unavailable, falling back to poll-only: {err}");
        })
        .ok()
    };
    let watcher_state = if sc_handle.is_some() {
        WatcherState::Active
    } else {
        WatcherState::Unavailable
    };

    let poll_thread = std::thread::spawn(move || {
        // Reset whenever `enabled` transitions false -> true, not just once
        // for the thread's whole lifetime — a user toggling auto-refresh
        // off and back on needs a fresh `Full` fire, since a real network
        // change during the "off" window (dropped by both this thread and
        // the config-change watcher, which are gated on `enabled` too)
        // would otherwise go unnoticed until the next poll tick after that.
        let mut was_enabled = false;
        let mut confidence_streak: u32 = 0;
        loop {
            let is_enabled = enabled.load(Ordering::Relaxed);
            match poll_tick(was_enabled, is_enabled, confidence_streak) {
                Some((scope, trigger)) => {
                    confidence_streak = match scope {
                        CheckScope::ConfidenceOnly => confidence_streak + 1,
                        CheckScope::Full => 0,
                    };
                    fire(scope, trigger, None);
                    std::thread::sleep(interval_for(intervals, health.load(Ordering::Relaxed)));
                }
                None => std::thread::sleep(IDLE_POLL_INTERVAL),
            }
            was_enabled = is_enabled;
        }
    });

    (
        WatchTrigger {
            _sc_handle: sc_handle,
            _poll_thread: poll_thread,
        },
        watcher_state,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_iteration_fires_full() {
        assert_eq!(poll_tick(false, true, 0), Some((CheckScope::Full, Trigger::Initial)));
    }

    #[test]
    fn subsequent_enabled_iterations_fire_confidence_only() {
        assert_eq!(
            poll_tick(true, true, 0),
            Some((CheckScope::ConfidenceOnly, Trigger::Poll))
        );
    }

    #[test]
    fn a_full_recheck_fires_once_the_confidence_streak_hits_the_threshold() {
        assert_eq!(
            poll_tick(true, true, POLL_FULL_EVERY - 1),
            Some((CheckScope::ConfidenceOnly, Trigger::Poll))
        );
        assert_eq!(
            poll_tick(true, true, POLL_FULL_EVERY),
            Some((CheckScope::Full, Trigger::Poll))
        );
    }

    fn pending(scope: CheckScope, trigger: Trigger, token: Option<&str>) -> PendingFire {
        PendingFire {
            scope,
            trigger,
            token: token.map(str::to_string),
        }
    }

    #[test]
    fn coalesce_with_nothing_queued_keeps_the_incoming_request() {
        let incoming = pending(CheckScope::ConfidenceOnly, Trigger::Poll, None);
        assert_eq!(coalesce(None, incoming.clone()), incoming);
    }

    #[test]
    fn coalesce_escalates_scope_to_full_if_either_side_is_full() {
        let got = coalesce(
            Some(pending(CheckScope::Full, Trigger::ConfigChange, None)),
            pending(CheckScope::ConfidenceOnly, Trigger::Poll, None),
        );
        assert_eq!(got.scope, CheckScope::Full);
    }

    #[test]
    fn coalesce_keeps_a_command_token_over_a_poll_or_config_fire() {
        // Manual refresh queued, then a poll fire lands behind it: the
        // follow-up must still carry the caller's token so the client's
        // CheckStarted correlation holds.
        let got = coalesce(
            Some(pending(CheckScope::Full, Trigger::Command, Some("t1"))),
            pending(CheckScope::ConfidenceOnly, Trigger::Poll, None),
        );
        assert_eq!(got.trigger, Trigger::Command);
        assert_eq!(got.token.as_deref(), Some("t1"));

        // ...and the same the other way round (poll queued, command lands).
        let got = coalesce(
            Some(pending(CheckScope::ConfidenceOnly, Trigger::Poll, None)),
            pending(CheckScope::Full, Trigger::Command, Some("t2")),
        );
        assert_eq!(got.trigger, Trigger::Command);
        assert_eq!(got.token.as_deref(), Some("t2"));
    }

    #[test]
    fn coalesce_between_two_non_command_fires_takes_the_newer() {
        let got = coalesce(
            Some(pending(CheckScope::ConfidenceOnly, Trigger::Poll, None)),
            pending(CheckScope::ConfidenceOnly, Trigger::ConfigChange, None),
        );
        assert_eq!(got.trigger, Trigger::ConfigChange);
    }

    #[test]
    fn disabled_iterations_dont_fire() {
        assert_eq!(poll_tick(false, false, 0), None);
        assert_eq!(poll_tick(true, false, 0), None);
        assert_eq!(poll_tick(true, false, POLL_FULL_EVERY), None);
    }

    #[test]
    fn re_enabling_after_being_disabled_fires_full_again() {
        // The bug this guards against: a stale "already fired once" flag
        // that never resets on re-enable would give ConfidenceOnly here
        // instead of Full, silently dropping the one-time full recheck a
        // user re-enabling auto-refresh expects. The false -> true
        // transition wins even if the confidence streak also says it's
        // time for a periodic Full.
        assert_eq!(
            poll_tick(false, true, POLL_FULL_EVERY),
            Some((CheckScope::Full, Trigger::Initial))
        );
    }
}
