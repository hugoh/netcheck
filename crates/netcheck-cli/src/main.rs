use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "netcheck", about = "Holistic view of macOS network status")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print a full network status snapshot as JSON
    Status,
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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => print_json(&netstatus::collect()),
        Command::Interfaces => print_json(&netstatus::list_interfaces()),
        Command::Dns => print_json(&netstatus::list_resolvers()),
        Command::Vpn => print_json(&netstatus::vpn_status(&netstatus::list_interfaces())),
        Command::Proxy => print_json(&netstatus::proxy_config()),
        Command::Wifi => print_json(&netstatus::wifi_status()),
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
