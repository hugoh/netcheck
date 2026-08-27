#[cfg(feature = "tui")]
pub mod tui;
pub mod watch;

use clap::{Parser, Subcommand, ValueEnum};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Parser)]
#[command(
    name = "netcheck",
    version = netstatus::VERSION,
    about = "Holistic view of macOS network status",
    long_about = "Holistic view of macOS network status.\n\n\
        Run with no subcommand for the interactive dashboard. Every \
        subcommand prints JSON to stdout, except `stream` and `watch`, \
        which print NDJSON (one JSON object per line, as each field \
        becomes ready) instead of a single blocking snapshot."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SchemaTarget {
    /// The `StreamEvent` envelope written to stdout by `stream`/`watch`.
    StreamEvent,
    /// Just the `StatusField` probe-result payload carried inside
    /// `StreamEvent::Field`.
    StatusField,
    /// The `WatchCommand` input `watch` reads from stdin.
    WatchCommand,
}

#[derive(Subcommand)]
pub enum Command {
    /// Print a full network status snapshot as JSON
    Status,
    /// Stream each status field as NDJSON (one JSON object per line), as
    /// soon as it's ready
    Stream,
    /// Like `stream`, but re-runs on every OS network-config change (and
    /// periodically as a safety net) instead of exiting after one pass —
    /// runs until interrupted. Also reads `WatchCommand` lines from stdin
    /// (`"Refresh"` or `{"RefreshToken":{"token":"…"}}` trigger an immediate
    /// check) — see the README's wire format section
    Watch {
        /// Poll interval in seconds while the connection looks healthy —
        /// the safety net for a failure that produces no OS config-change
        /// event at all (e.g. a blackholed route)
        #[arg(long, default_value_t = watch::Intervals::default().healthy.as_secs())]
        interval: u64,
        /// Poll interval in seconds once a check has come back degraded, so
        /// recovery (or a worsening outage) is noticed quickly
        #[arg(long, default_value_t = watch::Intervals::default().degraded.as_secs())]
        degraded_interval: u64,
    },
    /// Print JSON Schema for `stream`/`watch`'s NDJSON wire format
    Schema {
        /// Which schema to print: the `StreamEvent` envelope written to
        /// stdout by `stream`/`watch` (default), the bare `StatusField`
        /// payload inside it, or the `WatchCommand` input `watch` reads
        /// from stdin
        #[arg(value_enum, default_value_t = SchemaTarget::StreamEvent)]
        target: SchemaTarget,
    },
    /// Print network interfaces as JSON
    Interfaces,
    /// Print DNS resolver configuration as JSON
    Dns,
    /// Print VPN/tunnel status as JSON
    Vpn,
    /// Print proxy configuration as JSON
    Proxy,
    /// Print Wi-Fi diagnostics as JSON
    Wifi,
    /// Print captive-portal status as JSON
    Captive,
    /// Print connection confidence (derived from DNS/ICMP/TCP probes) as JSON
    Connection,
    /// Ping one or more hosts (defaults to well-known public DNS IPs)
    Ping {
        #[arg(default_values_t = netstatus::DEFAULT_PING_TARGETS.iter().map(|s| s.to_string()))]
        targets: Vec<String>,
    },
    /// Resolve one or more domains (defaults to a well-known domain list)
    Resolve {
        #[arg(default_values_t = netstatus::DEFAULT_RESOLUTION_TARGETS.iter().map(|s| s.to_string()))]
        domains: Vec<String>,
    },
    /// TCP connect to one or more hosts (defaults to a well-known domain list on port 443)
    Connect {
        #[arg(default_values_t = netstatus::DEFAULT_RESOLUTION_TARGETS.iter().map(|s| s.to_string()))]
        targets: Vec<String>,
        #[arg(short, long, default_value_t = 443)]
        port: u16,
    },
}

fn print_json<T: serde::Serialize>(value: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serializable status")
    );
}

/// Writes each `StreamEvent` from `rx` to stdout as one NDJSON line,
/// flushing after every line so a consumer piping this output sees each
/// event as soon as it's ready instead of buffered. Shared by `stream` and
/// `watch`, the two subcommands that print NDJSON instead of a single
/// blocking snapshot.
fn print_stream_events(rx: std::sync::mpsc::Receiver<netstatus::StreamEvent>) {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    for event in rx {
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string(&event).expect("serializable stream event")
        );
        let _ = stdout.flush();
    }
}

/// Protocol version reported in `StreamEvent::Hello`. Bumped only on a
/// breaking change to the `stream`/`watch` wire format.
const WIRE_PROTOCOL_VERSION: u32 = 1;

/// `watch`'s liveness cadence — `StreamEvent::Heartbeat` is emitted this
/// often regardless of check activity, so a client can tell a wedged
/// collector from an idle one.
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Runs a JSON subcommand to completion, printing its result to stdout.
pub fn run_command(command: Command) {
    match command {
        Command::Status => print_json(&netstatus::collect()),
        Command::Schema { target } => match target {
            SchemaTarget::StreamEvent => print_json(&netstatus::stream_event_schema()),
            SchemaTarget::StatusField => print_json(&netstatus::status_field_schema()),
            SchemaTarget::WatchCommand => print_json(&netstatus::watch_command_schema()),
        },
        Command::Stream => {
            use netstatus::{Scope, StreamEvent, Trigger, WatcherState};
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(StreamEvent::Hello {
                    protocol_version: WIRE_PROTOCOL_VERSION,
                    config_watcher: WatcherState::Disabled,
                    healthy_interval_secs: 0,
                    degraded_interval_secs: 0,
                });
                let _ = tx.send(StreamEvent::CheckStarted {
                    generation: 1,
                    scope: Scope::Full,
                    trigger: Trigger::Initial,
                    token: None,
                });
                let (inner_tx, inner_rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || netstatus::collect_streaming(inner_tx));
                for field in inner_rx {
                    let _ = tx.send(StreamEvent::Field {
                        generation: 1,
                        field,
                    });
                }
                let _ = tx.send(StreamEvent::CheckComplete { generation: 1 });
            });
            print_stream_events(rx);
        }
        Command::Watch {
            interval,
            degraded_interval,
        } => {
            use netstatus::{Scope, StreamEvent, Trigger, WatchCommand};
            use std::sync::Mutex;
            use std::sync::atomic::AtomicU64;
            let (tx, rx) = std::sync::mpsc::channel::<StreamEvent>();
            let enabled = Arc::new(AtomicBool::new(true));
            let health = Arc::new(AtomicU8::new(watch::HEALTHY));
            let generation = Arc::new(AtomicU64::new(0));
            // Held false until `Hello` has been queued, so nothing — poll,
            // config-change, or stdin command — can put a `CheckStarted`
            // ahead of the protocol's mandatory first line.
            let hello_sent = Arc::new(AtomicBool::new(false));
            // Accumulates across fires (not reset per-run) so a
            // confidence-only fire — which never sends a fresh
            // `CaptivePortal` field — still has the last known one to
            // derive confidence from, instead of `confidence()` going
            // `None` and health tracking silently freezing at HEALTHY.
            let partial = Arc::new(Mutex::new(netstatus::PartialStatus::default()));
            let raw_fire = {
                let tx = tx.clone();
                let health = health.clone();
                let generation = generation.clone();
                let partial = partial.clone();
                let hello_sent = hello_sent.clone();
                move |scope: watch::CheckScope, trigger: Trigger, token: Option<String>| {
                    while !hello_sent.load(Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    let this_gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = tx.send(StreamEvent::CheckStarted {
                        generation: this_gen,
                        scope: Scope::from(scope),
                        trigger,
                        token,
                    });
                    let forward_tx = tx.clone();
                    watch::run_and_forward(
                        &generation,
                        this_gen,
                        scope,
                        |field| {
                            partial.lock().unwrap().merge(field.clone());
                        },
                        move |field| {
                            let _ = forward_tx.send(StreamEvent::Field {
                                generation: this_gen,
                                field,
                            });
                        },
                    );
                    let degraded = partial.lock().unwrap().confidence()
                        == Some(netstatus::ConnectionConfidence::Offline);
                    health.store(
                        if degraded { watch::DEGRADED } else { watch::HEALTHY },
                        Ordering::Relaxed,
                    );
                    let _ = tx.send(StreamEvent::CheckComplete {
                        generation: this_gen,
                    });
                }
            };
            // `spawn_watch_trigger`'s poll thread fires its first run as
            // Full precisely so CLI `watch` (which starts `enabled`) gets a
            // full snapshot immediately, without a second racing initial
            // fetch that the generation guard would just discard the
            // slower of two concurrent runs of.
            let intervals = watch::Intervals {
                healthy: std::time::Duration::from_secs(interval),
                degraded: std::time::Duration::from_secs(degraded_interval),
            };
            // One collection at a time: the config watcher, the poll
            // thread, and the stdin thread below all fire through this, and
            // an overlapping request is coalesced into a single follow-up
            // rather than racing a second `collect_streaming`.
            let fire = Arc::new(watch::SingleFlight::new(raw_fire));
            {
                // A caller that already knows this `watch` process is
                // running wants an immediate check without waiting for the
                // next poll/config-change — e.g. the SwiftUI app's manual
                // refresh, writing a line to this process's stdin instead
                // of spawning a second subprocess.
                let fire = fire.clone();
                let tx = tx.clone();
                let hello_sent = hello_sent.clone();
                std::thread::spawn(move || {
                    while !hello_sent.load(Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    for line in std::io::stdin().lines() {
                        // A transient read error (e.g. an invalid UTF-8
                        // byte on the pipe) is skipped, not fatal; only
                        // actual stdin EOF ends the loop.
                        let Ok(line) = line else { continue };
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<WatchCommand>(&line) {
                            Ok(WatchCommand::Refresh) => {
                                fire.fire(watch::CheckScope::Full, Trigger::Command, None);
                            }
                            Ok(WatchCommand::RefreshToken { token }) => {
                                fire.fire(watch::CheckScope::Full, Trigger::Command, Some(token));
                            }
                            Err(err) => {
                                let _ = tx.send(StreamEvent::Error {
                                    message: format!("unrecognized command: {err}"),
                                    fatal: false,
                                });
                            }
                        }
                    }
                });
            }
            {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(HEARTBEAT_INTERVAL);
                        if tx.send(StreamEvent::Heartbeat {}).is_err() {
                            break;
                        }
                    }
                });
            }
            let (_trigger, config_watcher) = watch::spawn_watch_trigger(enabled, health, intervals, {
                let fire = fire.clone();
                move |scope, trigger, token| fire.fire(scope, trigger, token)
            });
            let _ = tx.send(StreamEvent::Hello {
                protocol_version: WIRE_PROTOCOL_VERSION,
                config_watcher,
                healthy_interval_secs: interval,
                degraded_interval_secs: degraded_interval,
            });
            hello_sent.store(true, Ordering::Release);
            print_stream_events(rx);
        }
        Command::Interfaces => print_json(&netstatus::list_interfaces()),
        Command::Dns => print_json(&netstatus::list_resolvers()),
        Command::Vpn => print_json(&netstatus::vpn_status(&netstatus::list_interfaces())),
        Command::Proxy => print_json(&netstatus::proxy_config()),
        Command::Wifi => print_json(&netstatus::wifi_status()),
        Command::Captive => print_json(&netstatus::check_captive_portal()),
        Command::Connection => print_json(&netstatus::confidence_only()),
        Command::Ping { targets } => {
            let refs: Vec<&str> = targets.iter().map(String::as_str).collect();
            print_json(&netstatus::ping_all(&refs));
        }
        Command::Resolve { domains } => {
            let refs: Vec<&str> = domains.iter().map(String::as_str).collect();
            print_json(&netstatus::resolve_all(&refs));
        }
        Command::Connect { targets, port } => {
            let refs: Vec<&str> = targets.iter().map(String::as_str).collect();
            print_json(&netstatus::connect_all(&refs, port));
        }
    }
}
