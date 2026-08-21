# netcheck

A holistic view of network status on macOS: interfaces, VPN/tunnel state,
DNS resolvers (including split-tunnel/split-DNS), and reachability — the
things you end up checking manually when the Cisco VPN acts up.

## Layout

```text
crates/
  netstatus/      core diagnostics logic (no UI) — shared by CLI/TUI/GUI
  netcheck-cli/   `netcheck` CLI, JSON output
  netcheck-tui/   `netcheck-tui` terminal dashboard (ratatui)
  netcheck-gui/   `netcheck-gui` native window (egui/eframe), deliberately plain
apps/
  NetCheckMac/    a real SwiftUI macOS app (Swift Package) — shells out to
                  `netcheck status` and decodes its JSON. Genuinely native
                  window chrome, vibrancy, Dark Mode, and widgets (GroupBox,
                  List, SF Symbols) for free — it's actually AppKit under
                  the hood, unlike egui's custom-drawn immediate-mode GUI.
```

`netcheck-gui` (egui) can never look pixel-native on macOS — it draws every
widget itself rather than using AppKit, so no amount of color tuning changes
that. `apps/NetCheckMac` is the answer when you want it to look like a Mac
app.

`netstatus` shells out to `scutil --dns`, `scutil --nwi`, and `ping`, and
uses the `netdev` crate for interface enumeration. Each OS-facing call is a
thin wrapper around a pure, unit-tested parser/transform function.

### What it reports

- **Interfaces**: name, up/down, loopback, addresses
- **VPN / tunnel**: which `utun`/`ppp` interfaces have a real (non-link-local)
  address, the primary (default-route) interface from `scutil --nwi`, and
  whether that combination looks like a split tunnel (VPN tunnel(s) present
  but the primary route is still the physical interface)
- **DNS**: every resolver block from `scutil --dns`, including scoped
  (per-interface) resolvers — the mechanism Cisco AnyConnect and similar
  clients use for split-DNS — and whether any scoped resolver is bound to a
  VPN tunnel
- **Reachability (IPs)**: concurrent ICMP pings to well-known public DNS IPs
  (`1.1.1.1`, `1.0.0.1`, `8.8.8.8`, `8.8.4.4`, `9.9.9.9`, `208.67.222.222`,
  `208.67.220.220`)
- **Reachability (domains)**: concurrent TCP connects to port 443 on the
  well-known domain list — not ICMP ping, since several large operators
  (Amazon, Microsoft) filter ICMP at their edge regardless of whether the
  service itself is up, which made ping-based checks report false timeouts
- **DNS resolution**: resolves the well-known domain list (`google.com`,
  `cloudflare.com`, `github.com`, `apple.com`, `microsoft.com`,
  `amazon.com`, `wikipedia.org`), reporting success, resolved addresses, and
  lookup latency

## Usage

```text
cargo run -p netcheck-cli -- status     # full snapshot as JSON
cargo run -p netcheck-cli -- interfaces
cargo run -p netcheck-cli -- dns
cargo run -p netcheck-cli -- vpn
cargo run -p netcheck-cli -- ping 1.1.1.1 8.8.8.8
cargo run -p netcheck-cli -- resolve google.com github.com
cargo run -p netcheck-cli -- connect amazon.com microsoft.com --port 443

# live terminal dashboard: q quit, r refresh, a toggle auto-refresh
cargo run -p netcheck-tui

# native window: auto-refresh off by default, [a] or checkbox to toggle
cargo run -p netcheck-gui

# build+sign a release .app bundle, then launch it
mise run bundle:macos
open target/NetCheck.app

# the native SwiftUI app (r refresh, a toggle auto-refresh)
cd apps/NetCheckMac && swift run -c release

# or: build dev versions of every binary, including a working NetCheckMac.app
mise run build:dev
open target/dev-app/NetCheckMac.app
```

## Status

Working prototype: all three front ends (CLI, TUI, GUI) share the same
`netstatus` core and refresh on a 5s interval (TUI/GUI) or on demand (CLI).

Not yet covered: proxy configuration (`scutil --proxy`), IPv6-only
reachability nuances, and Wi-Fi-specific diagnostics (SSID, signal, channel).
