use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use crossterm::{ExecutableCommand, execute};
use netstatus::NetworkStatus;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// Spawns the background workers. Returns a receiver fed by both an initial
/// one-shot collection, a periodic auto-refresh (gated by `auto_refresh`,
/// off by default), and manual refreshes triggered via the returned sender.
fn spawn_workers(auto_refresh: Arc<AtomicBool>) -> (mpsc::Receiver<NetworkStatus>, mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel();
    let (manual_tx, manual_rx) = mpsc::channel::<()>();

    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for () in manual_rx {
                if tx.send(netstatus::collect()).is_err() {
                    return;
                }
            }
        });
    }

    std::thread::spawn(move || {
        if tx.send(netstatus::collect()).is_err() {
            return;
        }
        loop {
            if auto_refresh.load(Ordering::Relaxed) {
                std::thread::sleep(REFRESH_INTERVAL);
                if auto_refresh.load(Ordering::Relaxed) && tx.send(netstatus::collect()).is_err() {
                    return;
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

fn interfaces_list(status: &NetworkStatus) -> List<'static> {
    let items: Vec<ListItem> = status
        .interfaces
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
                Span::styled(format!("{:<8}", i.name), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw(if i.up { "UP   " } else { "DOWN " }),
                Span::raw(addrs),
            ]))
        })
        .collect();
    List::new(items).block(Block::default().borders(Borders::ALL).title("Interfaces"))
}

fn vpn_paragraph(status: &NetworkStatus) -> Paragraph<'static> {
    let vpn = &status.vpn;
    let mut lines = vec![
        Line::from(format!(
            "Primary: {}",
            vpn.primary_interface.clone().unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!("VPN connected: {}", vpn.connected)),
        Line::from(format!("Split tunnel: {}", vpn.split_tunnel)),
        Line::from(format!("Split DNS: {}", status.split_dns)),
    ];
    if !vpn.tunnels.is_empty() {
        lines.push(Line::from(format!("Tunnels: {}", vpn.tunnels.join(", "))));
    }
    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("VPN / Tunnel"))
}

fn dns_list(status: &NetworkStatus) -> List<'static> {
    let items: Vec<ListItem> = status
        .resolvers
        .iter()
        .filter(|r| !r.nameservers.is_empty())
        .map(|r| {
            let label = r
                .domain
                .clone()
                .or_else(|| r.search_domains.first().cloned())
                .unwrap_or_else(|| "*".to_string());
            let scope = r.if_name.clone().unwrap_or_else(|| "any".to_string());
            let color = if r.reachable { Color::Green } else { Color::Red };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{label:<20}"), Style::default().fg(color)),
                Span::raw(format!("{:<10} ", scope)),
                Span::raw(r.nameservers.join(", ")),
            ]))
        })
        .collect();
    List::new(items).block(Block::default().borders(Borders::ALL).title("DNS resolvers"))
}

fn ping_list(title: &'static str, results: &[netstatus::PingResult]) -> List<'static> {
    let items: Vec<ListItem> = results
        .iter()
        .map(|p| {
            let color = if p.reachable { Color::Green } else { Color::Red };
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
            let color = if c.reachable { Color::Green } else { Color::Red };
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

fn resolution_list(status: &NetworkStatus) -> List<'static> {
    let items: Vec<ListItem> = status
        .resolution
        .iter()
        .map(|r| {
            let color = if r.resolved { Color::Green } else { Color::Red };
            let detail = if r.resolved {
                format!(
                    "{}  ({})",
                    r.duration_ms.map(|ms| format!("{ms:.1} ms")).unwrap_or_default(),
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
        .collect();
    List::new(items).block(Block::default().borders(Borders::ALL).title("DNS resolution"))
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    status: Option<&NetworkStatus>,
    last_updated: Option<Instant>,
    auto_refresh: bool,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        match status {
            None => {
                frame.render_widget(Paragraph::new("Collecting network status..."), rows[0]);
            }
            Some(status) => {
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

                frame.render_widget(interfaces_list(status), left[0]);
                frame.render_widget(vpn_paragraph(status), left[1]);
                frame.render_widget(dns_list(status), middle[0]);
                frame.render_widget(resolution_list(status), middle[1]);
                frame.render_widget(ping_list("Reachability (IPs)", &status.reachability), right[0]);
                frame.render_widget(
                    connect_list("Reachability (domains, TCP:443)", &status.domain_reachability),
                    right[1],
                );
            }
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
    let mut status: Option<NetworkStatus> = None;
    let mut last_updated: Option<Instant> = None;

    let result = (|| -> io::Result<()> {
        loop {
            while let Ok(new_status) = rx.try_recv() {
                status = Some(new_status);
                last_updated = Some(Instant::now());
            }

            draw(
                &mut terminal,
                status.as_ref(),
                last_updated,
                auto_refresh.load(Ordering::Relaxed),
            )?;

            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
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
            }
        }
    })();

    restore_terminal(&mut terminal)?;
    result
}
