//! Network: HorizonX's servers behind one address (horizon-proxy, HorizonsX's Rust proxy). Without a network:
//! a short form to make one (a name, the servers in it, the lobby). With one: its state and address, Start, Stop
//! and Restart, the servers in it with their players, adding and taking out servers, its settings and its log.
use crate::app::{Ev, Field, HostApp};
use crate::pages::{self, panel_title};
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_launcher::network::{self, Network};
use horizon_theme::glass::{ink, radius};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// What the Network page knows.
#[derive(Default)]
pub struct NetState {
    pub network: Option<Network>,
    pub running: bool,
    pub memory: Option<u64>,
    pub counts: Option<(u64, Vec<(String, u64)>)>,
    pub log: Vec<String>,
    pub has_program: bool,
    pub loaded: bool,
    polling: bool,
    polled: Option<Instant>,
    /// The form: the servers ticked, and the lobby among them.
    pub pick: BTreeSet<PathBuf>,
    pub lobby: Option<PathBuf>,
    /// An action under way ("Starting the network").
    pub busy: Option<String>,
    /// Servers that joined or left the network while running: they need a restart.
    pub restart: BTreeSet<PathBuf>,
    /// The settings fields were filled from the network.
    filled: bool,
    /// The form's ticks were changed by hand (until then every server is ticked).
    touched: bool,
}

/// One look at the network, from a background thread.
pub struct NetSnapshot {
    network: Option<Network>,
    running: bool,
    memory: Option<u64>,
    counts: Option<(u64, Vec<(String, u64)>)>,
    log: Vec<String>,
    has_program: bool,
}

const PORT: u16 = 25577;

impl HostApp {
    /// Reads the network's state in the background (every 2 s while the page is open, or now with `force`).
    pub fn net_poll(&mut self, force: bool) {
        let n = &mut self.pages.net;
        if n.polling || (!force && n.polled.is_some_and(|t| t.elapsed() < Duration::from_secs(2))) {
            return;
        }
        n.polling = true;
        n.polled = Some(Instant::now());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let running = network::running();
            let snap = NetSnapshot {
                network: network::load(),
                running,
                memory: if running { network::memory() } else { None },
                counts: if running { network::counts() } else { None },
                log: network::log_tail(14),
                has_program: network::program().is_some(),
            };
            let _ = tx.send(Ev::Net(Box::new(snap)));
        });
    }

    pub fn net_event(&mut self, s: Box<NetSnapshot>) {
        let fill = !self.pages.net.filled && s.network.is_some();
        if let (true, Some(net)) = (fill, s.network.as_ref()) {
            let address = if net.address.is_empty() { network::lan_address() } else { net.address.clone() };
            self.set_text(Field::NetAddress, &address);
            self.set_text(Field::NetMotd, &net.motd);
        }
        let n = &mut self.pages.net;
        n.polling = false;
        n.loaded = true;
        n.filled |= fill;
        n.network = s.network;
        n.running = s.running;
        n.memory = s.memory;
        n.counts = s.counts;
        n.log = s.log;
        n.has_program = s.has_program;
    }

    /// Runs a network action in the background; its message comes back as a note.
    fn net_action(&mut self, doing: &str, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
        if self.pages.net.busy.is_some() {
            return;
        }
        self.pages.net.busy = Some(doing.to_owned());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Ev::NetDone(f()));
        });
    }

    pub fn net_done(&mut self, r: Result<String, String>) {
        self.pages.net.busy = None;
        self.pages.net.filled = false;
        self.note = Some((r.unwrap_or_else(|e| e), Instant::now()));
        self.net_poll(true);
    }

    /// Saves the settings fields (address, server list message) into the network.
    pub fn net_save_fields(&mut self) {
        let Some(mut net) = self.pages.net.network.clone() else { return };
        let address = self.text(Field::NetAddress).trim().to_owned();
        net.address = if address == network::lan_address() { String::new() } else { address };
        let motd = self.text(Field::NetMotd).trim().to_owned();
        if !motd.is_empty() {
            net.motd = motd;
        }
        let running = self.pages.net.running;
        self.net_action("Saving", move || {
            network::update(&net)?;
            Ok(if running { "Saved. Restart the network to use the new settings.".into() } else { "Saved.".into() })
        });
    }

    pub fn network_page(&mut self, cx: &mut Context<Self>) -> Div {
        self.net_poll(false);
        let has_network = self.pages.net.network.is_some();
        let controls = has_network.then(|| self.net_controls(cx));
        let head = pages::header("One address for all your servers", "Network", controls);
        if !self.pages.net.loaded {
            return pages::page("network-scroll", head, div());
        }
        let body = match self.pages.net.network.clone() {
            None => self.net_setup(cx),
            Some(net) => self.net_dashboard(&net, cx),
        };
        pages::page("network-scroll", head, body)
    }

    /// Start, or Restart and Stop, at the top right.
    fn net_controls(&mut self, cx: &mut Context<Self>) -> Div {
        let n = &self.pages.net;
        let busy = n.busy.clone();
        let mut row = div().flex().items_center().gap(px(8.));
        if let Some(b) = busy {
            return row.child(ui::chip(format!("{b}..."), Some(Tone::Busy)));
        }
        if n.running {
            row = row
                .child(ui::icon_button("net-restart", "refresh", "Restart", Kind::Secondary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.net_action("Restarting", || network::restart().map(|_| "The network restarted: players can join again.".into()));
                    cx.notify();
                })))
                .child(ui::icon_button("net-stop", "stop", "Stop", Kind::Secondary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.net_action("Stopping", || network::stop().map(|_| "The network stopped. The servers keep running.".into()));
                    cx.notify();
                })))
        } else {
            row = row.child(ui::icon_button("net-start", "play", "Start network", Kind::Primary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.net_action("Starting", || network::start().map(|_| "The network is running.".into()));
                cx.notify();
            })));
        }
        row
    }

    // ----- No network yet: make one --------------------------------------------------------------------

    fn net_setup(&mut self, cx: &mut Context<Self>) -> Div {
        let has_program = self.pages.net.has_program;
        let name = self.field(Field::NetName, "My network", cx);
        if !self.pages.net.touched {
            self.pages.net.pick = self.list.iter().map(|e| e.dir.clone()).collect();
        }
        let picked = self.pages.net.pick.clone();
        // The lobby: the one picked, else a server called Lobby or Hub, else the first in the list.
        let in_order: Vec<(String, PathBuf)> = self.list.iter().filter(|e| picked.contains(&e.dir)).map(|e| (e.name.to_lowercase(), e.dir.clone())).collect();
        let lobby = self
            .pages
            .net
            .lobby
            .clone()
            .filter(|l| picked.contains(l))
            .or_else(|| in_order.iter().find(|(n, _)| n.contains("lobby") || n.contains("hub")).map(|(_, d)| d.clone()))
            .or_else(|| in_order.first().map(|(_, d)| d.clone()));
        let mut rows = div().flex().flex_col().mt(px(6.));
        if self.list.is_empty() {
            rows = rows.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).py(px(8.)).child("Make a server first: New server, at the bottom left."));
        }
        let n = self.list.len();
        for (i, e) in self.list.clone().into_iter().enumerate() {
            let on = picked.contains(&e.dir);
            let is_lobby = lobby.as_ref() == Some(&e.dir);
            let st = self.status_of(&e.dir);
            let d = e.dir.clone();
            let tick = checkbox(SharedString::from(format!("net-pick-{i}")), on).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.pages.net.touched = true;
                let p = &mut this.pages.net.pick;
                if !p.remove(&d) {
                    p.insert(d.clone());
                }
                cx.notify();
            }));
            let d2 = e.dir.clone();
            let lobby_ctl = if !on {
                div().into_any_element()
            } else if is_lobby {
                ui::chip("Lobby", Some(Tone::Live)).into_any_element()
            } else {
                ui::sized_button(SharedString::from(format!("net-lobby-{i}")), "Make lobby", Kind::Ghost, 28., 12.)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pages.net.lobby = Some(d2.clone());
                        cx.notify();
                    }))
                    .into_any_element()
            };
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(10.))
                    .px(px(4.))
                    .when(i + 1 < n, |d| d.border_b_1().border_color(white(0.06)))
                    .child(tick)
                    .child(ui::dot(crate::frame::server_tone(&st)))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(ui::text(14., FontWeight::SEMIBOLD, if on { ink::FULL } else { ink::BODY }).child(e.name.clone())).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(if st.running() { "Running" } else { "Stopped" })))
                    .child(div().flex_none().w(px(110.)).flex().justify_end().child(lobby_ctl)),
            );
        }
        let chosen: Vec<(String, PathBuf)> = self.list.iter().filter(|e| picked.contains(&e.dir)).map(|e| (e.name.clone(), e.dir.clone())).collect();
        let lobby_idx = chosen.iter().position(|(_, d)| Some(d) == lobby.as_ref()).unwrap_or(0);
        let ready = has_program && !chosen.is_empty();
        let running_now: Vec<PathBuf> = chosen.iter().filter(|(_, d)| self.status_of(d).running()).map(|(_, d)| d.clone()).collect();
        let create = ui::button("net-create", "Make the network", if ready { Kind::Primary } else { Kind::Secondary }).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !ready {
                this.note = Some((if this.pages.net.has_program { "Tick at least one server.".into() } else { "The network proxy isn't installed with this copy of HorizonX.".into() }, Instant::now()));
                cx.notify();
                return;
            }
            let name = match this.text(Field::NetName).trim() {
                "" => "My network".to_owned(),
                n => n.to_owned(),
            };
            let chosen = chosen.clone();
            this.pages.net.restart.extend(running_now.iter().cloned());
            this.net_action("Making the network", move || {
                network::create(&name, PORT, &chosen, lobby_idx)?;
                network::start()?;
                Ok("The network is running. Restart its servers so they take players from it.".into())
            });
            cx.notify();
        }));
        let how = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(step("1", "Players join one address", "Your PC's address on port 25577 (or your playit address). They land on the lobby."))
            .child(step("2", "They move with /server", "/server lists every server with its players; /server survival goes there; /hub comes back."))
            .child(step("3", "The network checks accounts", "Each server in it lets in only players coming through the network, so nobody can skip the checks."));
        let mut form = ui::panel()
            .flex_1()
            .min_w_0()
            .px(px(22.))
            .py(px(20.))
            .child(panel_title("Make a network", None))
            .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).mt(px(2.)).child("Join your servers behind one address, like a Velocity network, with HorizonsX's own proxy."))
            .child(div().mt(px(16.)).child(ui::text(12.5, FontWeight::MEDIUM, ink::BODY).pb(px(6.)).child("Name")).child(name))
            .child(div().flex().items_center().justify_between().mt(px(18.)).child(ui::text(12.5, FontWeight::MEDIUM, ink::BODY).child("Servers in it")).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(format!("{} picked", chosen_count(&picked)))))
            .child(rows);
        if !has_program {
            form = form.child(warn_bar("The network proxy (horizon-proxy) isn't beside HorizonX. Reinstall HorizonX to get it."));
        }
        form = form.child(div().flex().justify_end().mt(px(18.)).child(create));
        let side = ui::panel().px(px(20.)).py(px(20.)).child(ui::heading(18.).child("How it works")).child(div().mt(px(12.)).child(how));
        if ui::wide() {
            div().flex().items_start().gap(px(16.)).child(form).child(side.flex_none().w(px(380.)))
        } else {
            div().flex().flex_col().gap(px(16.)).child(form).child(side)
        }
    }

    // ----- A network: its state, servers, settings and log ---------------------------------------------

    fn net_dashboard(&mut self, net: &Network, cx: &mut Context<Self>) -> Div {
        let n = &self.pages.net;
        let (running, memory, log) = (n.running, n.memory, n.log.clone());
        let counts = n.counts.clone();
        let address = format!("{}:{}", if net.address.is_empty() { network::lan_address() } else { net.address.clone() }, net.port);
        let total = counts.as_ref().map(|c| c.0).unwrap_or(0);
        let copy_text = address.clone();
        let copied = self.copied.as_ref().is_some_and(|(t, _)| t == &address);
        // The state and the address.
        let state = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(ui::dot(if running { Tone::Live } else { Tone::Off }))
            .child(ui::heading(20.).child(if running { "Running" } else { "Stopped" }))
            .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(net.name.clone()));
        let stats = div()
            .flex()
            .gap(px(28.))
            .mt(px(16.))
            .child(ui::stat(if running { total.to_string() } else { "-".into() }, "players on the network"))
            .child(ui::stat(net.members.len().to_string(), if net.members.len() == 1 { "server" } else { "servers" }))
            .child(ui::stat(memory.map(ui::bytes).unwrap_or_else(|| "-".into()), "proxy memory"));
        let address_row = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .mt(px(16.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(radius::CONTROL))
            .bg(white(0.05))
            .child(ui::icon("link-01", 15., white(ink::QUIET)))
            .child(div().flex().flex_col().flex_1().min_w_0().child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child("Players join at")).child(ui::mono(15., ink::FULL).child(address.clone())))
            .child(ui::sized_button("net-copy", if copied { "Copied" } else { "Copy" }, Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.copy(copy_text.clone(), cx);
                cx.notify();
            })));
        let mut top = ui::panel().px(px(22.)).py(px(20.)).child(state).child(stats).child(address_row);
        // Servers that joined or left while running.
        let waiting: Vec<PathBuf> = self.pages.net.restart.iter().filter(|d| self.status_of(d).running()).cloned().collect();
        if !waiting.is_empty() {
            let names = waiting.iter().map(|d| self.name_of(d).to_string()).collect::<Vec<_>>().join(", ");
            let w2 = waiting.clone();
            top = top.child(
                warn_bar(&format!("{names} need a restart to take players from the network."))
                    .child(ui::sized_button("net-restart-servers", "Restart them", Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        for d in &w2 {
                            this.restart(d.clone(), cx);
                        }
                        this.pages.net.restart.clear();
                        cx.notify();
                    }))),
            );
        }
        if !self.pages.net.has_program {
            top = top.child(warn_bar("The network proxy (horizon-proxy) isn't beside HorizonX. Reinstall HorizonX to get it."));
        }
        let members = self.net_members(net, &counts, running, cx);
        let settings = self.net_settings(net, cx);
        let mut log_panel = ui::panel().px(px(22.)).py(px(18.)).child(panel_title("Proxy log", None));
        let mut lines = div().flex().flex_col().gap(px(2.)).mt(px(10.));
        if log.is_empty() {
            lines = lines.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).child("Nothing yet: the proxy writes here when it runs."));
        }
        for l in log {
            lines = lines.child(ui::mono(11.5, ink::BODY).truncate().child(l));
        }
        log_panel = log_panel.child(lines);
        let left = div().flex().flex_col().gap(px(16.)).flex_1().min_w_0().child(top).child(members);
        let right = div().flex().flex_col().gap(px(16.)).child(settings).child(log_panel);
        if ui::wide() {
            div().flex().items_start().gap(px(16.)).child(left).child(right.flex_none().w(px(420.)))
        } else {
            div().flex().flex_col().gap(px(16.)).child(left).child(right)
        }
    }

    fn net_members(&mut self, net: &Network, counts: &Option<(u64, Vec<(String, u64)>)>, running: bool, cx: &mut Context<Self>) -> Div {
        let mut panel = ui::panel().px(px(22.)).py(px(20.)).child(panel_title("Servers in the network", Some(net.members.len())));
        panel = panel.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).mt(px(2.)).child("Players land on the lobby and move with /server. Each server here checks nobody itself: only players coming through the network get in."));
        let n = net.members.len();
        for (i, m) in net.members.iter().enumerate() {
            let known = self.list.iter().any(|e| e.dir == m.dir);
            let name = if known { self.name_of(&m.dir).to_string() } else { m.name.clone() };
            let st = self.status_of(&m.dir);
            let players = counts.as_ref().and_then(|c| c.1.iter().find(|(k, _)| k == &m.name).map(|(_, v)| *v));
            let is_lobby = m.name == net.lobby;
            let lobby_ctl = if is_lobby {
                ui::chip("Lobby", Some(Tone::Live)).into_any_element()
            } else {
                let (net2, pname) = (net.clone(), m.name.clone());
                ui::sized_button(SharedString::from(format!("net-mk-lobby-{i}")), "Make lobby", Kind::Ghost, 28., 12.)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let mut net3 = net2.clone();
                        net3.lobby = pname.clone();
                        this.net_action("Saving", move || {
                            network::update(&net3)?;
                            Ok(format!("{} is the lobby now. Restart the network to use it.", net3.lobby))
                        });
                        cx.notify();
                    }))
                    .into_any_element()
            };
            let key = format!("net-rm:{}", m.dir.display());
            let confirming = self.pages.confirm.as_deref() == Some(key.as_str());
            let (d, k2) = (m.dir.clone(), key.clone());
            let remove = ui::sized_button(SharedString::from(format!("net-rm-{i}")), if confirming { "Take out?" } else { "Take out" }, if confirming { Kind::Danger } else { Kind::Ghost }, 28., 12.)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if this.pages.confirm.as_deref() != Some(k2.as_str()) {
                        this.pages.confirm = Some(k2.clone());
                    } else {
                        this.pages.confirm = None;
                        let d2 = d.clone();
                        if this.status_of(&d).running() {
                            this.pages.net.restart.insert(d.clone());
                        }
                        this.net_action("Taking it out", move || {
                            network::remove(&d2)?;
                            Ok("Taken out: it checks accounts itself again. Restart it and the network to finish.".into())
                        });
                    }
                    cx.notify();
                }));
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(11.))
                    .px(px(4.))
                    .when(i + 1 < n, |d| d.border_b_1().border_color(white(0.06)))
                    .child(ui::dot(if known { crate::frame::server_tone(&st) } else { Tone::Bad }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(ui::text(14., FontWeight::SEMIBOLD, ink::FULL).child(name))
                            .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(if !known {
                                "This server is gone: take it out".to_string()
                            } else {
                                format!("/server {} · port {} · {}", m.name, m.port, if st.running() { "running" } else { "stopped" })
                            })),
                    )
                    .child(div().flex_none().w(px(90.)).child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(match (running, players) {
                        (true, Some(p)) => format!("{p} online"),
                        _ => "-".into(),
                    })))
                    .child(div().flex_none().w(px(104.)).flex().justify_end().child(lobby_ctl))
                    .child(div().flex_none().w(px(92.)).flex().justify_end().child(remove)),
            );
        }
        // Servers that could join.
        let outside: Vec<(String, PathBuf)> = self.list.iter().filter(|e| !net.members.iter().any(|m| m.dir == e.dir)).map(|e| (e.name.clone(), e.dir.clone())).collect();
        if !outside.is_empty() {
            panel = panel.child(div().flex().items_center().gap(px(10.)).pt(px(16.)).pb(px(4.)).child(ui::mono(10.5, ink::QUIET).child("ADD A SERVER")).child(div().flex_1().h(px(1.)).bg(white(0.07))));
            for (i, (name, dir)) in outside.into_iter().enumerate() {
                let st = self.status_of(&dir);
                let (nm, d) = (name.clone(), dir.clone());
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .py(px(8.))
                        .px(px(4.))
                        .child(ui::dot(crate::frame::server_tone(&st)))
                        .child(div().flex_1().min_w_0().child(ui::text(13.5, FontWeight::MEDIUM, ink::BODY).child(name)))
                        .child(ui::sized_button(SharedString::from(format!("net-add-{i}")), "Add", Kind::Secondary, 28., 12.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if this.status_of(&d).running() {
                                this.pages.net.restart.insert(d.clone());
                            }
                            let (nm, d) = (nm.clone(), d.clone());
                            this.net_action("Adding the server", move || {
                                network::add(&nm, &d)?;
                                Ok(format!("{nm} is in the network. Restart it and the network to finish."))
                            });
                            cx.notify();
                        }))),
                );
            }
        }
        panel
    }

    fn net_settings(&mut self, net: &Network, cx: &mut Context<Self>) -> Div {
        let address = self.field(Field::NetAddress, "This PC's address", cx);
        let motd = self.field(Field::NetMotd, "A HorizonsX network", cx);
        let online = net.online_mode;
        let net2 = net.clone();
        let accounts = ui::toggle("net-online", online).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            let mut n = net2.clone();
            n.online_mode = !online;
            this.net_action("Saving", move || {
                network::update(&n)?;
                Ok("Saved. Restart the network to use it.".into())
            });
            cx.notify();
        }));
        let save = ui::sized_button("net-save", "Save", Kind::Secondary, 32., 12.5).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.net_save_fields();
            cx.notify();
        }));
        let confirming = self.pages.confirm.as_deref() == Some("net-delete");
        let delete = ui::sized_button("net-delete", if confirming { "Delete the network?" } else { "Delete network" }, if confirming { Kind::Danger } else { Kind::Ghost }, 30., 12.5).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            if this.pages.confirm.as_deref() != Some("net-delete") {
                this.pages.confirm = Some("net-delete".into());
            } else {
                this.pages.confirm = None;
                let running: Vec<PathBuf> = this.pages.net.network.iter().flat_map(|n| n.members.iter().map(|m| m.dir.clone())).filter(|d| this.status_of(d).running()).collect();
                this.pages.net.restart.extend(running);
                this.net_action("Deleting the network", || {
                    network::delete()?;
                    Ok("The network is gone. Its servers check accounts themselves again after a restart.".into())
                });
            }
            cx.notify();
        }));
        ui::panel()
            .px(px(22.))
            .py(px(20.))
            .child(panel_title("Settings", None))
            .child(div().mt(px(12.)).child(ui::text(12.5, FontWeight::MEDIUM, ink::BODY).child("Address players join with")).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).pb(px(6.)).child("This PC's address by default. Put your playit address or domain here so /server sends players back to it.")).child(address))
            .child(div().mt(px(14.)).child(ui::text(12.5, FontWeight::MEDIUM, ink::BODY).pb(px(6.)).child("Server list message")).child(motd))
            .child(div().flex().justify_end().mt(px(10.)).child(save))
            .child(div().mt(px(8.)).child(ui::setting_row("Check Minecraft accounts", "Off lets anyone join under any name: only for a LAN or testing.", accounts)))
            .child(div().flex().items_center().justify_between().mt(px(14.)).pt(px(14.)).border_t_1().border_color(white(0.07)).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Takes every server out; the servers stay.")).child(delete))
    }
}

/// A small tick box.
fn checkbox(id: SharedString, on: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded(px(6.))
        .cursor_pointer()
        .bg(if on { ui::color(ui::ACCENT) } else { white(0.06) })
        .border_1()
        .border_color(if on { ui::color(ui::ACCENT) } else { white(0.18) })
        .hover(|s| s.border_color(white(0.4)))
        .when(on, |d| d.child(ui::icon("tick-02", 14., white(1.))))
}

fn chosen_count(p: &BTreeSet<PathBuf>) -> usize {
    p.len()
}

/// A numbered line of "How it works".
fn step(n: &'static str, title: &'static str, note: &'static str) -> Div {
    div()
        .flex()
        .gap(px(12.))
        .py(px(6.))
        .child(div().flex().flex_none().items_center().justify_center().size(px(26.)).rounded(px(13.)).bg(white(0.08)).child(ui::text(12.5, FontWeight::SEMIBOLD, ink::FULL).child(n)))
        .child(div().flex().flex_col().flex_1().min_w_0().child(ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).child(title)).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(note)))
}

/// A note that wants attention, with room for a button after it.
fn warn_bar(text: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .mt(px(14.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(radius::CONTROL))
        .bg(ui::color(ui::WARN).opacity(0.09))
        .child(ui::icon("information-circle", 15., ui::color(ui::WARN)))
        .child(div().flex_1().min_w_0().child(ui::text(12.5, FontWeight::NORMAL, ink::FULL).child(text.to_owned())))
}
