# ![netcheck icon](assets/icon-1024.png) netcheck

A holistic view of network status on macOS: interfaces, VPN/tunnel state,
DNS resolvers (including split-tunnel/split-DNS), and reachability — the
things you end up checking manually when the Cisco VPN acts up.

netcheck reports:

- **Interfaces** — name, up/down, loopback, addresses
- **VPN / tunnel** — which tunnel interfaces have a real address, what your
  actual default route is, and whether that combination looks like a split
  tunnel
- **DNS** — every resolver macOS knows about, including scoped (per-interface)
  resolvers — the mechanism Cisco AnyConnect and similar clients use for
  split-DNS — and whether a scoped resolver is bound to a VPN tunnel
- **Reachability** — concurrent pings to well-known public DNS servers, and
  concurrent HTTPS connects to well-known domains (not ping, since some
  operators filter ICMP at the edge regardless of whether the service is up)
- **DNS resolution** — resolves well-known domains and reports success,
  addresses, and lookup latency
- **Path** — the OS's own `NWPath` verdict on the primary route: status
  (satisfied / unsatisfied / requires-connection), primary interface type, and
  whether the link is metered or in Low Data Mode

netcheck is macOS, Apple Silicon (arm64) only. It ships as a native SwiftUI
app plus a small `netcheck` CLI, both built on one Swift network-probing
engine (`NetStatus`).

## Install

### Homebrew (recommended)

```sh
brew tap hugoh/tap
brew install netcheck              # netcheck CLI
brew install --cask netcheck       # native SwiftUI app
```

You can install either or both — they're independent.

### Direct download

Grab the latest release from the
[Releases page](https://github.com/hugoh/netcheck/releases):

- `netcheck-<version>-aarch64-apple-darwin.tar.gz` — the `netcheck` binary.
  Unpack and put it on your `PATH`.
- `NetCheck-<version>.zip` — the native app. Unzip and drag
  `NetCheck.app` to `/Applications`.

> [!NOTE]
> Both artifacts are ad-hoc signed, not notarized with a Developer ID.
> Homebrew strips the quarantine attribute on install, so they run
> normally. With a direct download, macOS flags the app as from an
> unidentified developer on first launch — right-click (or Control-click)
> and choose **Open**, or clear it: `xattr -cr /Applications/NetCheck.app`
> (for the CLI: `xattr -d com.apple.quarantine ./netcheck`).

### From source

Requires the Swift toolchain (Xcode or the Command Line Tools).

```sh
mise run build:cli        # release netcheck CLI
mise run build:app        # dev build of NetCheck.app
mise run bundle:swiftui   # ad-hoc-signed release NetCheck.app bundle
```

or plain SwiftPM:

```sh
swift build --package-path apps/NetCheck
swift test  --package-path apps/NetCheck
```

`swift test` runs the deterministic suite (no network). Add the live network
and system probes with:

```sh
mise run test-live   # or: NETCHECK_LIVE_TESTS=1 swift test --package-path apps/NetCheck
```

Lint, format, and dead-code checks run together with:

```sh
mise run check   # swiftlint + swiftformat + periphery + icon
mise run fix     # apply swiftformat
```

## Usage

The CLI prints JSON to stdout — one snapshot per subcommand, meant for
scripting and piping into `jq`:

```sh
netcheck                     # full snapshot as JSON (same as `netcheck status`)
netcheck interfaces
netcheck dns
netcheck vpn
netcheck proxy
netcheck wifi
netcheck path                # primary network path (NWPath)
netcheck captive             # captive-portal status
netcheck connection          # connectivity tier ("online" / "limited" / "offline")
netcheck ping 1.1.1.1 8.8.8.8
netcheck resolve google.com github.com
netcheck connect amazon.com microsoft.com --port 443
```

Example — `netcheck vpn`:

```json
{
  "tunnels": [],
  "primary_interface": "en0",
  "connected": false,
  "split_tunnel": false,
  "routed_subnets": []
}
```

Output is JSON; there is no stability guarantee beyond that. `null`-valued
fields are omitted.

## NetCheck.app

![NetCheck.app screenshot](assets/screenshots/netcheck-app.png)

The app shows the same data across four tabs (Overview, DNS, Reachability,
Wi-Fi) and auto-refreshes: a re-check fires immediately on any OS
network-path change (interface up/down, IP/DNS reconfig, a VPN connecting
or dropping), backed by an adaptive-interval poll (60s while healthy, 10s
once a check comes back degraded, a full re-check every fifth tick) as a
safety net for failures that produce no path event — a blackholed route, a
captive-portal sign-in on an unchanged Wi-Fi association.

Fields fill in progressively as each probe finishes, so a slow probe (the
captive-portal check has a 10s timeout) never blocks the rest of the
panel. Shortcuts: `⌘R` refresh, `⌘A` toggle auto-refresh, `⌘1`–`⌘4` switch
tabs, `⌘Q` quit.

Each status collection includes a plain-HTTP (not HTTPS) request to
`captive.apple.com` — the same captive-portal-check endpoint macOS itself
uses — to determine whether the network is behind a captive portal.
