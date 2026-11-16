//! The setup wizard: Server type, Plugins, World, Performance, Review; then Setup, which gets
//! Horizon ready (once), makes the server, adds the plugins, writes the settings, starts it and
//! waits for its public address.
use crate::app::{Ev, Field, HostApp, Step, View};
use crate::ui::{self, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use crate::ui::px;
use horizon_launcher::host;
use horizon_launcher::servers::{self, NewServer, World};
use horizon_theme::glass::{ink, radius};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub const STEPS: [&str; 5] = ["Server type", "Plugins", "World", "Performance", "Review"];

#[derive(Clone, Copy, PartialEq)]
pub enum Engine {
    HorizonsX,
    Horizon,
}

pub struct Wizard {
    pub step: usize,
    pub engine: Engine,
    pub plugins: BTreeSet<String>,
    pub new_world: bool,
    pub copy: Option<PathBuf>,
    pub worlds: Vec<(String, PathBuf)>,
    pub mode: &'static str,
    pub difficulty: &'static str,
    pub max_players: u32,
    pub view: u32,
    pub sim: u32,
    pub friends_only: bool,
    pub accounts: bool,
    pub backups: bool,
    pub error: Option<String>,
}

impl Default for Wizard {
    fn default() -> Self {
        Self {
            step: 0,
            engine: if host::engine().is_some() { Engine::HorizonsX } else { Engine::Horizon },
            plugins: BTreeSet::new(),
            new_world: true,
            copy: None,
            worlds: vec![],
            mode: "survival",
            difficulty: "normal",
            max_players: 20,
            view: 10,
            sim: 8,
            friends_only: false,
            accounts: true,
            backups: true,
            error: None,
        }
    }
}

/// The Setup screen's state.
#[derive(Default)]
pub struct Setup {
    pub name: String,
    pub steps: Vec<(&'static str, Step)>,
    pub line: Option<String>,
    pub error: Option<String>,
    pub dir: Option<PathBuf>,
}

const SETUP_STEPS: [&str; 6] = [
    "Getting Horizon ready",
    "Creating the server",
    "Adding plugins",
    "Writing the settings",
    "Starting the server",
    "Getting a public address",
];
const START: usize = 4;
const PUBLIC: usize = 5;

const MODES: [(&str, &str); 3] = [("survival", "Survival"), ("creative", "Creative"), ("adventure", "Adventure")];
const DIFFICULTIES: [(&str, &str); 4] = [("peaceful", "Peaceful"), ("easy", "Easy"), ("normal", "Normal"), ("hard", "Hard")];

impl HostApp {
    pub fn open_wizard(&mut self, cx: &mut Context<Self>) {
        self.wiz = Wizard::default();
        let n = self.list.len();
        let name = if n == 0 { "My server".to_string() } else { format!("My server {}", n + 1) };
        self.set_text(Field::Name, &name);
        self.set_text(Field::Seed, "");
        self.view = View::Wizard;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Ev::Worlds(servers::player_worlds()));
        });
        cx.notify();
    }

    fn can_next(&self) -> Result<(), String> {
        match self.wiz.step {
            2 if !self.wiz.new_world && self.wiz.copy.is_none() => Err("Choose a world to copy".into()),
            3 => {
                let name = self.text(Field::Name);
                if name.trim().is_empty() {
                    return Err("Give the server a name".into());
                }
                if self.list.iter().any(|e| e.name.eq_ignore_ascii_case(name.trim())) {
                    return Err("A server with this name already exists".into());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn wizard(&mut self, cx: &mut Context<Self>) -> Div {
        let step = self.wiz.step;
        // The step rail.
        let mut rail = div().flex().items_center().gap(px(8.));
        for (i, label) in STEPS.iter().enumerate() {
            let done = i < step;
            let current = i == step;
            let badge = div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded_full()
                .bg(if current { ui::color(ui::ACCENT) } else if done { white(0.18) } else { white(0.07) })
                .child(if done {
                    ui::icon("tick-02", 14., gpui::white()).into_any_element()
                } else {
                    ui::text(13., FontWeight::SEMIBOLD, if current { ink::FULL } else { ink::QUIET })
                        .child(format!("{}", i + 1))
                        .into_any_element()
                });
            let item = div()
                .id(SharedString::from(format!("rail-{i}")))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .py(px(6.))
                .rounded_full()
                .when(done, |d| d.cursor_pointer().hover(|s| s.bg(white(0.06))))
                .child(badge)
                .child(ui::text(14., FontWeight::MEDIUM, if current { ink::FULL } else { ink::QUIET }).child(*label))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if i < this.wiz.step {
                        this.wiz.step = i;
                        this.wiz.error = None;
                        cx.notify();
                    }
                }));
            rail = rail.child(item);
            if i + 1 < STEPS.len() {
                rail = rail.child(div().w(px(24.)).h(px(1.)).bg(white(if done { 0.3 } else { 0.1 })));
            }
        }
        let (title, note, body) = match step {
            0 => ("What kind of server?", "Pick the engine your server runs on. You can add plugins next.", self.step_type(cx)),
            1 => ("Add plugins", "Horizon's plugins, made for HorizonsX. You can add or remove them later.", self.step_plugins(cx)),
            2 => ("Your world", "Start a fresh world or bring one of your own.", self.step_world(cx)),
            3 => ("Performance and access", "Name your server and choose who can join and how far players see.", self.step_performance(cx)),
            _ => ("Review your server", "Check everything, then create it. It starts by itself when it's ready.", self.step_review()),
        };
        let back = ui::button("wiz-back", if step == 0 { "Cancel" } else { "Back" }, Kind::Ghost)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.wiz.error = None;
                if this.wiz.step == 0 {
                    this.go_home(cx);
                } else {
                    this.wiz.step -= 1;
                }
                cx.notify();
            }));
        let next = if step + 1 < STEPS.len() {
            crate::demo::mark("wiz-next", ui::sized_button("wiz-next", "Continue", Kind::Primary, 46., 15.).relative()).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                match this.can_next() {
                    Ok(()) => {
                        this.wiz.error = None;
                        this.wiz.step += 1;
                    }
                    Err(e) => this.wiz.error = Some(e),
                }
                cx.notify();
            }))
        } else {
            crate::demo::mark("wiz-create", ui::sized_button("wiz-create", "Create server", Kind::Primary, 46., 15.).relative())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.create(cx)))
        };
        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .pt(px(20.))
            .child(back)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .children(self.wiz.error.clone().map(|e| ui::text(14., FontWeight::MEDIUM, ink::FULL).text_color(ui::color(ui::DANGER)).child(e)))
                    .child(next),
            );
        let panel = ui::panel()
            .w_full()
            .max_w(px(920.))
            .gap(px(6.))
            .p(px(36.))
            .child(ui::mono(12., ink::QUIET).child(format!("STEP {} OF {}", step + 1, STEPS.len())))
            .child(ui::display(38.).child(title))
            .child(ui::text(16., FontWeight::NORMAL, ink::BODY).mb(px(22.)).child(note))
            .child(body)
            .child(ui::rule().mt(px(24.)))
            .child(footer);
        div().flex_1().min_h_0().flex().flex_col().child(
            div()
            .id("wizard-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(24.))
            .px(px(32.))
            .pt(px(36.))
            .pb(px(24.))
            .child(rail)
            .child(panel),
        )
    }

    // ----- The steps ------------------------------------------------------------------------------

    fn step_type(&mut self, cx: &mut Context<Self>) -> Div {
        let hx_ok = host::engine().is_some();
        let card = |this: &Self, id: &'static str, engine: Engine, name: &'static str, tag: Option<&'static str>, about: &'static str, points: [&'static str; 3], enabled: bool| {
            let chosen = this.wiz.engine == engine;
            crate::demo::mark(id, ui::choice(id, chosen))
                .flex_1()
                .min_w_0()
                .when(!enabled, |d| d.opacity(0.45))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(ui::heading(22.).child(name))
                        .children(tag.map(|t| ui::chip(t, Some(Tone::Live)))),
                )
                .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(about))
                .child(div().flex().flex_col().gap(px(6.)).mt(px(6.)).children(points.into_iter().map(|p| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("tick-02", 14., white(ink::NOTE)))
                        .child(ui::text(13., FontWeight::NORMAL, ink::SOFT).child(p))
                })))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if enabled {
                        this.wiz.engine = engine;
                        cx.notify();
                    }
                }))
        };
        let ready = host::ready();
        let row = div()
            .flex()
            .gap(px(16.))
            .child(card(self, "engine-hx", Engine::HorizonsX, "HorizonsX", Some("For big servers"),
                "Region threads: each part of the world runs at its own speed, for massive player counts.",
                ["Hundreds of players at full speed", "Lag in one area stays in that area", "Rust and Java plugins"], hx_ok))
            .child(card(self, "engine-horizon", Engine::Horizon, "Horizon", None,
                "One speed for the whole server, like Paper: simple, and closest to vanilla.",
                ["One TPS for the whole world", "Rust and Java plugins", "Region threads one click away in Settings"], hx_ok));
        let version = div()
            .mt(px(18.))
            .flex()
            .items_center()
            .justify_between()
            .p(px(18.))
            .rounded(px(radius::ROW))
            .bg(white(0.05))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(ui::text(15., FontWeight::MEDIUM, ink::FULL).child("Minecraft: Java Edition 26.3"))
                    .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(if ready {
                        "Ready: the server files are prepared."
                    } else {
                        "First time: Horizon Host downloads Horizon and prepares the game from your own Minecraft. A few minutes, once."
                    })),
            )
            .child(ui::chip(if ready { "Ready" } else { "Prepared on create" }, Some(if ready { Tone::Live } else { Tone::Busy })));
        div().flex().flex_col().child(row).child(version)
    }

    fn step_plugins(&mut self, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap(px(12.));
        if self.catalog.is_empty() {
            return div().child(ui::text(15., FontWeight::NORMAL, ink::QUIET).child("No plugins came with Horizon Host."));
        }
        for p in self.catalog.clone() {
            let built_in = p.built_in;
            let on = built_in || self.wiz.plugins.contains(&p.file);
            let file = p.file.clone();
            let card = crate::demo::mark(format!("plug-{}", p.file), ui::choice(SharedString::from(format!("plug-{}", p.file)), on))
                .w(px(270.))
                .min_h(px(118.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(ui::text(16., FontWeight::SEMIBOLD, ink::FULL).child(p.name.clone()))
                        .child(if built_in {
                            ui::chip("Built in", None).into_any_element()
                        } else {
                            ui::toggle(SharedString::from(format!("plug-t-{}", p.file)), on).into_any_element()
                        }),
                )
                .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(p.description.clone()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !built_in && !this.wiz.plugins.remove(&file) {
                        this.wiz.plugins.insert(file.clone());
                    }
                    cx.notify();
                }));
            grid = grid.child(card);
        }
        grid
    }

    fn step_world(&mut self, cx: &mut Context<Self>) -> Div {
        let kind = ui::segments("world-kind", &[(true, "New world"), (false, "One of my worlds")], self.wiz.new_world, {
            let e = cx.entity();
            move |v, _, cx| e.update(cx, |this, cx| {
                this.wiz.new_world = v;
                this.wiz.error = None;
                cx.notify();
            })
        });
        let mut col = div().flex().flex_col().gap(px(4.)).child(kind);
        if self.wiz.new_world {
            let seed = self.field(Field::Seed, "Random", cx);
            let mode = ui::segments("world-mode", &MODES, self.wiz.mode, {
                let e = cx.entity();
                move |v, _, cx| e.update(cx, |this, cx| {
                    this.wiz.mode = v;
                    cx.notify();
                })
            });
            let diff = ui::segments("world-diff", &DIFFICULTIES, self.wiz.difficulty, {
                let e = cx.entity();
                move |v, _, cx| e.update(cx, |this, cx| {
                    this.wiz.difficulty = v;
                    cx.notify();
                })
            });
            col = col
                .child(div().mt(px(14.)).child(ui::setting_row("Seed", "Leave empty for a random world.", div().w(px(300.)).child(seed))))
                .child(ui::setting_row("Game mode", "", mode))
                .child(ui::setting_row("Difficulty", "", diff));
        } else if self.wiz.worlds.is_empty() {
            col = col.child(ui::text(14., FontWeight::NORMAL, ink::QUIET).mt(px(14.)).child("No single-player worlds were found in Horizon or Minecraft."));
        } else {
            let mut list = div().mt(px(14.)).flex().flex_col().gap(px(6.));
            for (i, (name, path)) in self.wiz.worlds.clone().into_iter().enumerate() {
                let chosen = self.wiz.copy.as_ref() == Some(&path);
                list = list.child(
                    div()
                        .id(SharedString::from(format!("world-{i}")))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(12.))
                        .rounded(px(radius::ROW))
                        .cursor_pointer()
                        .bg(white(if chosen { 0.1 } else { 0.04 }))
                        .border_1()
                        .border_color(if chosen { ui::color(ui::ACCENT) } else { white(0.0) })
                        .hover(|s| s.bg(white(0.08)))
                        .child(ui::icon("earth", 18., white(ink::BODY)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(ui::text(15., FontWeight::MEDIUM, ink::FULL).child(name.clone()))
                                .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).truncate().child(path.display().to_string())),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.wiz.copy = Some(path.clone());
                            this.wiz.error = None;
                            cx.notify();
                        })),
                );
            }
            col = col.child(list);
        }
        col
    }

    fn step_performance(&mut self, cx: &mut Context<Self>) -> Div {
        let name = self.field(Field::Name, "My server", cx);
        let e = cx.entity();
        let players = ui::stepper("max-players", self.wiz.max_players, 5, 1, 500, "", {
            let e = e.clone();
            move |v, _, cx| e.update(cx, |this, cx| {
                this.wiz.max_players = v;
                cx.notify();
            })
        });
        let view = ui::stepper("view", self.wiz.view, 2, 4, 32, " chunks", {
            let e = e.clone();
            move |v, _, cx| e.update(cx, |this, cx| {
                this.wiz.view = v;
                cx.notify();
            })
        });
        let sim = ui::stepper("sim", self.wiz.sim, 1, 3, 16, " chunks", {
            let e = e.clone();
            move |v, _, cx| e.update(cx, |this, cx| {
                this.wiz.sim = v;
                cx.notify();
            })
        });
        let toggle = |this: &Self, id: &'static str, on: bool, f: fn(&mut Wizard), cx: &mut Context<Self>| {
            let _ = this;
            ui::toggle(id, on).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                f(&mut this.wiz);
                cx.notify();
            }))
        };
        let friends = toggle(self, "friends-only", self.wiz.friends_only, |w| w.friends_only = !w.friends_only, cx);
        let accounts = toggle(self, "accounts", self.wiz.accounts, |w| w.accounts = !w.accounts, cx);
        let backups = crate::demo::mark("toggle-backups-row", toggle(self, "backups", self.wiz.backups, |w| w.backups = !w.backups, cx));
        let folder = servers::new_folder(&self.text(Field::Name));
        div()
            .flex()
            .flex_col()
            .child(ui::text(14., FontWeight::MEDIUM, ink::BODY).mb(px(8.)).child("Server name"))
            .child(name)
            .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).mt(px(6.)).mb(px(10.)).child(format!("Saved in {}", folder.display())))
            .child(ui::setting_row("Players", "How many can be on at once.", players))
            .child(ui::setting_row("View distance", "How far players see. Lower is lighter on your PC.", view))
            .child(ui::setting_row("Simulation distance", "How far mobs, crops and redstone keep running.", sim))
            .child(ui::setting_row("Friends only", "Only players you add can join. Off: anyone with the address.", friends))
            .child(ui::setting_row("Check Minecraft accounts", "Players sign in with Microsoft. Turn off only for testing.", accounts))
            .child(ui::setting_row("Automatic backups", "A backup every 6 hours, keeping the last 3.", backups))
            .child(ui::setting_row(
                "Public address",
                "Friends join from anywhere through playit.gg, with no port forwarding.",
                ui::chip("Built in", Some(Tone::Live)),
            ))
    }

    fn step_review(&self) -> Div {
        let w = &self.wiz;
        let plugins: Vec<String> = crate::app::with_playit(&w.plugins)
            .iter()
            .map(|f| self.catalog.iter().find(|p| &p.file == f).map(|p| p.name.clone()).unwrap_or_else(|| f.clone()))
            .collect();
        let row = |k: &'static str, v: String| {
            div()
                .flex()
                .justify_between()
                .gap(px(24.))
                .py(px(11.))
                .border_b_1()
                .border_color(white(0.07))
                .child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child(k))
                .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).text_right().child(v))
        };
        let world = if w.new_world {
            let seed = self.text(Field::Seed);
            format!("New world · {} · {}{}", cap(w.mode), cap(w.difficulty), if seed.trim().is_empty() { String::new() } else { format!(" · seed {}", seed.trim()) })
        } else {
            w.copy.as_ref().and_then(|p| p.file_name()).map(|n| format!("Copy of {}", n.to_string_lossy())).unwrap_or_default()
        };
        div()
            .flex()
            .flex_col()
            .child(row("Server name", self.text(Field::Name).trim().to_string()))
            .child(row("Server type", if w.engine == Engine::HorizonsX { "HorizonsX".into() } else { "Horizon".into() }))
            .child(row("Minecraft", "Java Edition 26.3".into()))
            .child(row("World", world))
            .child(row("Plugins", plugins.join(", ")))
            .child(row("Players", format!("Up to {}{}", w.max_players, if w.friends_only { " · friends only" } else { "" })))
            .child(row("Distances", format!("View {} · simulation {} chunks", w.view, w.sim)))
            .child(row("Accounts", if w.accounts { "Checked (Microsoft sign-in)".into() } else { "Not checked (offline)".into() }))
            .child(row("Backups", if w.backups { "Every 6 hours, last 3 kept".into() } else { "Off".into() }))
            .child(row("Public address", "playit.gg, no port forwarding".into()))
            .child(
                ui::text(12., FontWeight::NORMAL, ink::QUIET)
                    .mt(px(14.))
                    .child("By creating a server you agree to the Minecraft EULA (aka.ms/MinecraftEULA)."),
            )
    }

    // ----- Creating ------------------------------------------------------------------------------------

    pub(crate) fn create(&mut self, cx: &mut Context<Self>) {
        let w = &self.wiz;
        let name = self.text(Field::Name).trim().to_string();
        let seed = self.text(Field::Seed).trim().to_string();
        let spec = NewServer {
            name: name.clone(),
            world: if w.new_world { World::New { seed } } else { World::Copy(w.copy.clone().unwrap_or_default()) },
            game_mode: w.mode.to_string(),
            difficulty: w.difficulty.to_string(),
            max_players: w.max_players,
            friends_only: w.friends_only,
            check_accounts: w.accounts,
            players: vec![],
            eula: true,
        };
        let plugins: Vec<String> = crate::app::with_playit(&w.plugins).into_iter().collect();
        // Both run the HorizonsX engine: HorizonsX with region threads (a TPS per region), Horizon without (one
        // TPS for the server). The server's Settings switch them later.
        let regions = w.engine == Engine::HorizonsX;
        let settings = vec![
            ("properties.horizonsx-region-threads", regions.to_string()),
            ("server.view_distance", w.view.to_string()),
            ("server.simulation_distance", w.sim.to_string()),
            ("backups.every", if w.backups { "6h".into() } else { "0".into() }),
            ("backups.keep", "3".to_string()),
        ];
        self.setup = Setup { name: name.clone(), steps: SETUP_STEPS.iter().map(|s| (*s, Step::Waiting)).collect(), ..Setup::default() };
        self.view = View::Setup;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let step = |i: usize, s: Step| {
                let _ = tx.send(Ev::Step(i, s));
            };
            let line = |l: String| {
                let _ = tx.send(Ev::JobLine(l));
            };
            let fail = |i: usize, e: String| {
                let _ = tx.send(Ev::Step(i, Step::Failed));
                let _ = tx.send(Ev::Created(Err(e)));
            };
            step(0, Step::Running);
            let tag = match host::get_ready(&line) {
                Ok(t) => t,
                Err(e) => return fail(0, e),
            };
            step(0, Step::Done);
            step(1, Step::Running);
            let entry = match servers::create(&spec, Some(&tag), &line) {
                Ok(e) => e,
                Err(p) => return fail(1, p.message),
            };
            let dir = entry.dir.clone();
            // The HorizonsX engine for both kinds (the region-threads setting below makes the difference).
            for note in host::adopt(&dir) {
                line(note);
            }
            step(1, Step::Done);
            step(2, Step::Running);
            for p in &plugins {
                if !host::plugin_installed(&dir, p) {
                    if let Err(e) = host::install_plugin(&dir, p) {
                        line(format!("{p}: {e}"));
                    }
                }
            }
            step(2, Step::Done);
            step(3, Step::Running);
            let pairs: Vec<(&str, String)> = settings.iter().map(|(k, v)| (*k, v.clone())).collect();
            if let Err(e) = servers::set_config(&dir, &pairs) {
                line(e.message);
            }
            step(3, Step::Done);
            let _ = tx.send(Ev::Created(Ok(dir.clone())));
            step(START, Step::Running);
            line("Starting: a new world takes a minute the first time".into());
            if let Err(e) = servers::run_streamed(&dir, &["start"], &|_| {}) {
                // `start` returns when the server stops; an error before it ran is a failed start.
                let _ = tx.send(Ev::Note(e.message));
            }
        });
        cx.notify();
    }

    /// Marks Setup's last steps from the polled status and the playit plugin.
    pub fn setup_progress(&mut self, dir: &std::path::Path) {
        if self.view != View::Setup || self.setup.dir.as_deref() != Some(dir) {
            return;
        }
        let running = self.status.get(dir).is_some_and(|s| s.running());
        if running && self.setup.steps.get(START).is_some_and(|s| s.1 != Step::Done) {
            self.setup.steps[START].1 = Step::Done;
            self.setup.steps[PUBLIC].1 = Step::Running;
            self.setup.line = Some("The server is up. Asking playit.gg for an address".into());
        }
        if let Some(p) = self.playit.get(dir) {
            if (p.online() || p.waiting_for_claim()) && self.setup.steps.get(PUBLIC).is_some_and(|s| s.1 != Step::Done) {
                self.setup.steps[PUBLIC].1 = Step::Done;
                self.setup.line = None;
            }
        }
    }

    pub fn setup_view(&mut self, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(4.)).mt(px(18.));
        for (i, (label, s)) in self.setup.steps.clone().into_iter().enumerate() {
            let mark = match s {
                Step::Done => div().flex().items_center().justify_center().size(px(26.)).rounded_full().bg(ui::color(ui::LIVE).opacity(0.2)).child(ui::icon("tick-02", 14., ui::color(ui::LIVE))),
                Step::Running => div().flex().items_center().justify_center().size(px(26.)).rounded_full().bg(ui::color(ui::ACCENT).opacity(0.3)).child(ui::dot(Tone::Busy)),
                Step::Failed => div().flex().items_center().justify_center().size(px(26.)).rounded_full().bg(ui::color(ui::DANGER).opacity(0.2)).child(ui::icon("cancel-01", 14., ui::color(ui::DANGER))),
                Step::Waiting => div().flex().items_center().justify_center().size(px(26.)).rounded_full().bg(white(0.06)).child(ui::text(12., FontWeight::MEDIUM, ink::QUIET).child(format!("{}", i + 1))),
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .py(px(10.))
                    .child(mark)
                    .child(ui::text(16., FontWeight::MEDIUM, if s == Step::Waiting { ink::QUIET } else { ink::FULL }).child(label)),
            );
        }
        let all_done = self.setup.steps.iter().all(|s| s.1 == Step::Done);
        let title = if self.setup.error.is_some() {
            "Something went wrong".to_string()
        } else if all_done {
            format!("{} is running", self.setup.name)
        } else {
            format!("Setting up {}", self.setup.name)
        };
        let mut panel = ui::panel()
            .w_full()
            .max_w(px(720.))
            .p(px(36.))
            .child(ui::display(36.).child(title))
            .child(list);
        if let Some(l) = self.setup.line.clone().filter(|_| self.setup.error.is_none()) {
            panel = panel.child(ui::mono(12., ink::QUIET).mt(px(10.)).truncate().child(l));
        }
        if let Some(e) = self.setup.error.clone() {
            panel = panel
                .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).mt(px(12.)).text_color(ui::color(ui::DANGER)).child(e))
                .child(
                    div().flex().gap(px(10.)).mt(px(20.)).child(
                        ui::button("setup-back", "Back to the wizard", Kind::Secondary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.view = View::Wizard;
                            cx.notify();
                        })),
                    ),
                );
        }
        if let Some(dir) = self.setup.dir.clone() {
            if self.setup.steps.get(START).is_some_and(|s| s.1 == Step::Done) {
                panel = panel.child(ui::rule().my(px(22.))).child(self.share_card(&dir, cx)).child(
                    div().flex().justify_end().mt(px(22.)).child(
                        crate::demo::mark("setup-open", ui::sized_button("setup-open", "Open the dashboard", Kind::Primary, 46., 15.).relative()).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.open_server(dir.clone(), cx);
                        })),
                    ),
                );
            }
        }
        div().flex_1().min_h_0().flex().flex_col().child(div().id("setup-scroll").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().items_center().px(px(32.)).py(px(24.)).child(panel))
    }

    /// How friends join: the public address (or the link to get one) and this network's.
    pub fn share_card(&mut self, dir: &std::path::Path, cx: &mut Context<Self>) -> Div {
        let status = self.status_of(dir);
        let p = self.playit.get(dir).cloned();
        let lan = self.lan_address(dir);
        let mut public = div().flex().flex_col().gap(px(6.)).flex_1().min_w_0().child(ui::text(13., FontWeight::MEDIUM, ink::NOTE).child("Anywhere on the internet"));
        public = match (&p, status.running()) {
            (_, false) => public.child(ui::text(15., FontWeight::NORMAL, ink::QUIET).child("Start the server to get its address")),
            (Some(p), true) if p.online() => {
                let a = p.address.clone().unwrap_or_default();
                let copied = self.copied(&a);
                let a2 = a.clone();
                public.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::dot(Tone::Live))
                        .child(ui::heading(22.).truncate().child(a.clone()))
                        .child(ui::square("copy-public", if copied { "tick-02" } else { "copy-01" }, 34.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(a2.clone(), cx)))),
                )
            }
            (Some(p), true) if p.waiting_for_claim() => {
                let url = p.claim_url.clone().unwrap_or_default();
                public
                    .child(div().flex().child(ui::icon_button("claim", "link-01", "Get your public address", Kind::Primary).on_click(move |_, _, cx| cx.open_url(&url))))
                    .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Opens playit.gg: sign in or continue as a guest, then accept. Free, once per server."))
            }
            (Some(p), true) => public.child(ui::text(15., FontWeight::NORMAL, ink::BODY).child(cap(&p.state_text))),
            (None, true) => public.child(ui::text(15., FontWeight::NORMAL, ink::QUIET).child("Starting the tunnel…")),
        };
        let lan_copied = self.copied(&lan);
        let l2 = lan.clone();
        let here = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .w(px(260.))
            .child(ui::text(13., FontWeight::MEDIUM, ink::NOTE).child("On this Wi-Fi"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::text(17., FontWeight::MEDIUM, ink::FULL).child(lan.clone()))
                    .child(ui::square("copy-lan", if lan_copied { "tick-02" } else { "copy-01" }, 30.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(l2.clone(), cx)))),
            );
        div().flex().gap(px(28.)).child(public).child(here)
    }
}

pub fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[allow(unused)]
fn _keep(_: SharedString) {}
