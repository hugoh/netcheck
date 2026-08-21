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

#[derive(Debug, Clone, Default)]
struct PartialStatus {
    interfaces: Option<Vec<netstatus::Interface>>,
    vpn: Option<netstatus::VpnStatus>,
    split_dns: Option<bool>,
    resolvers: Option<Vec<netstatus::Resolver>>,
    reachability: Option<Vec<netstatus::PingResult>>,
    reachability_v6: Option<Vec<netstatus::PingResult>>,
    resolution: Option<Vec<netstatus::ResolutionResult>>,
    domain_reachability: Option<Vec<netstatus::ConnectResult>>,
    proxy: Option<netstatus::ProxyConfig>,
    wifi: Option<netstatus::WifiStatus>,
    ip_stack: Option<netstatus::IpStack>,
}

impl PartialStatus {
    fn merge(&mut self, field: StatusField) {
        match field {
            StatusField::Interfaces(v) => self.interfaces = Some(v),
            StatusField::Vpn(v) => self.vpn = Some(v),
            StatusField::Resolvers(v) => self.resolvers = Some(v),
            StatusField::SplitDns(v) => self.split_dns = Some(v),
            StatusField::Reachability(v) => self.reachability = Some(v),
            StatusField::ReachabilityV6(v) => self.reachability_v6 = Some(v),
            StatusField::Resolution(v) => self.resolution = Some(v),
            StatusField::DomainReachability(v) => self.domain_reachability = Some(v),
            StatusField::Proxy(v) => self.proxy = Some(v),
            StatusField::Wifi(v) => self.wifi = Some(v),
            StatusField::IpStack(v) => self.ip_stack = Some(v),
        }
    }

    fn has_any(&self) -> bool {
        self.interfaces.is_some()
            || self.vpn.is_some()
            || self.resolvers.is_some()
            || self.split_dns.is_some()
            || self.reachability.is_some()
            || self.reachability_v6.is_some()
            || self.resolution.is_some()
            || self.domain_reachability.is_some()
            || self.proxy.is_some()
            || self.wifi.is_some()
            || self.ip_stack.is_some()
    }
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

fn interfaces_list(interfaces: Option<&[netstatus::Interface]>) -> List<'static> {
    let items: Vec<ListItem> = match interfaces {
        None => vec![ListItem::new("Collecting...")],
        Some(interfaces) => interfaces
            .iter()
            .filter(|i| !i.loopback)
            .map(|i| {
                let color = if i.up { Color::Green } else { Color::DarkGray };
                let addrs = if i.addresses.is_empty() {
                    "-".to_string()
                } else {
                    i.addresses.join(", ")
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<8}", i.name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(if i.up { "UP   " } else { "DOWN " }),
                    Span::raw(addrs),
                ]))
            })
            .collect(),
    };
    List::new(items).block(Block::default().borders(Borders::ALL).title("Interfaces"))
}

fn vpn_paragraph(
    vpn: Option<&netstatus::VpnStatus>,
    split_dns: Option<bool>,
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
                    Span::styled(format!("{label:<20}"), Style::default().fg(color)),
                    Span::raw(format!("{:<10} ", scope)),
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
                Span::styled(format!("{:<20}", p.target), Style::default().fg(color)),
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
                Span::styled(format!("{:<20}", c.target), Style::default().fg(color)),
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
                    Span::styled(format!("{:<20}", r.domain), Style::default().fg(color)),
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

fn draw(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    status: &PartialStatus,
    last_updated: Option<Instant>,
    auto_refresh: bool,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        if !status.has_any() {
            frame.render_widget(Paragraph::new("Collecting network status..."), rows[0]);
        } else {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .split(rows[0]);
            let left = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(cols[0]);
            let middle = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(cols[1]);
            let right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(cols[2]);

            frame.render_widget(interfaces_list(status.interfaces.as_deref()), left[0]);
            frame.render_widget(
                vpn_paragraph(status.vpn.as_ref(), status.split_dns),
                left[1],
            );
            frame.render_widget(dns_list(status.resolvers.as_deref()), middle[0]);
            frame.render_widget(resolution_list(status.resolution.as_deref()), middle[1]);
            frame.render_widget(
                ping_list(
                    "Reachability (IPs)",
                    status.reachability.as_deref().unwrap_or(&[]),
                ),
                right[0],
            );
            frame.render_widget(
                connect_list(
                    "Reachability (domains, TCP:443)",
                    status.domain_reachability.as_deref().unwrap_or(&[]),
                ),
                right[1],
            );
        }

        let age = last_updated
            .map(|t| format!("updated {}s ago", t.elapsed().as_secs()))
            .unwrap_or_default();
        let auto_state = if auto_refresh { "on, every 5s" } else { "off" };
        frame.render_widget(
            Paragraph::new(format!(
                "q: quit   r: refresh now   a: auto-refresh ({auto_state})   {age}"
            )),
            rows[1],
        );
    })?;
    Ok(())
}

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let auto_refresh = Arc::new(AtomicBool::new(false));
    let (rx, manual_tx) = spawn_workers(auto_refresh.clone());
    let mut status = PartialStatus::default();
    let mut last_updated: Option<Instant> = None;

    let result = (|| -> io::Result<()> {
        loop {
            while let Ok(field) = rx.try_recv() {
                status.merge(field);
                last_updated = Some(Instant::now());
            }

            draw(
                &mut terminal,
                &status,
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
                    _ => {}
                }
            }
        }
    })();

    restore_terminal(&mut terminal)?;
    result
}
