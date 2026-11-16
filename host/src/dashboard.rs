//! A server's dashboard: its sections on the left (Overview, Console, Players, Plugins,
//! Performance, Backups, Schedule, Settings, Files), the open one on the right under the server's
//! name, state and Start / Stop / Restart.
use crate::app::{Ev, Field, HostApp};
use crate::ui::{self, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString, Window};
use crate::ui::px;
use horizon_launcher::host::{self, FileEntry};
use horizon_launcher::servers::{self, Status};
use horizon_theme::glass::{ink, radius};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Section {
    #[default]
    Overview,
    Console,
    Players,
    Plugins,
    Performance,
    Backups,
    Schedule,
    Settings,
    Files,
}

impl Section {
    pub fn from_name(name: &str) -> Section {
        SECTIONS.iter().find(|(_, l, _)| l.eq_ignore_ascii_case(name)).map(|(s, _, _)| *s).unwrap_or_default()
    }
}

const SECTIONS: [(Section, &str, &str); 9] = [
    (Section::Overview, "Overview", "dashboard-square-01"),
    (Section::Console, "Console", "computer-terminal-01"),
    (Section::Players, "Players", "user-multiple-02"),
    (Section::Plugins, "Plugins", "puzzle"),
    (Section::Performance, "Performance", "view"),
    (Section::Backups, "Backups", "download-04"),
    (Section::Schedule, "Schedule", "refresh"),
    (Section::Settings, "Settings", "sliders-horizontal"),
    (Section::Files, "Files", "folder-01"),
];

#[derive(Default)]
pub struct Dash {
    pub section: Section,
    pub log: VecDeque<String>,
    pub log_next: u64,
    pub log_dir: Option<PathBuf>,
    pub files_dir: Option<PathBuf>,
    pub files_rel: String,
    pub files: Option<Result<Vec<FileEntry>, String>>,
    pub viewing: Option<(String, Result<String, String>)>,
    pub schedule_checked: Option<Instant>,
    pub config: Option<(PathBuf, Value)>,
    pub config_loading: bool,
    pub confirm_delete: bool,
}

const BACKUP_EVERY: [(&str, &str); 6] = [("0", "Off"), ("1h", "1 h"), ("3h", "3 h"), ("6h", "6 h"), ("12h", "12 h"), ("24h", "Daily")];
/// Changes a server's schedule (host.json) and says so if it couldn't be saved.
fn change_schedule(e: &gpui::Entity<HostApp>, d: &Path, cx: &mut gpui::App, change: impl FnOnce(&mut host::Schedule)) {
    e.update(cx, |this, cx| {
        let mut s = host::schedule(d);
        change(&mut s);
        if let Err(err) = host::save_schedule(d, &s) {
            this.note = Some((err, Instant::now()));
        }
        cx.notify();
    })
}

const RESTART_MODE: [(u32, &str); 4] = [(0, "Off"), (6, "Every 6 h"), (12, "Every 12 h"), (24, "Daily")];
const RESTART_AT: [(u32, &str); 6] = [(0, "0:00"), (3, "3:00"), (4, "4:00"), (5, "5:00"), (6, "6:00"), (12, "12:00")];
const MODES: [(&str, &str); 3] = [("survival", "Survival"), ("creative", "Creative"), ("adventure", "Adventure")];
const DIFFICULTIES: [(&str, &str); 4] = [("peaceful", "Peaceful"), ("easy", "Easy"), ("normal", "Normal"), ("hard", "Hard")];

impl HostApp {
    pub fn dashboard(&mut self, dir: &Path, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let name = self.list.iter().find(|e| e.dir == dir).map(|e| e.name.clone()).unwrap_or_default();
        let st = self.status_of(dir);
        let busy = self.busy.get(dir).cloned();
        // The sections: one centred row of tabs (the app's sidebar is already on the left).
        let options: Vec<(Section, &str)> = SECTIONS.iter().map(|(s, l, _)| (*s, *l)).collect();
        let sections = ui::tabs("server-sections", &options, self.dash.section, {
            let e = cx.entity();
            move |s, _, cx| e.update(cx, |this, cx| this.open_section(s, cx))
        });
        let back = crate::demo::mark("back-home", div()
            .id("back-home")
            .relative()
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .hover(|s| s.opacity(0.8))
            .child(ui::text(14., FontWeight::MEDIUM, ink::QUIET).child("‹  Servers"))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_page(crate::frame::Page::Servers, cx))));
        // The head: name, state, actions.
        let (tone, state) = match (&busy, st.phase.as_str()) {
            (Some(b), _) => (Tone::Busy, b.clone()),
            (_, "running") => (Tone::Live, match st.uptime {
                Some(s) if s >= 3600 => format!("Running · up {} h {} min", s / 3600, s % 3600 / 60),
                Some(s) => format!("Running · up {} min", s / 60),
                None => "Running".into(),
            }),
            (_, "stopped") => (Tone::Off, "Stopped".into()),
            (_, p) => (Tone::Busy, crate::wizard::cap(p)),
        };
        let d = dir.to_path_buf();
        let mut actions = div().flex().items_center().gap(px(8.));
        if st.running() {
            let (d1, d2) = (d.clone(), d.clone());
            actions = actions
                .child(ui::button("head-restart", "Restart", Kind::Secondary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.restart(d1.clone(), cx))))
                .child(ui::button("head-stop", "Stop", Kind::Secondary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.stop(d2.clone(), cx))));
        } else {
            let d1 = d.clone();
            actions = actions.child(ui::sized_button("head-start", "Start", Kind::Primary, 40., 15.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.start(d1.clone(), cx))));
        }
        let folder = d.clone();
        actions = actions.child(ui::square("head-folder", "folder-01", 40.).on_click(move |_, _, cx| cx.open_with_system(&folder)));
        let head = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(back)
                    .child(ui::display(38.).mt(px(8.)).truncate().child(name))
                    .child(div().flex().items_center().gap(px(8.)).mt(px(4.)).child(ui::dot(tone)).child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(state))),
            )
            .child(actions);
        let body = match self.dash.section {
            Section::Overview => self.overview(dir, &st, cx),
            Section::Console => self.console(dir, &st, cx),
            Section::Players => self.players(dir, &st, cx),
            Section::Plugins => self.plugins(dir, &st, cx),
            Section::Performance => self.performance(dir, &st),
            Section::Backups => self.backups_section(dir, &st, cx),
            Section::Schedule => self.schedule(dir, cx),
            Section::Settings => self.settings(dir, cx),
            Section::Files => self.files(dir, cx),
        };
        // The name and actions across the top, the sections centred under them, the open one below.
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pt(px(8.))
            .child(head)
            .child(div().flex().justify_center().child(sections))
            .child(
                div()
                    .id("dash-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .pr(px(4.))
                    .child(body),
            )
    }

    pub(crate) fn open_section(&mut self, s: Section, cx: &mut Context<Self>) {
        self.dash.section = s;
        self.dash.confirm_delete = false;
        if let crate::app::View::Server(dir) = self.view.clone() {
            match s {
                Section::Settings => self.load_config(&dir),
                Section::Backups | Section::Plugins => self.load_details(&dir),
                _ => {}
            }
        }
        cx.notify();
    }

    /// Reads the server's settings in the background (`horizon-server config` is a process).
    fn load_config(&mut self, dir: &Path) {
        if self.dash.config_loading {
            return;
        }
        self.dash.config_loading = true;
        let tx = self.tx.clone();
        let d = dir.to_path_buf();
        std::thread::spawn(move || {
            let c = servers::config(&d);
            let _ = tx.send(Ev::Config(d, c, None));
        });
    }

    fn setting(&self, dir: &Path, pointer: &str) -> Option<Value> {
        self.dash.config.as_ref().filter(|(d, _)| d == dir).and_then(|(_, c)| c.pointer(pointer).cloned())
    }

    /// A change shows at once (the switch flips, the number steps) and is saved in the background;
    /// the settings are read again after, with what `config set` said.
    pub(crate) fn set_setting(&mut self, dir: &Path, key: &'static str, value: String, cx: &mut Context<Self>) {
        if let Some((_, c)) = self.dash.config.as_mut().filter(|(d, _)| d == dir) {
            let shown = match value.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                v => v.parse::<u64>().map(Value::from).unwrap_or_else(|_| Value::String(v.to_string())),
            };
            if let Some(slot) = c.pointer_mut(&format!("/{}", key.replace('.', "/"))) {
                *slot = shown;
            }
        }
        let tx = self.tx.clone();
        let d = dir.to_path_buf();
        std::thread::spawn(move || {
            let note = match servers::set_config(&d, &[(key, value)]) {
                Ok(a) if !a.restart.is_empty() && a.running => Some("Saved. It applies at the next restart.".to_string()),
                Ok(_) => None,
                Err(e) => Some(e.message),
            };
            let c = servers::config(&d);
            let _ = tx.send(Ev::Config(d, c, note));
        });
        cx.notify();
    }

    // ----- Overview --------------------------------------------------------------------------------

    fn overview(&mut self, dir: &Path, st: &Status, cx: &mut Context<Self>) -> Div {
        let fmt = |v: Option<f64>, d: usize| v.map(|x| format!("{x:.d$}")).unwrap_or_else(|| "–".into());
        let stats = div()
            .flex()
            .gap(px(12.))
            .child(ui::stat(match st.max_players {
                Some(max) if st.running() => format!("{} / {max}", st.players.len()),
                _ => "–".to_string(),
            }, "PLAYERS ONLINE"))
            .child(ui::stat(fmt(st.tps, 1), "TPS · 20 IS FULL SPEED"))
            .child(ui::stat(format!("{} ms", fmt(st.mspt, 1)), "TICK TIME"))
            .child(ui::stat(st.ram_bytes.map(|b| format!("{:.2} GB", b as f64 / 1e9)).unwrap_or_else(|| "–".into()), "MEMORY"));
        let share = ui::panel().child(ui::heading(18.).mb(px(14.)).child("Invite friends")).child(self.share_card(dir, cx));
        let mut recent = div().flex().flex_col().p(px(16.)).rounded(px(radius::ROW)).bg(gpui::black().opacity(0.25));
        let start = self.dash.log.len().saturating_sub(10);
        if self.dash.log.is_empty() {
            recent = recent.child(ui::mono(12., ink::QUIET).child(if st.running() { "Waiting for the log…" } else { "The server is stopped." }));
        }
        for l in self.dash.log.iter().skip(start) {
            recent = recent.child(ui::mono(12., ink::BODY).truncate().child(l.clone()));
        }
        let console = ui::panel()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .mb(px(12.))
                    .child(ui::heading(18.).child("Latest in the console"))
                    .child(ui::button("ov-console", "Open the console", Kind::Ghost).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_section(Section::Console, cx)))),
            )
            .child(recent);
        div().flex().flex_col().gap(px(16.)).child(stats).child(share).child(console)
    }

    // ----- Console -----------------------------------------------------------------------------------

    fn console(&mut self, _dir: &Path, st: &Status, cx: &mut Context<Self>) -> Div {
        let mut lines = div().id("console-lines").flex().flex_col().h(px(460.)).overflow_y_scroll().p(px(16.)).rounded(px(radius::ROW)).bg(gpui::black().opacity(0.3));
        let start = self.dash.log.len().saturating_sub(400);
        for l in self.dash.log.iter().skip(start) {
            lines = lines.child(ui::mono(12., ink::BODY).child(l.clone()));
        }
        let field = self.field(Field::Command, if st.running() { "Type a command: say hello, op Name, time set day" } else { "Start the server to send commands" }, cx);
        let send = ui::button("console-send", "Send", Kind::Primary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send_command(cx)));
        ui::panel().gap(px(12.)).child(lines).child(div().flex().gap(px(10.)).child(div().flex_1().child(field)).child(send))
    }

    pub fn send_command(&mut self, cx: &mut Context<Self>) {
        let crate::app::View::Server(dir) = self.view.clone() else { return };
        let line = self.text(Field::Command).trim().trim_start_matches('/').to_string();
        if line.is_empty() {
            return;
        }
        self.set_text(Field::Command, "");
        self.dash.log.push_back(format!("> {line}"));
        let tx = self.tx.clone();
        std::thread::spawn(move || match host::command(&dir, &line) {
            Ok(out) if !out.is_empty() => {
                let _ = tx.send(Ev::Note(out.join(" ")));
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(Ev::Note(e.message));
            }
        });
        cx.notify();
    }

    fn command(&mut self, dir: &Path, line: String) {
        let tx = self.tx.clone();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let msg = match host::command(&dir, &line) {
                Ok(out) => out.join(" "),
                Err(e) => e.message,
            };
            if !msg.is_empty() {
                let _ = tx.send(Ev::Note(msg));
            }
        });
    }

    // ----- Players -----------------------------------------------------------------------------------

    fn players(&mut self, dir: &Path, st: &Status, cx: &mut Context<Self>) -> Div {
        let running = st.running();
        let mut online = div().flex().flex_col().gap(px(4.));
        if st.players.is_empty() {
            online = online.child(ui::text(14., FontWeight::NORMAL, ink::QUIET).py(px(8.)).child(if running { "Nobody is on right now." } else { "The server is stopped." }));
        }
        for (i, p) in st.players.clone().into_iter().enumerate() {
            let (n1, n2, n3) = (p.name.clone(), p.name.clone(), p.name.clone());
            let d = dir.to_path_buf();
            let (da, db, dc) = (d.clone(), d.clone(), d);
            online = online.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(8.))
                    .child(div().size(px(30.)).rounded(px(7.)).bg(white(0.1)).flex().items_center().justify_center().child(ui::icon("user", 15., white(ink::BODY))))
                    .child(div().flex_1().child(ui::text(15., FontWeight::MEDIUM, ink::FULL).child(p.name.clone())))
                    .children(p.ping.map(|ms| ui::mono(12., ink::QUIET).child(format!("{ms} ms"))))
                    .child(ui::sized_button(SharedString::from(format!("op-{i}")), "Make operator", Kind::Secondary, 32., 13.).on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.command(&da, format!("op {n1}")))))
                    .child(ui::sized_button(SharedString::from(format!("kick-{i}")), "Kick", Kind::Secondary, 32., 13.).on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.command(&db, format!("kick {n2}")))))
                    .child(ui::sized_button(SharedString::from(format!("ban-{i}")), "Ban", Kind::Danger, 32., 13.).on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.command(&dc, format!("ban {n3}"))))),
            );
        }
        let list = |this: &mut Self, title: &'static str, file: &'static str, remove: &'static str, cx: &mut Context<Self>| {
            let names = servers::names(dir, file);
            let mut col = div().flex().flex_col().flex_1().min_w_0().gap(px(2.)).child(ui::text(14., FontWeight::SEMIBOLD, ink::FULL).mb(px(6.)).child(format!("{title} · {}", names.len())));
            if names.is_empty() {
                col = col.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child("None"));
            }
            for (i, n) in names.into_iter().enumerate() {
                let d = dir.to_path_buf();
                let line = format!("{remove} {n}");
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .py(px(4.))
                        .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(n))
                        .when(running, |row| {
                            row.child(ui::sized_button(SharedString::from(format!("{file}-rm-{i}")), "Remove", Kind::Ghost, 28., 12.).on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.command(&d, line.clone()))))
                        }),
                );
            }
            let _ = this;
            col
        };
        let ops = list(self, "Operators", "ops.json", "deop", cx);
        let wl = list(self, "Friends list", "whitelist.json", "whitelist remove", cx);
        let bans = list(self, "Banned", "banned-players.json", "pardon", cx);
        let add = self.field(Field::AddPlayer, "A player's name", cx);
        let add_btn = ui::button("add-friend", "Add to friends list", Kind::Secondary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_player(cx)));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(ui::panel().child(ui::heading(18.).mb(px(8.)).child("Online now")).child(online))
            .child(
                ui::panel()
                    .gap(px(16.))
                    .child(div().flex().gap(px(10.)).child(div().flex_1().child(add)).child(add_btn))
                    .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(if running { "Changes apply at once." } else { "Start the server to change these lists." }))
                    .child(div().flex().gap(px(24.)).child(ops).child(wl).child(bans)),
            )
    }

    pub fn add_player(&mut self, cx: &mut Context<Self>) {
        let crate::app::View::Server(dir) = self.view.clone() else { return };
        let name = self.text(Field::AddPlayer).trim().to_string();
        if name.is_empty() {
            return;
        }
        self.set_text(Field::AddPlayer, "");
        self.command(&dir, format!("whitelist add {name}"));
        cx.notify();
    }

    // ----- Plugins -----------------------------------------------------------------------------------

    fn plugins(&mut self, dir: &Path, st: &Status, cx: &mut Context<Self>) -> Div {
        let horizon = self.horizon_plugins(dir, st);
        let java = self.java_plugins_panel(dir, cx);
        div().flex().flex_col().gap(px(16.)).child(horizon).child(java)
    }

    /// The jars in the server (Bukkit, Spigot, Paper plugins run by the Java layer): each with its version, what
    /// it says it does, and its settings files, which open in Files.
    fn java_plugins_panel(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        let (list, layer) = self.java.get(dir).cloned().unwrap_or_default();
        let plugins_folder = dir.join("plugins");
        let open_folder = ui::sized_button("java-open-folder", "Open the plugins folder", Kind::Ghost, 32., 13.).on_click(move |_, _, cx| cx.open_with_system(&plugins_folder));
        let mut panel = ui::panel().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().flex().items_center().gap(px(10.)).child(ui::heading(18.).child("Java plugins")).when(!list.is_empty(), |d| d.child(ui::chip(list.len().to_string(), None))))
                .child(open_folder),
        );
        if list.is_empty() {
            return panel.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).mt(px(6.)).child(
                "No Java plugins in this server. Bukkit, Spigot and Paper plugin jars in its plugins folder show here, with their settings.",
            ));
        }
        panel = panel.child(if layer {
            ui::text(13., FontWeight::NORMAL, ink::QUIET).mt(px(4.)).mb(px(6.)).child("Run unchanged by HorizonsX's Java layer. A jar added or removed takes effect at the next restart.")
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .mt(px(8.))
                .mb(px(6.))
                .p(px(12.))
                .rounded(px(radius::ROW))
                .bg(ui::color(ui::WARN).opacity(0.12))
                .child(ui::icon("information-circle", 16., ui::color(ui::WARN)))
                .child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child("The Java layer isn't on for this server, so these jars don't run yet. Their files are still here to look at."))
        });
        for (i, p) in list.into_iter().enumerate() {
            let mut configs = div().flex().flex_wrap().gap(px(6.)).mt(px(8.));
            let folder = p.folder.clone().unwrap_or_default();
            for (j, cfg) in p.configs.iter().take(6).enumerate() {
                let shown = cfg.strip_prefix(&format!("{folder}/")).unwrap_or(cfg).to_string();
                let (d, f, c) = (dir.to_path_buf(), folder.clone(), cfg.clone());
                configs = configs.child(
                    div()
                        .id(SharedString::from(format!("jcfg-{i}-{j}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded_full()
                        .bg(white(0.06))
                        .cursor_pointer()
                        .hover(|s| s.bg(white(0.12)))
                        .child(ui::icon("pencil-edit-02", 12., white(ink::BODY)))
                        .child(ui::mono(12., ink::FULL).child(shown))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_plugin_file(&d, &f, Some(c.clone()), cx))),
                );
            }
            if p.configs.len() > 6 {
                let (d, f) = (dir.to_path_buf(), folder.clone());
                configs = configs.child(
                    div()
                        .id(SharedString::from(format!("jcfg-more-{i}")))
                        .flex()
                        .items_center()
                        .h(px(28.))
                        .px(px(10.))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|s| s.bg(white(0.06)))
                        .child(ui::text(12., FontWeight::MEDIUM, ink::BODY).child(format!("+{} more", p.configs.len() - 6)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_plugin_file(&d, &f, None, cx))),
                );
            }
            let about = if p.description.is_empty() { None } else { Some(p.description.clone()) };
            let by = (!p.authors.is_empty()).then(|| format!("by {}", p.authors.join(", ")));
            let files_button = p.folder.clone().map(|f| {
                let d = dir.to_path_buf();
                ui::sized_button(SharedString::from(format!("jfiles-{i}")), "Files", Kind::Secondary, 32., 13.)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_plugin_file(&d, &f, None, cx)))
            });
            panel = panel.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(14.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(white(0.07))
                    .child(div().flex_none().size(px(40.)).rounded(px(radius::ROW)).bg(ui::color(0x5bb8ff).opacity(0.18)).flex().items_center().justify_center().child(ui::icon("package-01", 18., ui::color(0x5bb8ff))))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(ui::text(15., FontWeight::SEMIBOLD, ink::FULL).child(p.name.clone()))
                                    .when(!p.version.is_empty(), |d| d.child(ui::mono(11., ink::QUIET).px(px(6.)).rounded(px(4.)).bg(white(0.06)).child(p.version.clone())))
                                    .when(layer, |d| d.child(ui::dot(Tone::Live))),
                            )
                            .children(about.map(|a| ui::text(13., FontWeight::NORMAL, ink::QUIET).truncate().child(a)))
                            .children(by.map(|b| ui::text(12., FontWeight::NORMAL, ink::FAINT).truncate().child(b)))
                            .when(!p.configs.is_empty(), |d| d.child(configs))
                            .when(p.folder.is_none(), |d| d.child(ui::text(12., FontWeight::NORMAL, ink::FAINT).mt(px(4.)).child("No settings yet: they appear after its first start."))),
                    )
                    .child(div().flex().flex_col().items_end().gap(px(6.)).children(files_button).child(ui::mono(11., ink::QUIET).child(format!("{} · {}", p.file, size(p.bytes))))),
            );
        }
        panel
    }

    /// Files, open at a plugin's data folder, with one of its settings files shown (or just the folder).
    pub(crate) fn open_plugin_file(&mut self, dir: &Path, folder: &str, file: Option<String>, cx: &mut Context<Self>) {
        self.dash.section = Section::Files;
        let at = file.as_deref().and_then(|f| f.rsplit_once('/').map(|(parent, _)| parent.to_string())).unwrap_or_else(|| folder.to_string());
        self.load_files(dir, at);
        if let Some(file) = file {
            let (d, tx) = (dir.to_path_buf(), self.tx.clone());
            std::thread::spawn(move || {
                let text = host::read_text(&d, &file);
                let _ = tx.send(Ev::FileText(file, text));
            });
        }
        cx.notify();
    }

    /// The plugins made for HorizonsX that come with HorizonX: add or remove each.
    fn horizon_plugins(&mut self, dir: &Path, st: &Status) -> Div {
        let installed = self.installed.get(dir).cloned().unwrap_or_default();
        let mut list = div().flex().flex_col();
        for p in self.catalog.clone() {
            let on = installed.contains(&p.file);
            let file = p.file.clone();
            let d = dir.to_path_buf();
            let tx = self.tx.clone();
            let action = if p.built_in && on {
                ui::chip("Built in", Some(Tone::Live)).into_any_element()
            } else {
                ui::sized_button(SharedString::from(format!("pl-{}", p.file)), if on { "Remove" } else { "Add" }, if on { Kind::Secondary } else { Kind::Primary }, 34., 13.)
                    .on_click(move |_, _, _| {
                        let (d, file, tx) = (d.clone(), file.clone(), tx.clone());
                        std::thread::spawn(move || {
                            let r = if on { host::remove_plugin(&d, &file) } else { host::install_plugin(&d, &file) };
                            if let Err(e) = r {
                                let _ = tx.send(Ev::Note(e));
                            }
                            let _ = tx.send(Ev::Installed(d.clone(), host::installed_plugins(&d)));
                        });
                    })
                    .into_any_element()
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(white(0.07))
                    .child(div().size(px(40.)).rounded(px(radius::ROW)).bg(white(0.08)).flex().items_center().justify_center().child(ui::icon("puzzle", 18., white(ink::FULL))))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(div().flex().items_center().gap(px(8.)).child(ui::text(15., FontWeight::SEMIBOLD, ink::FULL).child(p.name.clone())).when(on, |d| d.child(ui::dot(Tone::Live))))
                            .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(p.description.clone())),
                    )
                    .child(action),
            );
        }
        ui::panel()
            .child(ui::heading(18.).child("HorizonsX plugins"))
            .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).mb(px(6.)).child(if st.running() {
                "Added plugins start at once on HorizonsX. A removed one is gone after a restart."
            } else {
                "Horizon's plugins, made for HorizonsX."
            }))
            .child(list)
    }

    // ----- Performance -------------------------------------------------------------------------------

    fn performance(&mut self, dir: &Path, st: &Status) -> Div {
        let h = self.history.get(dir).cloned().unwrap_or_default();
        if h.tps.is_empty() {
            return ui::panel().child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child(if st.running() { "Collecting: the graphs fill in over the next minutes." } else { "Start the server to see how it runs." }));
        }
        let last = |v: &Vec<f32>| v.last().copied().unwrap_or(0.0);
        let max = |v: &Vec<f32>, floor: f32| v.iter().cloned().fold(floor, f32::max);
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(div().flex().gap(px(14.)).child(graph("Ticks a second", format!("{:.1}", last(&h.tps)), &h.tps, 20.)).child(graph("Tick time", format!("{:.1} ms", last(&h.mspt)), &h.mspt, max(&h.mspt, 50.))))
            .child(div().flex().gap(px(14.)).child(graph("Memory", format!("{:.2} GB", last(&h.ram_gb)), &h.ram_gb, max(&h.ram_gb, 1.))).when(h.cpu.iter().any(|c| *c > 0.), |d| d.child(graph("Processor", format!("{:.0} %", last(&h.cpu)), &h.cpu, max(&h.cpu, 100.)))))
            .child(graph("Players online", format!("{:.0}", last(&h.players)), &h.players, max(&h.players, 5.)))
    }

    // ----- Backups -----------------------------------------------------------------------------------

    fn backups_section(&mut self, dir: &Path, st: &Status, cx: &mut Context<Self>) -> Div {
        let list = self.backups.get(dir).cloned().unwrap_or_default();
        let d = dir.to_path_buf();
        let now = ui::icon_button("backup-now", "download-04", "Back up now", Kind::Primary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.server_action(d.clone(), "Backing up", vec!["backup".into()], cx);
        }));
        let mut rows = div().flex().flex_col();
        if list.is_empty() {
            rows = rows.child(ui::text(14., FontWeight::NORMAL, ink::QUIET).py(px(10.)).child("No backups yet."));
        }
        for (i, b) in list.into_iter().rev().enumerate() {
            let d = dir.to_path_buf();
            let name = b.name.clone();
            let running = st.running();
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(white(0.07))
                    .child(div().flex().flex_col().flex_1().child(ui::text(15., FontWeight::MEDIUM, ink::FULL).child(b.name.clone())).child(ui::mono(12., ink::QUIET).child(size(b.bytes))))
                    .child(ui::sized_button(SharedString::from(format!("restore-{i}")), "Restore", Kind::Secondary, 34., 13.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if running {
                            this.note = Some(("Stop the server before restoring a backup.".into(), Instant::now()));
                            cx.notify();
                        } else {
                            this.server_action(d.clone(), "Restoring", vec!["restore".into(), name.clone(), "--yes".into()], cx);
                        }
                    }))),
            );
        }
        ui::panel()
            .child(div().flex().items_center().justify_between().mb(px(8.)).child(ui::heading(18.).child("Backups")).child(now))
            .child(rows)
    }

    // ----- Schedule ----------------------------------------------------------------------------------

    fn schedule(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        if self.dash.config.as_ref().is_none_or(|(d, _)| d != dir) {
            self.load_config(dir);
        }
        let every = self.setting(dir, "/backups/every").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "0".into());
        let every: &'static str = BACKUP_EVERY.iter().find(|(k, _)| *k == every).map(|(k, _)| *k).unwrap_or("0");
        let keep = self.setting(dir, "/backups/keep").and_then(|v| v.as_u64()).unwrap_or(12) as u32;
        let sched = host::schedule(dir);
        // Off, every 6 or 12 hours of uptime, or daily at a time (24 = daily).
        let mode = match (sched.restart_every, sched.restart_hour) {
            (6 | 12, _) => sched.restart_every,
            (_, Some(_)) => 24,
            _ => 0,
        };
        let at = sched.restart_hour.unwrap_or(4);
        let e = cx.entity();
        let d = dir.to_path_buf();
        let backups = ui::segments("sch-backups", &BACKUP_EVERY, every, {
            let (e, d) = (e.clone(), d.clone());
            move |v, _, cx| e.update(cx, |this, cx| this.set_setting(&d, "backups.every", v.to_string(), cx))
        });
        let keep = ui::stepper("sch-keep", keep, 1, 1, 50, "", {
            let (e, d) = (e.clone(), d.clone());
            move |v, _, cx| e.update(cx, |this, cx| this.set_setting(&d, "backups.keep", v.to_string(), cx))
        });
        let restart = ui::segments("sch-restart", &RESTART_MODE, mode, {
            let (e, d) = (e.clone(), d.clone());
            move |v, _, cx| {
                change_schedule(&e, &d, cx, move |s: &mut host::Schedule| match v {
                    6 | 12 => {
                        s.restart_every = v;
                        s.restart_hour = None;
                    }
                    24 => {
                        s.restart_every = 0;
                        s.restart_hour = Some(s.restart_hour.unwrap_or(4));
                    }
                    _ => {
                        s.restart_every = 0;
                        s.restart_hour = None;
                    }
                })
            }
        });
        let time = ui::segments("sch-restart-at", &RESTART_AT, at, {
            let (e, d) = (e.clone(), d.clone());
            move |v, _, cx| change_schedule(&e, &d, cx, move |s: &mut host::Schedule| s.restart_hour = Some(v))
        });
        let note = match mode {
            6 | 12 => format!("Every {mode} hours the server has been up, while HorizonX is open. A fresh start keeps a long-running server quick."),
            24 => "Once a day at the time below, local time, while HorizonX is open.".to_string(),
            _ => "A fresh start now and then keeps a long-running server quick.".to_string(),
        };
        ui::panel()
            .child(ui::heading(18.).mb(px(4.)).child("Schedule"))
            .child(ui::setting_row("Automatic backups", "While the server runs.", backups))
            .child(ui::setting_row("Backups kept", "Older ones are removed.", keep))
            .child(ui::setting_row("Automatic restart", note, restart))
            .when(mode == 24, |p| p.child(ui::setting_row("Daily restart time", "Local time.", time)))
    }

    // ----- Settings ----------------------------------------------------------------------------------

    fn settings(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        if self.dash.config.as_ref().is_none_or(|(d, _)| d != dir) {
            self.load_config(dir);
        }
        let s = |this: &Self, p: &str| this.setting(dir, p);
        let mode = s(self, "/world/game_mode").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "survival".into());
        let mode: &'static str = MODES.iter().find(|(k, _)| *k == mode).map(|(k, _)| *k).unwrap_or("survival");
        let diff = s(self, "/world/difficulty").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "normal".into());
        let diff: &'static str = DIFFICULTIES.iter().find(|(k, _)| *k == diff).map(|(k, _)| *k).unwrap_or("normal");
        let num = |this: &Self, p: &str, d: u64| s(this, p).and_then(|v| v.as_u64()).unwrap_or(d) as u32;
        let boolean = |this: &Self, p: &str| s(this, p).and_then(|v| v.as_bool()).unwrap_or(false);
        let (players, view, sim) = (num(self, "/server/max_players", 20), num(self, "/server/view_distance", 10), num(self, "/server/simulation_distance", 8));
        let (friends, accounts) = (boolean(self, "/server/whitelist"), boolean(self, "/server/online_mode"));
        let e = cx.entity();
        let d = dir.to_path_buf();
        let set = |key: &'static str| {
            let (e, d) = (e.clone(), d.clone());
            move |v: String, cx: &mut gpui::App| e.update(cx, |this, cx| this.set_setting(&d, key, v, cx))
        };
        let mode_c = { let f = set("world.game_mode"); ui::segments("set-mode", &MODES, mode, move |v, _, cx| f(v.to_string(), cx)) };
        let diff_c = { let f = set("world.difficulty"); ui::segments("set-diff", &DIFFICULTIES, diff, move |v, _, cx| f(v.to_string(), cx)) };
        let players_c = { let f = set("server.max_players"); ui::stepper("set-players", players, 5, 1, 500, "", move |v, _, cx| f(v.to_string(), cx)) };
        let view_c = { let f = set("server.view_distance"); ui::stepper("set-view", view, 2, 4, 32, " chunks", move |v, _, cx| f(v.to_string(), cx)) };
        let sim_c = { let f = set("server.simulation_distance"); ui::stepper("set-sim", sim, 1, 3, 16, " chunks", move |v, _, cx| f(v.to_string(), cx)) };
        let friends_c = { let f = set("server.whitelist"); crate::demo::mark("set-friends", ui::toggle("set-friends", friends).on_click(move |_, _, cx| f((!friends).to_string(), cx))) };
        let accounts_c = { let f = set("server.online_mode"); ui::toggle("set-accounts", accounts).on_click(move |_, _, cx| f((!accounts).to_string(), cx)) };
        // Region threads (HorizonsX): each region its own TPS. Off: one TPS for the server (Horizon). On unless set.
        let regions = s(self, "/properties/horizonsx-region-threads")
            .map(|v| v.as_bool().unwrap_or_else(|| v.as_str().map(|t| t.trim() != "false").unwrap_or(true)))
            .unwrap_or(true);
        let regions_c = {
            let f = set("properties.horizonsx-region-threads");
            ui::toggle("set-regions", regions).on_click(move |_, _, cx| f((!regions).to_string(), cx))
        };
        let confirm = self.dash.confirm_delete;
        let d2 = dir.to_path_buf();
        let delete = ui::button("delete-server", if confirm { "Press again to delete it" } else { "Delete this server" }, Kind::Danger).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.dash.confirm_delete {
                this.dash.confirm_delete = true;
            } else {
                match servers::delete(&d2) {
                    Ok(()) => {
                        this.note = Some(("Server moved to the trash.".into(), Instant::now()));
                        this.go_home(cx);
                    }
                    Err(e) => this.note = Some((e, Instant::now())),
                }
            }
            cx.notify();
        }));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                ui::panel()
                    .child(ui::heading(18.).mb(px(4.)).child("Settings"))
                    .child(ui::setting_row(
                        if regions { "Region threads · HorizonsX" } else { "Region threads · off (Horizon)" },
                        "On: each part of the world runs at its own speed, for big player counts, and lag stays where it starts. Off: one TPS for the whole server, like Paper. Applies at the next restart.",
                        regions_c,
                    ))
                    .child(ui::setting_row("Game mode", "", mode_c))
                    .child(ui::setting_row("Difficulty", "", diff_c))
                    .child(ui::setting_row("Players", "How many can be on at once.", players_c))
                    .child(ui::setting_row("View distance", "Lower is lighter on your PC.", view_c))
                    .child(ui::setting_row("Simulation distance", "How far mobs, crops and redstone run.", sim_c))
                    .child(ui::setting_row("Friends only", "Only players on the friends list can join.", friends_c))
                    .child(ui::setting_row("Check Minecraft accounts", "Players sign in with Microsoft.", accounts_c))
                    .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).mt(px(10.)).child("Some changes apply at the next restart.")),
            )
            .child(ui::panel().child(ui::heading(18.).mb(px(8.)).child("Danger zone")).child(div().flex().child(delete)))
    }

    // ----- Files -------------------------------------------------------------------------------------

    pub(crate) fn load_files(&mut self, dir: &Path, rel: String) {
        self.dash.files_dir = Some(dir.to_path_buf());
        self.dash.files_rel = rel.clone();
        self.dash.files = None;
        self.dash.viewing = None;
        let tx = self.tx.clone();
        let d = dir.to_path_buf();
        std::thread::spawn(move || {
            let list = host::list_files(&d, &rel);
            let _ = tx.send(Ev::Files(d, rel, list));
        });
    }

    fn files(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        if self.dash.files_dir.as_deref() != Some(dir) {
            self.load_files(dir, String::new());
        }
        let rel = self.dash.files_rel.clone();
        let mut crumbs = div().flex().items_center().gap(px(6.)).mb(px(10.));
        let d0 = dir.to_path_buf();
        crumbs = crumbs.child(
            div().id("crumb-root").cursor_pointer().hover(|s| s.underline()).child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child("Server"))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.load_files(&d0, String::new());
                    cx.notify();
                })),
        );
        let mut acc = String::new();
        for (i, part) in rel.split('/').filter(|p| !p.is_empty()).enumerate() {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            let (target, d) = (acc.clone(), dir.to_path_buf());
            crumbs = crumbs.child(ui::text(14., FontWeight::NORMAL, ink::FAINT).child("/")).child(
                div().id(SharedString::from(format!("crumb-{i}"))).cursor_pointer().hover(|s| s.underline()).child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(part.to_string()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.load_files(&d, target.clone());
                        cx.notify();
                    })),
            );
        }
        let here = dir.join(&rel);
        crumbs = crumbs.child(div().flex_1()).child(ui::sized_button("files-open", "Open the folder", Kind::Ghost, 32., 13.).on_click(move |_, _, cx| cx.open_with_system(&here)));
        let mut panel = ui::panel().child(crumbs);
        if let Some((name, text)) = self.dash.viewing.clone() {
            let path = dir.join(&name);
            let body = text.unwrap_or_else(|e| e);
            // In colour with line numbers, as hosting panels show configs.
            let lines = div()
                .id("file-view")
                .flex()
                .flex_col()
                .max_h(px(560.))
                .overflow_y_scroll()
                .py(px(14.))
                .pr(px(16.))
                .pl(px(6.))
                .rounded(px(radius::ROW))
                .bg(ui::color(0x0b0a18).opacity(0.85))
                .border_1()
                .border_color(white(0.06))
                .child(crate::highlight::view(&name, &body, 2000));
            return panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .mb(px(10.))
                        .child(ui::text(15., FontWeight::SEMIBOLD, ink::FULL).child(name))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(ui::sized_button("file-edit", "Edit", Kind::Secondary, 32., 13.).on_click(move |_, _, cx| cx.open_with_system(&path)))
                                .child(ui::sized_button("file-close", "Close", Kind::Ghost, 32., 13.).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.dash.viewing = None;
                                    cx.notify();
                                }))),
                        ),
                )
                .child(lines);
        }
        match self.dash.files.clone() {
            None => panel = panel.child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child("Reading…")),
            Some(Err(e)) => panel = panel.child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child(e)),
            Some(Ok(entries)) => {
                for (i, e) in entries.into_iter().take(400).enumerate() {
                    let child = if rel.is_empty() { e.name.clone() } else { format!("{rel}/{}", e.name) };
                    let d = dir.to_path_buf();
                    let editable = !e.dir && host::editable(&e.name);
                    let row = div()
                        .id(SharedString::from(format!("file-{i}")))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(10.))
                        .py(px(9.))
                        .rounded(px(radius::ROW))
                        .cursor_pointer()
                        .hover(|s| s.bg(white(0.05)))
                        .child(ui::icon(if e.dir { "folder-01" } else if editable { "pencil-edit-02" } else { "package-01" }, 16., white(ink::BODY)))
                        .child(div().flex_1().min_w_0().child(ui::text(14., FontWeight::NORMAL, ink::FULL).truncate().child(e.name.clone())))
                        .child(ui::mono(12., ink::QUIET).child(if e.dir { "Folder".to_string() } else { size(e.bytes) }));
                    let row = if e.dir {
                        row.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.load_files(&d, child.clone());
                            cx.notify();
                        }))
                    } else if editable {
                        let tx = self.tx.clone();
                        row.on_click(move |_, _, _| {
                            let (d, child, tx) = (d.clone(), child.clone(), tx.clone());
                            std::thread::spawn(move || {
                                let text = host::read_text(&d, &child);
                                let _ = tx.send(Ev::FileText(child, text));
                            });
                        })
                    } else {
                        let path = d.join(&child);
                        row.on_click(move |_, _, cx| cx.open_with_system(&path))
                    };
                    panel = panel.child(row);
                }
            }
        }
        panel
    }
}

/// A graph: its name and current value, the samples as bars scaled to `max`.
fn graph(name: &'static str, value: String, samples: &[f32], max: f32) -> Div {
    const BARS: usize = 120;
    let start = samples.len().saturating_sub(BARS);
    let max = if max <= 0.0 { 1.0 } else { max };
    let mut bars = div().flex().items_end().gap(px(1.)).h(px(90.)).w_full();
    for _ in samples.len().min(BARS)..BARS {
        bars = bars.child(div().flex_1().h(px(1.)).bg(white(0.08)));
    }
    for v in &samples[start..] {
        let f = (v / max).clamp(0.02, 1.0);
        bars = bars.child(div().flex_1().h(gpui::relative(f)).rounded(px(1.)).bg(ui::color(ui::ACCENT).opacity(0.75)));
    }
    ui::panel()
        .flex_1()
        .min_w_0()
        .gap(px(12.))
        .child(div().flex().justify_between().child(ui::text(14., FontWeight::MEDIUM, ink::BODY).child(name)).child(ui::heading(18.).child(value)))
        .child(bars)
}

fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}
