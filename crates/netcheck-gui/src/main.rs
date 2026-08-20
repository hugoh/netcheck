use eframe::egui;
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use netstatus::NetworkStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const GOOD: egui::Color32 = egui::Color32::from_rgb(80, 200, 120);
const BAD: egui::Color32 = egui::Color32::from_rgb(220, 80, 80);

/// Spawns the auto-refresh worker. Collects once immediately, then only
/// keeps collecting on a timer while `auto_refresh` is true (off by default).
fn spawn_auto_collector(auto_refresh: Arc<AtomicBool>) -> mpsc::Receiver<NetworkStatus> {
    let (tx, rx) = mpsc::channel();
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
    rx
}

struct App {
    rx: mpsc::Receiver<NetworkStatus>,
    status: Option<NetworkStatus>,
    last_updated: Option<Instant>,
    manual_refresh: mpsc::Sender<()>,
    manual_rx: mpsc::Receiver<NetworkStatus>,
    auto_refresh: Arc<AtomicBool>,
}

impl App {
    fn new() -> Self {
        let auto_refresh = Arc::new(AtomicBool::new(false));
        let rx = spawn_auto_collector(auto_refresh.clone());

        let (manual_tx, manual_trigger_rx) = mpsc::channel::<()>();
        let (manual_result_tx, manual_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for () in manual_trigger_rx {
                if manual_result_tx.send(netstatus::collect()).is_err() {
                    return;
                }
            }
        });

        Self {
            rx,
            status: None,
            last_updated: None,
            manual_refresh: manual_tx,
            manual_rx,
            auto_refresh,
        }
    }

    fn poll(&mut self) {
        while let Ok(status) = self.rx.try_recv() {
            self.status = Some(status);
            self.last_updated = Some(Instant::now());
        }
        while let Ok(status) = self.manual_rx.try_recv() {
            self.status = Some(status);
            self.last_updated = Some(Instant::now());
        }
    }
}

/// One bordered, titled, independently-scrollable panel. Content that
/// overflows the panel's cell scrolls inside it instead of spilling into
/// neighboring panels.
fn panel(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            ui.vertical(|ui| {
                ui.heading(title);
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(title)
                    .auto_shrink([false, false])
                    .show(ui, add_contents);
            });
        });
}

fn interfaces_table(ui: &mut egui::Ui, status: &NetworkStatus) {
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::auto().at_least(50.0))
        .column(Column::remainder())
        .body(|body| {
            let rows: Vec<_> = status.interfaces.iter().filter(|i| !i.loopback).collect();
            body.rows(18.0, rows.len(), |mut row| {
                let iface = rows[row.index()];
                row.col(|ui| {
                    ui.colored_label(if iface.up { GOOD } else { BAD }, &iface.name);
                });
                row.col(|ui| {
                    ui.label(if iface.addresses.is_empty() {
                        "-".to_string()
                    } else {
                        iface.addresses.join(", ")
                    });
                });
            });
        });
}

fn dns_resolvers_table(ui: &mut egui::Ui, status: &NetworkStatus) {
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::auto().at_least(90.0))
        .column(Column::auto().at_least(50.0))
        .column(Column::remainder())
        .body(|body| {
            let rows: Vec<_> = status.resolvers.iter().filter(|r| !r.nameservers.is_empty()).collect();
            body.rows(18.0, rows.len(), |mut row| {
                let r = rows[row.index()];
                let label = r
                    .domain
                    .clone()
                    .or_else(|| r.search_domains.first().cloned())
                    .unwrap_or_else(|| "*".to_string());
                row.col(|ui| {
                    ui.colored_label(if r.reachable { GOOD } else { BAD }, label);
                });
                row.col(|ui| {
                    ui.label(r.if_name.clone().unwrap_or_else(|| "any".into()));
                });
                row.col(|ui| {
                    ui.label(r.nameservers.join(", "));
                });
            });
        });
}

fn resolution_table(ui: &mut egui::Ui, status: &NetworkStatus) {
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::auto().at_least(110.0))
        .column(Column::auto().at_least(60.0))
        .column(Column::remainder())
        .body(|body| {
            body.rows(18.0, status.resolution.len(), |mut row| {
                let r = &status.resolution[row.index()];
                row.col(|ui| {
                    ui.colored_label(if r.resolved { GOOD } else { BAD }, &r.domain);
                });
                row.col(|ui| {
                    ui.label(r.duration_ms.map(|ms| format!("{ms:.1} ms")).unwrap_or_default());
                });
                row.col(|ui| {
                    ui.label(r.addresses.first().cloned().unwrap_or_default());
                });
            });
        });
}

fn ping_table(ui: &mut egui::Ui, targets: &[netstatus::PingResult]) {
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::auto().at_least(120.0))
        .column(Column::remainder())
        .body(|body| {
            body.rows(18.0, targets.len(), |mut row| {
                let p = &targets[row.index()];
                row.col(|ui| {
                    ui.colored_label(if p.reachable { GOOD } else { BAD }, &p.target);
                });
                row.col(|ui| {
                    ui.label(
                        p.rtt_ms
                            .map(|ms| format!("{ms:.1} ms"))
                            .unwrap_or_else(|| "timeout".to_string()),
                    );
                });
            });
        });
}

fn connect_table(ui: &mut egui::Ui, targets: &[netstatus::ConnectResult]) {
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::auto().at_least(120.0))
        .column(Column::remainder())
        .body(|body| {
            body.rows(18.0, targets.len(), |mut row| {
                let c = &targets[row.index()];
                row.col(|ui| {
                    ui.colored_label(if c.reachable { GOOD } else { BAD }, &c.target);
                });
                row.col(|ui| {
                    ui.label(
                        c.rtt_ms
                            .map(|ms| format!("{ms:.1} ms"))
                            .unwrap_or_else(|| "unreachable".to_string()),
                    );
                });
            });
        });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        ui.ctx().request_repaint_after(Duration::from_millis(300));

        if ui.ctx().input(|i| i.key_pressed(egui::Key::A)) {
            self.auto_refresh.fetch_xor(true, Ordering::Relaxed);
        }
        if ui.ctx().input(|i| i.key_pressed(egui::Key::R)) {
            let _ = self.manual_refresh.send(());
        }

        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("netcheck");
                if ui.button("Refresh now [r]").clicked() {
                    let _ = self.manual_refresh.send(());
                }
                let mut auto = self.auto_refresh.load(Ordering::Relaxed);
                if ui.checkbox(&mut auto, "Auto-refresh (5s, [a])").changed() {
                    self.auto_refresh.store(auto, Ordering::Relaxed);
                }
                if let Some(t) = self.last_updated {
                    ui.label(format!("updated {}s ago", t.elapsed().as_secs()));
                }
            });
        });

        let Some(status) = self.status.clone() else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("Collecting network status...");
            });
            return;
        };

        egui::CentralPanel::default().show(ui, |ui| {
            StripBuilder::new(ui)
                .size(Size::relative(0.32))
                .size(Size::relative(0.34))
                .size(Size::remainder())
                .horizontal(|mut strip| {
                    strip.cell(|ui| {
                        panel(ui, "Interfaces", |ui| interfaces_table(ui, &status));
                    });

                    strip.cell(|ui| {
                        StripBuilder::new(ui)
                            .size(Size::relative(0.22))
                            .size(Size::relative(0.28))
                            .size(Size::remainder())
                            .vertical(|mut strip| {
                                strip.cell(|ui| {
                                    panel(ui, "VPN / Tunnel", |ui| {
                                        let vpn = &status.vpn;
                                        ui.label(format!(
                                            "Primary interface: {}",
                                            vpn.primary_interface.clone().unwrap_or_else(|| "unknown".into())
                                        ));
                                        ui.colored_label(
                                            if vpn.connected { GOOD } else { BAD },
                                            format!("VPN connected: {}", vpn.connected),
                                        );
                                        ui.label(format!("Split tunnel: {}", vpn.split_tunnel));
                                        ui.label(format!("Split DNS: {}", status.split_dns));
                                        if !vpn.tunnels.is_empty() {
                                            ui.label(format!("Tunnels: {}", vpn.tunnels.join(", ")));
                                        }
                                    });
                                });
                                strip.cell(|ui| {
                                    panel(ui, "DNS resolvers", |ui| dns_resolvers_table(ui, &status));
                                });
                                strip.cell(|ui| {
                                    panel(ui, "DNS resolution", |ui| resolution_table(ui, &status));
                                });
                            });
                    });

                    strip.cell(|ui| {
                        StripBuilder::new(ui)
                            .size(Size::relative(0.5))
                            .size(Size::remainder())
                            .vertical(|mut strip| {
                                strip.cell(|ui| {
                                    panel(ui, "Reachability (IPs)", |ui| ping_table(ui, &status.reachability));
                                });
                                strip.cell(|ui| {
                                    panel(ui, "Reachability (domains, TCP:443)", |ui| {
                                        connect_table(ui, &status.domain_reachability)
                                    });
                                });
                            });
                    });
                });
        });
    }
}

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icon-1024.png");
    let image = image::load_from_memory(bytes).expect("bundled icon should decode").to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 600.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "netcheck",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}
