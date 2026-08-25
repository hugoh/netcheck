use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, execute};
use netstatus::StatusField;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::io::{self, Stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// Pads `text` to `width` columns with a single trailing separator space.
/// The one place list-row column widths are defined, so a new/reordered
/// list can't glue its target and status text together the way two past
/// bugs did by hand-writing `format!("{:<N} ", ...)` at each call site.
fn pad_col(text: &str, width: usize) -> String {
    format!("{text:<width$} ")
}

/// Formats an elapsed duration as "now" under 3s, seconds under a minute,
/// minutes under an hour, hours above it — "42s" reads fine, "3717s" doesn't.
fn format_age(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 3 {
        "now".to_string()
    } else if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Overview,
    Dns,
    Reachability,
    Wifi,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Dns => "DNS",
            Tab::Reachability => "Reachability",
            Tab::Wifi => "Wi-Fi",
        }
    }

    fn from_digit(n: u8) -> Option<Tab> {
        match n {
            1 => Some(Tab::Overview),
            2 => Some(Tab::Dns),
            3 => Some(Tab::Reachability),
            4 => Some(Tab::Wifi),
            _ => None,
        }
    }
}

const TABS: [Tab; 4] = [Tab::Overview, Tab::Dns, Tab::Reachability, Tab::Wifi];

fn tabs_line(active: Tab) -> Line<'static> {
    let spans: Vec<Span> = TABS
        .iter()
        .flat_map(|&tab| {
            let style = if tab == active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            vec![
                Span::styled(format!(" {} ", tab.title()), style),
                Span::raw(" "),
            ]
        })
        .collect();
    Line::from(spans)
}

/// Spawns the background workers. Returns a receiver fed by both an initial
/// one-shot collection, a periodic auto-refresh (gated by `auto_refresh`,
/// off by default), and manual refreshes triggered via the returned sender.
fn spawn_workers(auto_refresh: Arc<AtomicBool>) -> (mpsc::Receiver<StatusField>, mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel();
    let (manual_tx, manual_rx) = mpsc::channel::<()>();

    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for () in manual_rx {
                netstatus::collect_streaming(tx.clone());
            }
        });
    }

    std::thread::spawn(move || {
        netstatus::collect_streaming(tx.clone());
        loop {
            if auto_refresh.load(Ordering::Relaxed) {
                std::thread::sleep(REFRESH_INTERVAL);
                if auto_refresh.load(Ordering::Relaxed) {
                    netstatus::collect_streaming(tx.clone());
                }
            } else {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    (rx, manual_tx)
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn interface_class_label(class: netstatus::InterfaceClass) -> (&'static str, Color) {
    match class {
        netstatus::InterfaceClass::Down => ("DOWN            ", Color::DarkGray),
        netstatus::InterfaceClass::Unaddressed => ("UP, NO ADDRESS  ", Color::Red),
        netstatus::InterfaceClass::LinkLocalOnly => ("UP, LINK-LOCAL  ", Color::Yellow),
        netstatus::InterfaceClass::Routable => ("UP, ROUTABLE    ", Color::Green),
    }
}

fn sorted_non_loopback(interfaces: &[netstatus::Interface]) -> Vec<&netstatus::Interface> {
    let mut interfaces: Vec<&netstatus::Interface> =
        interfaces.iter().filter(|i| !i.loopback).collect();
    interfaces.sort_by_key(|i| netstatus::classify_interface(i));
    interfaces
}

fn interface_list_items(
    interfaces: Option<&[netstatus::Interface]>,
    render: impl FnOnce(Vec<&netstatus::Interface>) -> Vec<ListItem<'static>>,
) -> Vec<ListItem<'static>> {
    match interfaces {
        None => vec![ListItem::new("Collecting...")],
        Some(interfaces) => render(sorted_non_loopback(interfaces)),
    }
}

fn interfaces_list(interfaces: Option<&[netstatus::Interface]>) -> List<'static> {
    let items = interface_list_items(interfaces, |interfaces| {
        interfaces
            .into_iter()
            .map(|i| {
                let (label, color) = interface_class_label(netstatus::classify_interface(i));
                let addrs = if i.addresses.is_empty() {
                    "-".to_string()
                } else {
                    i.addresses.join(", ")
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        pad_col(&i.name, 8),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(label, Style::default().fg(color)),
                    Span::raw(addrs),
                ]))
            })
            .collect()
    });
    List::new(items).block(Block::default().borders(Borders::ALL).title("Interfaces"))
}

fn interface_detail_list(interfaces: Option<&[netstatus::Interface]>) -> List<'static> {
    let items = interface_list_items(interfaces, |interfaces| {
        interfaces
            .into_iter()
            .flat_map(|i| {
                let header = ListItem::new(Line::from(Span::styled(
                    i.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                let addr_lines = if i.addresses.is_empty() {
                    vec![ListItem::new(Line::from(Span::raw("  (no addresses)")))]
                } else {
                    i.addresses
                        .iter()
                        .map(|addr| {
                            let (tag, color) = match netstatus::classify_address(addr) {
                                netstatus::AddressClass::LinkLocal => ("link-local", Color::Yellow),
                                netstatus::AddressClass::RoutableV4 => {
                                    ("routable v4", Color::Green)
                                }
                                netstatus::AddressClass::RoutableV6 => {
                                    ("routable v6", Color::Green)
                                }
                            };
                            ListItem::new(Line::from(vec![
                                Span::raw(format!("  {addr:<28}")),
                                Span::styled(tag, Style::default().fg(color)),
                            ]))
                        })
                        .collect()
                };
                std::iter::once(header).chain(addr_lines)
            })
            .collect()
    });
    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Interface detail"),
    )
}

fn vpn_paragraph(
    vpn: Option<&netstatus::VpnStatus>,
    split_dns: Option<bool>,
    resolvers: Option<&[netstatus::Resolver]>,
) -> Paragraph<'static> {
    let Some(vpn) = vpn else {
        return Paragraph::new("Collecting...")
            .block(Block::default().borders(Borders::ALL).title("VPN / Tunnel"));
    };
    let mut lines = vec![
        Line::from(format!(
            "Primary: {}",
            vpn.primary_interface
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!("VPN connected: {}", vpn.connected)),
        Line::from(format!("Split tunnel: {}", vpn.split_tunnel)),
        Line::from(format!(
            "Split DNS: {}",
            split_dns
                .map(|b| b.to_string())
                .unwrap_or_else(|| "collecting...".to_string())
        )),
    ];
    if !vpn.tunnels.is_empty() {
        lines.push(Line::from(format!("Tunnels: {}", vpn.tunnels.join(", "))));
    }
    if !vpn.routed_subnets.is_empty() {
        lines.push(Line::from(format!(
            "Routed subnets: {}",
            vpn.routed_subnets.join(", ")
        )));
    }
    if let Some(resolvers) = resolvers {
        let domains = netstatus::vpn_scoped_domains(resolvers);
        if !domains.is_empty() {
            lines.push(Line::from(format!("VPN domains: {}", domains.join(", "))));
        }
    }
    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("VPN / Tunnel"))
}

fn dns_list(resolvers: Option<&[netstatus::Resolver]>) -> List<'static> {
    let items: Vec<ListItem> = match resolvers {
        None => vec![ListItem::new("Collecting...")],
        Some(resolvers) => resolvers
            .iter()
            .filter(|r| !r.nameservers.is_empty())
            .map(|r| {
                let label = r
                    .domain
                    .clone()
                    .or_else(|| r.search_domains.first().cloned())
                    .unwrap_or_else(|| "*".to_string());
                let scope = r.if_name.clone().unwrap_or_else(|| "any".to_string());
                let color = if r.reachable {
                    Color::Green
                } else {
                    Color::Red
                };
                ListItem::new(Line::from(vec![
                    Span::styled(pad_col(&label, 20), Style::default().fg(color)),
                    Span::raw(pad_col(&scope, 10)),
                    Span::raw(r.nameservers.join(", ")),
                ]))
            })
            .collect(),
    };
    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("DNS resolvers"),
    )
}

fn ping_list(title: &'static str, results: &[netstatus::PingResult]) -> List<'static> {
    let items: Vec<ListItem> = results
        .iter()
        .map(|p| {
            let color = if p.reachable {
                Color::Green
            } else {
                Color::Red
            };
            let rtt = p
                .rtt_ms
                .map(|ms| format!("{ms:.1} ms"))
                .unwrap_or_else(|| "timeout".to_string());
            ListItem::new(Line::from(vec![
                Span::styled(pad_col(&p.target, 20), Style::default().fg(color)),
                Span::raw(rtt),
            ]))
        })
        .collect();
    List::new(items).block(Block::default().borders(Borders::ALL).title(title))
}

fn connect_list(title: &'static str, results: &[netstatus::ConnectResult]) -> List<'static> {
    let items: Vec<ListItem> = results
        .iter()
        .map(|c| {
            let color = if c.reachable {
                Color::Green
            } else {
                Color::Red
            };
            let rtt = c
                .rtt_ms
                .map(|ms| format!("{ms:.1} ms"))
                .unwrap_or_else(|| "unreachable".to_string());
            ListItem::new(Line::from(vec![
                Span::styled(pad_col(&c.target, 20), Style::default().fg(color)),
                Span::raw(format!(":{}  {}", c.port, rtt)),
            ]))
        })
        .collect();
    List::new(items).block(Block::default().borders(Borders::ALL).title(title))
}

fn resolution_list(resolution: Option<&[netstatus::ResolutionResult]>) -> List<'static> {
    let items: Vec<ListItem> = match resolution {
        None => vec![ListItem::new("Collecting...")],
        Some(resolution) => resolution
            .iter()
            .map(|r| {
                let color = if r.resolved { Color::Green } else { Color::Red };
                let detail = if r.resolved {
                    format!(
                        "{}  ({})",
                        r.duration_ms
                            .map(|ms| format!("{ms:.1} ms"))
                            .unwrap_or_default(),
                        r.addresses.first().cloned().unwrap_or_default()
                    )
                } else {
                    "failed".to_string()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(pad_col(&r.domain, 20), Style::default().fg(color)),
                    Span::raw(detail),
                ]))
            })
            .collect(),
    };
    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("DNS resolution"),
    )
}

fn proxy_paragraph(proxy: Option<&netstatus::ProxyConfig>) -> Paragraph<'static> {
    let Some(proxy) = proxy else {
        return Paragraph::new("Collecting...")
            .block(Block::default().borders(Borders::ALL).title("Proxy"));
    };

    let endpoint_line = |label: &str, endpoint: &netstatus::ProxyEndpoint| {
        if !endpoint.enabled {
            Line::from(format!("{label}: off"))
        } else {
            Line::from(format!(
                "{label}: {}:{}",
                endpoint.host.clone().unwrap_or_else(|| "?".to_string()),
                endpoint
                    .port
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "?".to_string())
            ))
        }
    };

    let mut lines = vec![
        endpoint_line("HTTP", &proxy.http),
        endpoint_line("HTTPS", &proxy.https),
        endpoint_line("SOCKS", &proxy.socks),
    ];
    lines.push(Line::from(match &proxy.pac_url {
        Some(url) => format!("PAC: {url}"),
        None => "PAC: off".to_string(),
    }));
    if !proxy.exceptions.is_empty() {
        lines.push(Line::from(format!(
            "Exceptions: {}",
            proxy.exceptions.join(", ")
        )));
    }

    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Proxy"))
}

fn ip_stack_paragraph(ip_stack: Option<netstatus::IpStack>) -> Paragraph<'static> {
    let text = match ip_stack {
        None => "Collecting...".to_string(),
        Some(netstatus::IpStack::Ipv4Only) => "IPv4 only".to_string(),
        Some(netstatus::IpStack::Ipv6Only) => "IPv6 only".to_string(),
        Some(netstatus::IpStack::DualStack) => "Dual-stack (IPv4 + IPv6)".to_string(),
        Some(netstatus::IpStack::None) => "No routable address".to_string(),
    };
    Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("IP stack"))
}

/// Wi-Fi status arrives as two independent, independently-paced probes:
/// `radio` (channel/signal/noise/security/PHY-mode) is fast CoreWLAN, no
/// shell-out, and typically shows up immediately; `identity` (SSID,
/// connected-state) is slow `system_profiler`, commonly ~1s. Rendered
/// separately so radio fields aren't held hostage by the slow SSID lookup.
fn wifi_paragraph(
    identity: Option<&netstatus::WifiIdentity>,
    radio: Option<&netstatus::WifiRadio>,
) -> Paragraph<'static> {
    if identity.is_none() && radio.is_none() {
        return Paragraph::new("Collecting...")
            .block(Block::default().borders(Borders::ALL).title("Wi-Fi"));
    }
    if identity.is_some_and(|i| !i.connected) {
        return Paragraph::new("Not connected")
            .block(Block::default().borders(Borders::ALL).title("Wi-Fi"));
    }

    let field = |label: &str, value: Option<&String>| {
        Line::from(format!(
            "{label}: {}",
            value.cloned().unwrap_or_else(|| "-".to_string())
        ))
    };

    let mut lines = vec![match identity {
        Some(i) => field("SSID", i.ssid.as_ref()),
        None => Line::from("SSID: collecting..."),
    }];

    match radio {
        Some(r) => {
            lines.push(field("Channel", r.channel.as_ref()));
            lines.push(Line::from(format!(
                "Signal: {}",
                r.signal_dbm
                    .map(|d| format!("{d} dBm"))
                    .unwrap_or_else(|| "-".to_string())
            )));
            lines.push(Line::from(format!(
                "Noise: {}",
                r.noise_dbm
                    .map(|d| format!("{d} dBm"))
                    .unwrap_or_else(|| "-".to_string())
            )));
            lines.push(field("Security", r.security.as_ref()));
            lines.push(field("PHY mode", r.phy_mode.as_ref()));
        }
        None => lines.push(Line::from("Channel/signal: collecting...")),
    }

    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Wi-Fi"))
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    status: &netstatus::PartialStatus,
    active_tab: Tab,
    last_updated: Option<Instant>,
    auto_refresh: bool,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        frame.render_widget(Paragraph::new(tabs_line(active_tab)), rows[0]);

        if !status.has_any() {
            frame.render_widget(Paragraph::new("Collecting network status..."), rows[1]);
        } else {
            match active_tab {
                Tab::Overview => {
                    let cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Percentage(30),
                            Constraint::Percentage(30),
                        ])
                        .split(rows[1]);
                    let left = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                        .split(cols[0]);
                    let middle = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(cols[1]);
                    let right = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                        .split(cols[2]);

                    frame.render_widget(interfaces_list(status.interfaces.as_deref()), left[0]);
                    frame.render_widget(
                        interface_detail_list(status.interfaces.as_deref()),
                        left[1],
                    );
                    frame.render_widget(
                        vpn_paragraph(
                            status.vpn.as_ref(),
                            status.split_dns,
                            status.resolvers.as_deref(),
                        ),
                        middle[0],
                    );
                    frame.render_widget(proxy_paragraph(status.proxy.as_ref()), middle[1]);
                    frame.render_widget(wifi_paragraph(status.wifi_identity.as_ref(), status.wifi_radio.as_ref()), right[0]);
                    frame.render_widget(ip_stack_paragraph(status.ip_stack), right[1]);
                }
                Tab::Dns => {
                    let cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(rows[1]);
                    frame.render_widget(dns_list(status.resolvers.as_deref()), cols[0]);
                    frame.render_widget(resolution_list(status.resolution.as_deref()), cols[1]);
                }
                Tab::Reachability => {
                    let cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(34),
                            Constraint::Percentage(33),
                            Constraint::Percentage(33),
                        ])
                        .split(rows[1]);
                    frame.render_widget(
                        ping_list(
                            "Reachability (IPv4)",
                            status.reachability.as_deref().unwrap_or(&[]),
                        ),
                        cols[0],
                    );
                    frame.render_widget(
                        ping_list(
                            "Reachability (IPv6)",
                            status.reachability_v6.as_deref().unwrap_or(&[]),
                        ),
                        cols[1],
                    );
                    frame.render_widget(
                        connect_list(
                            "Reachability (domains, TCP:443)",
                            status.domain_reachability.as_deref().unwrap_or(&[]),
                        ),
                        cols[2],
                    );
                }
                Tab::Wifi => {
                    frame.render_widget(wifi_paragraph(status.wifi_identity.as_ref(), status.wifi_radio.as_ref()), rows[1]);
                }
            }
        }

        let age = last_updated
            .map(|t| format!("updated {} ago", format_age(t.elapsed())))
            .unwrap_or_default();
        let auto_state = if auto_refresh { "on, every 5s" } else { "off" };
        frame.render_widget(
            Paragraph::new(format!(
                "q: quit   r: refresh now   a: auto-refresh ({auto_state})   1-4: tabs   {age}   netcheck {}",
                netstatus::VERSION
            )),
            rows[2],
        );
    })?;
    Ok(())
}

/// Runs the interactive dashboard until the user quits.
pub fn run() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let auto_refresh = Arc::new(AtomicBool::new(false));
    let (rx, manual_tx) = spawn_workers(auto_refresh.clone());
    let mut status = netstatus::PartialStatus::default();
    let mut last_updated: Option<Instant> = None;
    let mut active_tab = Tab::Overview;

    let result = (|| -> io::Result<()> {
        loop {
            while let Ok(field) = rx.try_recv() {
                status.merge(field);
                last_updated = Some(Instant::now());
            }

            draw(
                &mut terminal,
                &status,
                active_tab,
                last_updated,
                auto_refresh.load(Ordering::Relaxed),
            )?;

            if event::poll(Duration::from_millis(200))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('r') => {
                        let _ = manual_tx.send(());
                    }
                    KeyCode::Char('a') => {
                        auto_refresh.fetch_xor(true, Ordering::Relaxed);
                    }
                    KeyCode::Char(c @ '1'..='4') => {
                        if let Some(tab) = Tab::from_digit(c as u8 - b'0') {
                            active_tab = tab;
                        }
                    }
                    _ => {}
                }
            }
        }
    })();

    restore_terminal(&mut terminal)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_col_pads_short_text_and_appends_separator() {
        assert_eq!(pad_col("en0", 8), "en0      ");
    }

    #[test]
    fn pad_col_still_separates_text_longer_than_width() {
        assert_eq!(
            pad_col("a-very-long-target-name", 8),
            "a-very-long-target-name "
        );
    }

    /// Cases shared with the SwiftUI app's FooterFormattingTests, so both
    /// UIs agree on the now/seconds/minutes/hours thresholds.
    #[test]
    fn format_age_matches_shared_fixture() {
        let fixture = include_str!("../../../testdata/age-format-cases.tsv");
        for line in fixture.lines().skip(1) {
            let mut cols = line.split('\t');
            let seconds: u64 = cols.next().unwrap().parse().unwrap();
            let magnitude: u64 = cols.next().unwrap().parse().unwrap();
            let unit = cols.next().unwrap();
            let expected = if unit == "now" {
                "now".to_string()
            } else {
                format!("{magnitude}{unit}")
            };
            assert_eq!(
                format_age(Duration::from_secs(seconds)),
                expected,
                "seconds={seconds}"
            );
        }
    }
}
