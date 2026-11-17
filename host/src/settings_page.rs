//! Settings: HorizonX's own, under centred tabs. General (open with Windows, start servers that were
//! running, warn before scheduled restarts), Servers (where they live, the server software), Network (each
//! server's public address), Java plugins (which servers run jars), Updates and About. Only settings that do
//! something are here.
use crate::app::HostApp;
use crate::pages::{self, SettingsTab};
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_launcher::{host, servers, stats};
use horizon_theme::glass::{ink, radius};

fn row(title: impl Into<SharedString>, note: impl Into<SharedString>, control: impl IntoElement) -> Div {
    let note: SharedString = note.into();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .py(px(12.))
        .border_b_1()
        .border_color(white(0.06))
        .child(div().flex().flex_col().flex_1().min_w_0().child(ui::text(13.5, FontWeight::MEDIUM, ink::FULL).child(title.into())).when(!note.is_empty(), |d| d.child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(note))))
        .child(control)
}

fn group(title: &'static str) -> Div {
    div().flex().flex_col().px(px(22.)).pt(px(18.)).pb(px(6.)).child(ui::heading(16.).mb(px(4.)).child(title))
}

impl HostApp {
    pub fn settings_page(&mut self, cx: &mut Context<Self>) -> Div {
        let tab = self.pages.settings_tab;
        let tabs = ui::tabs(
            "settings-tab",
            &[
                (SettingsTab::General, "General"),
                (SettingsTab::Servers, "Servers"),
                (SettingsTab::Network, "Network"),
                (SettingsTab::Java, "Java plugins"),
                (SettingsTab::Ai, "AI assistants"),
                (SettingsTab::Updates, "Updates"),
                (SettingsTab::About, "About"),
            ],
            tab,
            {
                let e = cx.entity();
                move |t, _, cx| {
                    e.update(cx, |this, cx| {
                        this.pages.settings_tab = t;
                        cx.notify();
                    })
                }
            },
        );
        let head = pages::header("HorizonX on this PC", "Settings", None);
        let body = match tab {
            SettingsTab::General => self.settings_general(cx),
            SettingsTab::Servers => self.settings_servers(),
            SettingsTab::Network => self.settings_network(cx),
            SettingsTab::Java => self.settings_java(cx),
            SettingsTab::Ai => self.settings_ai(cx),
            SettingsTab::Updates => self.settings_updates(),
            SettingsTab::About => self.settings_about(),
        };
        let panel = ui::plate(radius::PANEL).relative().flex().flex_col().pb(px(12.)).child(body);
        pages::page("settings-scroll", head, div().flex().flex_col().gap(px(14.)).child(div().flex().justify_center().child(tabs)).child(panel))
    }

    fn settings_general(&mut self, cx: &mut Context<Self>) -> Div {
        let with_windows = stats::opens_with_windows();
        let restore = self.settings.restore_servers;
        let warn = self.settings.warn_before_restart;
        let t1 = ui::toggle("set-windows", with_windows).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if let Err(e) = stats::set_opens_with_windows(!with_windows) {
                this.note = Some((e, std::time::Instant::now()));
            }
            cx.notify();
        }));
        let t2 = ui::toggle("set-restore", restore).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.settings.restore_servers = !restore;
            let _ = stats::save_settings(&this.settings);
            cx.notify();
        }));
        let t3 = ui::toggle("set-warn", warn).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.settings.warn_before_restart = !warn;
            let _ = stats::save_settings(&this.settings);
            cx.notify();
        }));
        group("General")
            .child(row(if cfg!(windows) { "Open HorizonX when Windows starts" } else { "Open HorizonX when you sign in" }, "Your servers come back after a reboot, with the setting below.", t1))
            .child(row("Start servers that were running when HorizonX opens", "The ones running when it last closed.", t2))
            .child(row("Warn players before a scheduled restart", "A chat message 5 minutes ahead.", t3))
    }

    fn settings_servers(&mut self) -> Div {
        let folder = horizon_launcher::backend::data_dir().join("servers");
        let f2 = folder.clone();
        let open = ui::sized_button("set-open-servers", "Open", Kind::Secondary, 32., 12.5).on_click(move |_, _, cx| {
            let _ = std::fs::create_dir_all(&f2);
            cx.open_with_system(&f2);
        });
        let free = stats::disk_free(&folder).map(|b| format!("{} free on this drive", ui::bytes(b))).unwrap_or_default();
        let engine = servers::runtime_release().unwrap_or_else(|| "not set up yet".into());
        let horizonsx = self.list.iter().filter(|e| host::runs_horizonsx(&e.dir)).count();
        group("Servers")
            .child(row("Where new servers are kept", folder.display().to_string(), open))
            .child(row("Room on that drive", free, div()))
            .child(row("Server software", format!("HorizonsX on {horizonsx} of {} servers · runtime {engine}", self.list.len()), ui::chip("HorizonsX", Some(Tone::Live))))
    }

    fn settings_network(&mut self, cx: &mut Context<Self>) -> Div {
        let mut g = group("Network").child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).pb(px(6.)).child("Each server gets a public address from playit.gg, so friends join from anywhere with no port forwarding."));
        if self.list.is_empty() {
            return g.child(row("No servers yet", "", div()));
        }
        for (i, e) in self.list.clone().into_iter().enumerate() {
            let p = self.playit.get(&e.dir).cloned().or_else(|| host::playit(&e.dir));
            let (note, control): (String, gpui::AnyElement) = match p {
                Some(p) if p.online() => {
                    let address = p.address.clone().unwrap_or_default();
                    let copied = self.copied(&address);
                    let a2 = address.clone();
                    (
                        address,
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(ui::chip("Connected", Some(Tone::Live)))
                            .child(ui::sized_button(SharedString::from(format!("net-copy-{i}")), if copied { "Copied" } else { "Copy" }, Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(a2.clone(), cx))))
                            .into_any_element(),
                    )
                }
                Some(p) if p.waiting_for_claim() => {
                    let url = p.claim_url.clone().unwrap_or_default();
                    ("Waiting for you to link it to your playit.gg account".into(), ui::sized_button(SharedString::from(format!("net-claim-{i}")), "Link", Kind::Primary, 30., 12.5).on_click(move |_, _, cx| cx.open_url(&url)).into_any_element())
                }
                Some(p) => (if p.state_text.is_empty() { "Starting".into() } else { crate::wizard::cap(&p.state_text) }, ui::chip("Connecting", Some(Tone::Busy)).into_any_element()),
                None => ("On this network only: playit starts with the server".into(), ui::chip("Off", Some(Tone::Off)).into_any_element()),
            };
            g = g.child(row(e.name.clone(), note, control));
        }
        g
    }

    fn settings_java(&mut self, cx: &mut Context<Self>) -> Div {
        let mut g = group("Java plugins").child(
            ui::text(12.5, FontWeight::NORMAL, ink::QUIET)
                .pb(px(6.))
                .child("Bukkit, Spigot and Paper plugin jars run unchanged on HorizonsX's Java layer. A server with the layer runs the jars in its plugins folder."),
        );
        for e in self.list.clone() {
            if !self.java.contains_key(&e.dir) {
                self.java.insert(e.dir.clone(), (vec![], horizon_launcher::host::java_layer_on(&e.dir)));
                self.load_details(&e.dir);
            }
            let (jars, layer) = self.java.get(&e.dir).cloned().unwrap_or_default();
            let d = e.dir.clone();
            let manage = ui::sized_button(SharedString::from(format!("java-{}", e.name)), "Plugins", Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.pages.plugins_server = Some(d.clone());
                this.open_page(crate::frame::Page::Plugins, cx);
            }));
            g = g.child(row(
                e.name.clone(),
                if layer { format!("Java layer on · {} jar{}", jars.len(), if jars.len() == 1 { "" } else { "s" }) } else { "Java layer off".to_string() },
                div().flex().items_center().gap(px(8.)).child(ui::chip(if layer { "On" } else { "Off" }, Some(if layer { Tone::Live } else { Tone::Off }))).child(manage),
            ));
        }
        g
    }

    fn settings_ai(&mut self, cx: &mut Context<Self>) -> Div {
        use crate::mcp::Client;
        let connected = self.pages.ai.get_or_insert_with(|| Client::ALL.iter().map(|c| c.connected()).collect()).clone();
        let mut g = group("AI assistants").child(
            ui::text(12.5, FontWeight::NORMAL, ink::QUIET).pb(px(6.)).child(
                "Let an AI assistant run HorizonX for you: make and start servers, add plugins from Modrinth, change settings and configs, moderate players, back up, run the network and find problems. It does what you ask, and asks you before deleting anything.",
            ),
        );
        for (i, client) in Client::ALL.into_iter().enumerate() {
            let on = connected.get(i).copied().unwrap_or(false);
            let control: gpui::AnyElement = if on {
                ui::chip("Connected", Some(Tone::Live)).into_any_element()
            } else {
                ui::sized_button(SharedString::from(format!("ai-connect-{i}")), "Connect", Kind::Primary, 30., 12.5)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let note = match client.connect() {
                            Ok(m) => m,
                            Err(e) => format!("{}: {e}", client.label()),
                        };
                        this.pages.ai = None;
                        this.note = Some((note, std::time::Instant::now()));
                        cx.notify();
                    }))
                    .into_any_element()
            };
            let note = match client {
                Client::ClaudeCode => "Adds HorizonX to Claude Code for every project (claude mcp add).",
                Client::ClaudeDesktop => "Adds HorizonX to Claude Desktop's settings; restart it after.",
                Client::Cursor => "Adds HorizonX to Cursor's MCP settings.",
            };
            g = g.child(row(client.label(), if on { "HorizonX is in its tools." } else { note }, control));
        }
        let config = crate::mcp::client_config();
        let copied = self.copied(&config);
        let (cmd, _) = crate::mcp::launch_command();
        g.child(row(
            "Any other MCP client",
            format!("Runs \"{cmd}\" mcp. Copy the setup and paste it into the client's MCP settings."),
            ui::sized_button("ai-copy", if copied { "Copied" } else { "Copy setup" }, Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(config.clone(), cx))),
        ))
        .child(row(
            "What it can do",
            format!("{} tools: servers, settings and files, plugins (HorizonX's and Modrinth's), players, backups, schedules, the network and diagnostics.", crate::mcp::tools().len()),
            div(),
        ))
    }

    fn settings_updates(&mut self) -> Div {
        let engine = servers::runtime_release().unwrap_or_else(|| "not set up yet".into());
        let available = self.status.values().find_map(|s| s.update_available.clone());
        let busy = self.status.values().find_map(|s| s.update_busy.clone());
        let (note, chip) = match (busy, available) {
            (Some(b), _) => (b, ui::chip("Updating", Some(Tone::Busy))),
            (None, Some(v)) => (format!("{v} is ready: each server picks it up at its next restart"), ui::chip("Update ready", Some(Tone::Busy))),
            (None, None) => ("Up to date".into(), ui::chip("Up to date", Some(Tone::Live))),
        };
        group("Updates")
            .child(row(format!("HorizonsX runtime {engine}"), note, chip))
            .child({
                let state = crate::update::state();
                let note = if state.is_empty() { "Updates itself from GitHub: a new version downloads, installs and reopens on its own, and servers keep running.".to_string() } else { state };
                let check = ui::sized_button("update-check", "Check now", Kind::Secondary, 30., 12.5).on_click(|_, _, _| crate::update::check_now());
                row(format!("HorizonX {}", env!("CARGO_PKG_VERSION")), note, div().flex().items_center().gap(px(8.)).child(ui::chip("Automatic", Some(Tone::Live))).child(check))
            })
    }

    fn settings_about(&mut self) -> Div {
        let data = horizon_launcher::backend::data_dir();
        let d2 = data.clone();
        let open = ui::sized_button("about-open", "Open", Kind::Secondary, 32., 12.5).on_click(move |_, _, cx| cx.open_with_system(&d2));
        group("About")
            .child(row(format!("HorizonX {}", env!("CARGO_PKG_VERSION")), "Make a Minecraft server on HorizonsX in a few clicks; friends join from anywhere.", crate::frame::logo(32.)))
            .child(row("Data folder", data.display().to_string(), open))
    }
}
