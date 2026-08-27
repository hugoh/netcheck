#[cfg(feature = "tui")]
pub mod tui;
pub mod watch;

use clap::{Parser, Subcommand};
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

#[derive(Subcommand)]
pub enum Command {
    /// Print a full network status snapshot as JSON
    Status,
    /// Stream each status field as NDJSON (one JSON object per line), as
    /// soon as it's ready
    Stream,
    /// Like `stream`, but re-runs on every OS network-config change (and
    /// periodically as a safety net) instead of exiting after one pass —
    /// runs until interrupted
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

/// Runs a JSON subcommand to completion, printing its result to stdout.
pub fn run_command(command: Command) {
    match command {
        Command::Status => print_json(&netstatus::collect()),
        Command::Stream => {
            use std::io::Write;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || netstatus::collect_streaming(tx));
            let mut stdout = std::io::stdout();
            for field in rx {
                let _ = writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&field).expect("serializable status field")
                );
                let _ = stdout.flush();
            }
        }
        Command::Watch {
            interval,
            degraded_interval,
        } => {
            use std::io::Write;
            use std::sync::Mutex;
            use std::sync::atomic::AtomicU64;
            let (tx, rx) = std::sync::mpsc::channel();
            let enabled = Arc::new(AtomicBool::new(true));
            let health = Arc::new(AtomicU8::new(watch::HEALTHY));
            let generation = Arc::new(AtomicU64::new(0));
            // Accumulates across fires (not reset per-run) so a
            // confidence-only fire — which never sends a fresh
            // `CaptivePortal` field — still has the last known one to
            // derive confidence from, instead of `confidence()` going
            // `None` and health tracking silently freezing at HEALTHY.
            let partial = Arc::new(Mutex::new(netstatus::PartialStatus::default()));
            let fire = {
                let tx = tx.clone();
                let health = health.clone();
                let generation = generation.clone();
                let partial = partial.clone();
                move |scope: watch::CheckScope| {
                    let this_gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let (inner_tx, inner_rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || match scope {
                        watch::CheckScope::Full => netstatus::collect_streaming(inner_tx),
                        watch::CheckScope::ConfidenceOnly => {
                            netstatus::collect_confidence_streaming(inner_tx)
                        }
                    });
                    for field in inner_rx {
                        partial.lock().unwrap().merge(field.clone());
                        if generation.load(Ordering::SeqCst) == this_gen {
                            let _ = tx.send(field);
                        }
                    }
                    let degraded = partial.lock().unwrap().confidence()
                        == Some(netstatus::ConnectionConfidence::Offline);
                    health.store(
                        if degraded { watch::DEGRADED } else { watch::HEALTHY },
                        Ordering::Relaxed,
                    );
                }
            };
            // `spawn_watch_trigger`'s poll thread fires its first run as
            // Full precisely so CLI `watch` (which starts `enabled`) gets a
            // full snapshot immediately, without a second racing initial
            // fetch that the generation guard above would just discard the
            // slower of two concurrent runs of.
            let intervals = watch::Intervals {
                healthy: std::time::Duration::from_secs(interval),
                degraded: std::time::Duration::from_secs(degraded_interval),
            };
            let _trigger = watch::spawn_watch_trigger(enabled, health, intervals, fire)
                .expect("failed to start config-change watcher");
            let mut stdout = std::io::stdout();
            for field in rx {
                let _ = writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&field).expect("serializable status field")
                );
                let _ = stdout.flush();
            }
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
