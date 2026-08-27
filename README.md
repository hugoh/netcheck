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

## Install

Three ways to get netcheck, in order of ease:

### Homebrew (recommended)

```sh
brew tap hugoh/tap
brew install netcheck              # netcheck binary
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

Both are Apple Silicon (arm64) only.

> [!NOTE]
> `NetCheck.app` is ad-hoc signed, not notarized with a Developer ID.
> The Homebrew cask strips the quarantine attribute on install, so it opens
> normally. With a direct download, macOS will flag it as from an
> unidentified developer on first launch — right-click (or Control-click)
> the app and choose **Open**, or clear it manually:
> `xattr -cr /Applications/NetCheck.app`.

### From source

```sh
mise run build:tui       # netcheck
mise run build:app       # dev build of the native NetCheck.app
mise run bundle:swiftui  # signed release .app bundle of NetCheck
```

## The two flavors

netcheck ships as two front ends, both built on the same diagnostics core:

| | What it is | Run it |
|---|---|---|
| **`netcheck`** | Interactive dashboard by default, JSON with a subcommand | `netcheck` / `netcheck status` |
| **NetCheck.app** | Native macOS app | open from Applications, or `open /Applications/NetCheck.app` |

## Screenshots

**JSON** — `netcheck vpn`

```json
{
  "tunnels": [],
  "primary_interface": "en0",
  "connected": false,
  "split_tunnel": false,
  "routed_subnets": []
}
```

**Dashboard** — `netcheck`

```text
 Overview   DNS   Reachability   Wi-Fi
┌Interfaces────────────────────────────┐┌VPN / Tunnel────────────────┐┌Wi-Fi───────────────────────┐
│en0      UP, ROUTABLE    192.168.68.18││Primary: en0                ││SSID: -                     │
│awdl0    UP, LINK-LOCAL  fe80::xxxx:xx││VPN connected: false        ││Channel: 40 (5GHz, 160MHz)  │
│llw0     UP, LINK-LOCAL  fe80::xxxx:xx││Split tunnel: false         ││Signal: -44 dBm             │
│utun0    UP, LINK-LOCAL  fe80::8318:87││Split DNS: false            ││Noise: -92 dBm              │
│utun1    UP, LINK-LOCAL  fe80::1ab6:1a││                            ││Security: WPA3 Personal     │
│utun2    UP, LINK-LOCAL  fe80::822d:1f││                            ││PHY mode: 802.11ax          │
└──────────────────────────────────────┘│                            ││                            │
┌Interface detail──────────────────────┐└────────────────────────────┘│                            │
│en0                                   │┌Proxy───────────────────────┐│                            │
│  192.168.68.186/24           routable││HTTP: off                   ││                            │
│  fe80::42f:a459:cbee:c64e/64 link-loc││HTTPS: off                  │└────────────────────────────┘
└──────────────────────────────────────┘└────────────────────────────┘└────────────────────────────┘
Online   q: quit   r: refresh now   a: auto-refresh (off)   1-4: tabs   updated 0s ago   netcheck dev-396365d
```

**NetCheck.app**:

![NetCheck.app screenshot](assets/screenshots/netcheck-app.png)

## Usage

```sh
netcheck                                     # interactive dashboard
netcheck status                              # full snapshot as JSON
netcheck interfaces
netcheck dns
netcheck vpn
netcheck captive                             # captive-portal status as JSON
netcheck connection                          # connection confidence as JSON
netcheck ping 1.1.1.1 8.8.8.8
netcheck resolve google.com github.com
netcheck connect amazon.com microsoft.com --port 443
netcheck stream                              # one snapshot as NDJSON, field by field
netcheck watch                               # like stream, but keeps running — see below
netcheck schema                              # JSON Schema for stream/watch's NDJSON output

# dashboard keys: q quit, r refresh, a toggle auto-refresh, 1-4 switch tabs
```

The native app and the dashboard both auto-refresh (on by default — toggle
with `a` in the dashboard): a re-check fires immediately on any OS
network-config change (interface up/down, IP/DNS reconfig, VPN connect),
backed by an adaptive-interval poll (60s while healthy, 10s once a check
comes back degraded) as a safety net for failures that produce no config
event at all, like a blackholed route. `netcheck watch --interval
<secs> --degraded-interval <secs>` exposes the same behavior on the
command line. Other subcommands run on demand and are meant for
scripting/piping into `jq`.

## Wire format

`netcheck stream` and `netcheck watch` print NDJSON (one JSON object per
line) instead of a single blocking snapshot — each line is one `StatusField`
result as soon as that probe completes. `status`/`ping`/`resolve`/etc.
still print one complete JSON object.

Two things worth knowing about the shape:

- **Externally tagged.** Each line is `{"VariantName": <data>}` — e.g.
  `{"SplitDns":false}` or `{"IpStack":"Ipv4Only"}`.
- **Target-list probes stream per-target, not per-list.** `Ping`, `Connect`,
  and `Resolution` (reachability pings, TCP connects, DNS resolution) send
  one message per target as soon as *that* target responds, instead of
  waiting for the slowest target in the group — so one unreachable host
  doesn't hold up the rest of the list from showing up:
  ```json
  {"Ping":{"group":"Reachability","result":{"target":"1.1.1.1","reachable":true,"rtt_ms":12.3}}}
  {"Ping":{"group":"Reachability","result":{"target":"8.8.8.8","reachable":true,"rtt_ms":9.1}}}
  {"GroupComplete":"Reachability"}
  ```
  `GroupComplete` marks a target list as fully drained; it's only sent for
  the four groups (`Resolution`, `Reachability`, `ReachabilityV6`,
  `DomainReachability`) that determine connection confidence, since that's
  the only place completeness (not just partial data) actually matters.

The full shape is defined as [JSON Schema](schema/status-field.schema.json),
generated straight from the Rust `StatusField` type via
[`schemars`](https://docs.rs/schemars) — run `netcheck schema` to print the
current version; it can't drift from the real wire format the way
hand-written docs could. Browse it rendered (via
[json-schema-for-humans](https://github.com/coveooss/json-schema-for-humans))
at [netcheck.larve.net/schema](https://netcheck.larve.net/schema/).

Each status collection — every CLI `status`/`stream` call, and every
dashboard auto-refresh — includes a plain HTTP (not HTTPS) request to
`captive.apple.com`, the same captive-portal-check endpoint macOS itself
uses, to determine whether the network is behind a captive portal.

NetCheck.app uses Cmd-modified shortcuts instead — `q` to quit, `⌘R` to
refresh, `⌘A` to toggle auto-refresh, `⌘1`-`⌘4` to switch tabs — shown in
its toolbar and footer, so typing in a field can't accidentally trigger
them.
