#[cfg(feature = "tui")]
pub mod tui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "netcheck",
    version = netstatus::VERSION,
    about = "Holistic view of macOS network status",
    long_about = "Holistic view of macOS network status.\n\n\
        Run with no subcommand for the interactive dashboard. Every \
        subcommand prints JSON to stdout, except `stream`, which prints \
        NDJSON (one JSON object per line, as each field becomes ready) \
        instead of a single blocking snapshot."
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
        Command::Interfaces => print_json(&netstatus::list_interfaces()),
        Command::Dns => print_json(&netstatus::list_resolvers()),
        Command::Vpn => print_json(&netstatus::vpn_status(&netstatus::list_interfaces())),
        Command::Proxy => print_json(&netstatus::proxy_config()),
        Command::Wifi => print_json(&netstatus::wifi_status()),
        Command::Captive => print_json(&netstatus::check_captive_portal()),
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
