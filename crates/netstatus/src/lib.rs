mod connect;
mod dns;
mod interfaces;
mod reachability;
mod resolution;
mod status;
mod vpn;

pub use connect::{ConnectResult, connect, connect_all};
pub use dns::{Resolver, has_split_dns, list_resolvers};
pub use interfaces::{Interface, list_interfaces};
pub use reachability::{PingResult, ping, ping_all};
pub use resolution::{DEFAULT_RESOLUTION_TARGETS, ResolutionResult, resolve, resolve_all};
pub use status::{DEFAULT_PING_TARGETS, NetworkStatus, collect};
pub use vpn::{VpnStatus, vpn_status};
