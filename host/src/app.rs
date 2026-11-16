//! HorizonX's window: its state, the work done off the UI thread, and the frame every view sits in (the
//! pixel-sunset scene behind frosted plates, the sidebar beside the page).
use crate::frame::Page;
use crate::line_edit::{self, Key, LineEdit};
use crate::scene::Scene;
use crate::ui::{self, px, white};
use gpui::{div, prelude::*, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, KeyDownEvent, SharedString, Stateful, Window};
use horizon_launcher::host::{self, FileEntry, History, Playit};
use horizon_launcher::servers::{self, Backup, Entry, Status};
use horizon_launcher::stats::{self, HostSettings, Pc, Stats};
use horizon_theme::glass::{ink, radius};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// Where the window is.
#[derive(Clone, PartialEq)]
pub enum View {
    Page(Page),
    Wizard,
    /// The new server being made and started (the wizard's last screen).
    Setup,
    Server(PathBuf),
}

/// The text fields.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Field {
    Name,
    Seed,
    Command,
    AddPlayer,
    /// A message to the player chosen on the Players page.
    Message,
    /// Plugins: the Modrinth search.
    PluginSearch,
    /// Network: the name of a new network, the address players join with, the server list message.
    NetName,
    NetAddress,
    NetMotd,
}

/// What the background tells the window.
pub enum Ev {
    Statuses(Vec<(PathBuf, Status)>),
    Polled { dir: PathBuf, status: Status, logs: Option<(u64, Vec<String>)>, playit: Option<Playit>, since: u64 },
    Installed(PathBuf, Vec<String>),
    /// Every plugin file in a server, on or set aside.
    Plugins(PathBuf, Vec<(String, bool)>),
    /// The Java plugins (jars) in a server, and whether its Java layer is on.
    Java(PathBuf, Vec<host::JavaPlugin>, bool),
    /// A server's playit status (its public address).
    Playit(PathBuf, Option<Playit>),
    Backups(PathBuf, Vec<Backup>),
    Files(PathBuf, String, Result<Vec<FileEntry>, String>),
    FileText(String, Result<String, String>),
    Worlds(Vec<(String, PathBuf)>),
    Step(usize, Step),
    JobLine(String),
    Created(Result<PathBuf, String>),
    Note(String),
    Done(PathBuf, Result<String, String>),
    /// A server's settings read (after a change, with what the change said).
    Config(PathBuf, Option<serde_json::Value>, Option<String>),
    /// A server's settings for the pages that list every server (Schedules).
    Settings(PathBuf, serde_json::Value),
    /// A server's players and players-over-time, read or just recorded.
    Stats(PathBuf, Stats),
    /// A player's face (by UUID), from their skin.
    Face(String, Option<Vec<[u8; 4]>>),
    /// Modrinth's plugins for a search: (query, offset, the page).
    Market(String, usize, Result<horizon_launcher::plugin_market::Results, String>),
    /// A Modrinth plugin put into servers: (servers, the plugin's title, what each install said).
    MarketDone(Vec<PathBuf>, String, Vec<Result<horizon_launcher::plugin_market::Installed, String>>),
    /// The network's state, read in the background.
    Net(Box<crate::network_page::NetSnapshot>),
    /// A network action finished, with what to tell.
    NetDone(Result<String, String>),
}

/// A setup step's state.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    Waiting,
    Running,
    Done,
    Failed,
}

pub struct HostApp {
    pub scene: Entity<Scene>,
    pub view: View,
    pub list: Vec<Entry>,
    pub status: HashMap<PathBuf, Status>,
    pub playit: HashMap<PathBuf, Playit>,
    pub history: HashMap<PathBuf, History>,
    pub installed: HashMap<PathBuf, Vec<String>>,
    pub plugin_files: HashMap<PathBuf, Vec<(String, bool)>>,
    /// Each server's Java plugins, and whether its Java layer is on.
    pub java: HashMap<PathBuf, (Vec<host::JavaPlugin>, bool)>,
    /// What happened lately on the servers (newest last): time, line, tone. The dashboard's feed.
    pub activity: VecDeque<(String, String, ui::Tone)>,
    pub backups: HashMap<PathBuf, Vec<Backup>>,
    /// Each server's settings, for the Schedules page.
    pub configs: HashMap<PathBuf, serde_json::Value>,
    pub stats: HashMap<PathBuf, Stats>,
    pub faces: HashMap<String, Option<Vec<[u8; 4]>>>,
    pub pc: Option<Pc>,
    pub settings: HostSettings,
    pub catalog: Vec<host::Plugin>,
    pub wiz: crate::wizard::Wizard,
    pub setup: crate::wizard::Setup,
    pub dash: crate::dashboard::Dash,
    pub pages: crate::pages::PageState,
    pub edits: HashMap<Field, LineEdit>,
    pub focus: HashMap<Field, FocusHandle>,
    pub focused: Option<Field>,
    pub tx: Sender<Ev>,
    rx: Receiver<Ev>,
    polling: bool,
    polled: Option<Instant>,
    sampled: Option<Instant>,
    pc_read: Option<Instant>,
    update_checked: Option<Instant>,
    /// The updater's state last drawn (`update::generation`).
    update_seen: u64,
    restored: bool,
    pub copied: Option<(String, Instant)>,
    pub note: Option<(String, Instant)>,
    /// Actions under way on a server (Starting, Stopping...).
    pub busy: HashMap<PathBuf, String>,
    /// The demo's scripted tour (`HORIZON_HOST_DEMO=1`).
    pub tour: Option<crate::demo::Tour>,
    pub root: FocusHandle,
}

impl HostApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = channel();
        let root = cx.focus_handle();
        window.focus(&root, cx);
        // The window's own clock: what the background sent, and the next poll.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                break;
            }
        })
        .detach();
        let list = servers::list();
        // What was remembered: each server's players over time.
        {
            let (tx, dirs) = (tx.clone(), list.iter().map(|e| e.dir.clone()).collect::<Vec<_>>());
            std::thread::spawn(move || {
                for d in dirs {
                    let s = stats::load(&d);
                    let _ = tx.send(Ev::Stats(d, s));
                }
            });
        }
        let mut app = Self {
            scene: cx.new(|_| Scene::new()),
            view: View::Page(Page::Dashboard),
            list,
            status: HashMap::new(),
            playit: HashMap::new(),
            history: HashMap::new(),
            installed: HashMap::new(),
            plugin_files: HashMap::new(),
            java: HashMap::new(),
            activity: VecDeque::new(),
            backups: HashMap::new(),
            configs: HashMap::new(),
            stats: HashMap::new(),
            faces: HashMap::new(),
            pc: stats::pc(),
            settings: stats::settings(),
            catalog: host::catalog(),
            wiz: crate::wizard::Wizard::default(),
            setup: crate::wizard::Setup::default(),
            dash: crate::dashboard::Dash::default(),
            pages: crate::pages::PageState::default(),
            edits: HashMap::new(),
            focus: HashMap::new(),
            focused: None,
            tx,
            rx,
            polling: false,
            polled: None,
            sampled: None,
            pc_read: None,
            update_checked: None,
            update_seen: 0,
            restored: false,
            copied: None,
            note: crate::update::settle().then(|| (format!("HorizonX updated to {}.", env!("CARGO_PKG_VERSION")), Instant::now())),
            busy: HashMap::new(),
            tour: crate::demo::on().then(crate::demo::Tour::new),
            root,
        };
        // Screenshot switches: `HORIZON_HOST_VIEW=wizard|servers|players|plugins|backups|schedules|settings`,
        // `HORIZON_HOST_STEP=2`, `HORIZON_HOST_SERVER=name` + `HORIZON_HOST_SECTION=players`.
        match std::env::var("HORIZON_HOST_VIEW").as_deref() {
            Ok("wizard") => {
                app.open_wizard(cx);
                if let Some(n) = std::env::var("HORIZON_HOST_STEP").ok().and_then(|v| v.parse::<usize>().ok()) {
                    app.wiz.step = n.min(crate::wizard::STEPS.len() - 1);
                }
            }
            Ok(name) => {
                let page = match name {
                    "servers" => Some(Page::Servers),
                    "players" => Some(Page::Players),
                    "plugins" => Some(Page::Plugins),
                    "network" => Some(Page::Network),
                    "backups" => Some(Page::Backups),
                    "schedules" => Some(Page::Schedules),
                    "settings" => Some(Page::Settings),
                    "settings-ai" => {
                        app.pages.settings_tab = crate::pages::SettingsTab::Ai;
                        Some(Page::Settings)
                    }
                    _ => None,
                };
                if let Some(p) = page {
                    app.open_page(p, cx);
                }
            }
            Err(_) => {}
        }
        // `HORIZON_HOST_TAB=everyone|operators|banned`: the Players page's tab, for screenshots.
        match std::env::var("HORIZON_HOST_TAB").as_deref() {
            Ok("everyone") => app.pages.players_tab = crate::pages::PlayersTab::Everyone,
            Ok("operators") => app.pages.players_tab = crate::pages::PlayersTab::Operators,
            Ok("banned") => app.pages.players_tab = crate::pages::PlayersTab::Banned,
            _ => {}
        }
        // `HORIZON_HOST_PICK=name`: the server the pages show (Plugins, Backups, the players chart), for screenshots.
        if let Ok(name) = std::env::var("HORIZON_HOST_PICK") {
            if let Some(e) = app.list.iter().find(|e| e.name == name).cloned() {
                app.pages.plugins_server = Some(e.dir.clone());
                app.pages.chart_server = Some(e.dir.clone());
                app.load_details(&e.dir);
            }
        }
        // `HORIZON_HOST_SEARCH=shop`: that Modrinth search on the Plugins page, for screenshots.
        if let Ok(q) = std::env::var("HORIZON_HOST_SEARCH") {
            app.set_text(Field::PluginSearch, &q);
            app.market_search(0);
        }
        if let Ok(name) = std::env::var("HORIZON_HOST_SERVER") {
            if let Some(e) = app.list.iter().find(|e| e.name == name).cloned() {
                app.open_server(e.dir.clone(), cx);
                if let Ok(sec) = std::env::var("HORIZON_HOST_SECTION") {
                    app.dash.section = crate::dashboard::Section::from_name(&sec);
                }
                // `HORIZON_HOST_FILE=plugins/Essentials/config.yml`: that file open in Files, for screenshots.
                if let Ok(file) = std::env::var("HORIZON_HOST_FILE") {
                    let folder = file.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default();
                    app.open_plugin_file(&e.dir, &folder, Some(file), cx);
                }
            }
        }
        app
    }

    // ----- The clock ----------------------------------------------------------------------------

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            self.event(ev, cx);
            changed = true;
        }
        if self.note.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6)) {
            self.note = None;
            changed = true;
        }
        if self.copied.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(2)) {
            self.copied = None;
            changed = true;
        }
        if self.pc_read.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.pc_read = Some(Instant::now());
            self.pc = stats::pc().or(self.pc);
            changed = true;
        }
        if crate::update::generation() != self.update_seen {
            self.update_seen = crate::update::generation();
            changed = true;
        }
        // A newer HorizonX waiting beside this one: swap to it (not in the middle of setting a server up).
        if self.update_checked.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) && !matches!(self.view, View::Wizard | View::Setup) {
            self.update_checked = Some(Instant::now());
            if crate::update::pending() {
                match crate::update::apply() {
                    Ok(()) => {
                        cx.quit();
                        return;
                    }
                    Err(e) => self.note = Some((format!("Update didn't apply: {e}"), Instant::now())),
                }
            }
        }
        self.poll();
        self.sample();
        if self.dash.schedule_checked.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
            self.dash.schedule_checked = Some(Instant::now());
            self.run_schedules();
        }
        if changed {
            cx.notify();
        }
    }

    /// Every server's state for the sidebar and the pages, and the open server's log and address.
    fn poll(&mut self) {
        let every = match self.view {
            View::Server(_) | View::Setup => Duration::from_secs(1),
            _ => Duration::from_secs(2),
        };
        if self.polling || self.polled.is_some_and(|t| t.elapsed() < every) {
            return;
        }
        self.polled = Some(Instant::now());
        self.polling = true;
        let tx = self.tx.clone();
        let open = match &self.view {
            View::Server(d) => Some(d.clone()),
            View::Setup => self.setup.dir.clone(),
            _ => None,
        };
        let since = self.dash.log_next;
        let fresh = self.dash.log_dir != open;
        let dirs: Vec<PathBuf> = self.list.iter().map(|e| e.dir.clone()).filter(|d| Some(d) != open.as_ref()).collect();
        std::thread::spawn(move || {
            // Every other server, with its address for the dashboard.
            let all: Vec<(PathBuf, Status)> = dirs
                .into_iter()
                .map(|d| {
                    let _ = tx.send(Ev::Playit(d.clone(), host::playit(&d)));
                    let s = full_status(&d).0;
                    (d, s)
                })
                .collect();
            if let Some(dir) = open {
                let (status, api) = full_status(&dir);
                let from = if fresh { 0 } else { since };
                let logs = api.and_then(|a| a.logs(from));
                let playit = host::playit(&dir);
                let _ = tx.send(Ev::Polled { dir, status, logs, playit, since: from });
            }
            let _ = tx.send(Ev::Statuses(all));
        });
    }

    /// Once a minute: each running server's players into its stats file (the charts and the Players page).
    fn sample(&mut self) {
        if self.sampled.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) || self.status.is_empty() {
            return;
        }
        self.sampled = Some(Instant::now());
        let looks: Vec<(PathBuf, Status)> = self.list.iter().filter_map(|e| self.status.get(&e.dir).map(|s| (e.dir.clone(), s.clone()))).collect();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            for (d, s) in looks {
                let recorded = stats::record(&d, &s);
                let _ = tx.send(Ev::Stats(d, recorded));
            }
        });
    }

    fn run_schedules(&mut self) {
        let running: Vec<(PathBuf, Option<u64>, usize)> =
            self.status.iter().filter(|(_, s)| s.running()).map(|(d, s)| (d.clone(), s.uptime, s.players.len())).collect();
        if running.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let warn = self.settings.warn_before_restart;
        std::thread::spawn(move || {
            use chrono::Timelike;
            let now = chrono::Local::now();
            for (d, uptime, players) in running {
                // Five minutes before a scheduled restart, tell whoever is on.
                if warn && players > 0 && host::restart_soon(&host::schedule(&d), now.hour(), now.minute(), uptime) {
                    let _ = host::command(&d, "say The server restarts in 5 minutes.");
                }
                for line in host::run_schedule(&d, now.hour(), uptime) {
                    let _ = tx.send(Ev::Note(line));
                }
            }
        });
    }

    fn event(&mut self, ev: Ev, cx: &mut Context<Self>) {
        match ev {
            Ev::Statuses(all) => {
                self.polling = false;
                for (d, s) in all {
                    self.history.entry(d.clone()).or_default().push(&s);
                    self.note_changes(&d, &s);
                    self.status.insert(d, s);
                }
                self.restore_and_remember();
            }
            Ev::Polled { dir, status, logs, playit, since } => {
                self.history.entry(dir.clone()).or_default().push(&status);
                // Lines asked for after another server's (or none): only a read from the start begins this one's
                // log; anything else is read again from 0 next time.
                let logs = logs.filter(|_| self.dash.log_dir.as_ref() == Some(&dir) || since == 0);
                if let Some((next, lines)) = logs {
                    if self.dash.log_dir.as_ref() != Some(&dir) {
                        self.dash.log.clear();
                        self.dash.log_dir = Some(dir.clone());
                    }
                    for l in lines {
                        // The engine's periodic region report is noise in a console.
                        if l.starts_with("HorizonsX region threads:") {
                            continue;
                        }
                        self.dash.log.push_back(crate::demo::mask_ips(&l));
                    }
                    while self.dash.log.len() > 1500 {
                        self.dash.log.pop_front();
                    }
                    self.dash.log_next = next;
                }
                match playit {
                    Some(p) => self.playit.insert(dir.clone(), p),
                    None => self.playit.remove(&dir),
                };
                self.note_changes(&dir, &status);
                self.status.insert(dir.clone(), status);
                self.setup_progress(&dir);
            }
            Ev::Installed(d, l) => {
                self.installed.insert(d, l);
            }
            Ev::Plugins(d, l) => {
                self.plugin_files.insert(d, l);
            }
            Ev::Java(d, list, layer) => {
                self.java.insert(d, (list, layer));
            }
            Ev::Playit(d, p) => {
                match p {
                    Some(p) => self.playit.insert(d, p),
                    None => self.playit.remove(&d),
                };
            }
            Ev::Backups(d, b) => {
                self.backups.insert(d, b);
            }
            Ev::Files(d, rel, list) => {
                if self.dash.files_dir.as_ref() == Some(&d) && self.dash.files_rel == rel {
                    self.dash.files = Some(list);
                }
            }
            Ev::FileText(rel, text) => self.dash.viewing = Some((rel, text)),
            Ev::Worlds(w) => self.wiz.worlds = w,
            Ev::Step(i, s) => {
                if let Some(step) = self.setup.steps.get_mut(i) {
                    step.1 = s;
                }
            }
            Ev::JobLine(l) => {
                self.setup.line = Some(l);
            }
            Ev::Created(r) => match r {
                Ok(dir) => {
                    self.list = servers::list();
                    self.setup.dir = Some(dir);
                }
                Err(e) => self.setup.error = Some(e),
            },
            Ev::Note(n) => self.note = Some((n, Instant::now())),
            Ev::Market(q, offset, r) => self.market_event(q, offset, r),
            Ev::MarketDone(dirs, title, results) => self.market_done(dirs, title, results),
            Ev::Net(s) => self.net_event(s),
            Ev::NetDone(r) => self.net_done(r),
            Ev::Config(dir, config, note) => {
                self.dash.config_loading = false;
                if let Some(c) = config {
                    self.configs.insert(dir.clone(), c.clone());
                    self.dash.config = Some((dir, c));
                }
                if let Some(n) = note {
                    self.note = Some((n, Instant::now()));
                }
            }
            Ev::Settings(dir, c) => {
                self.configs.insert(dir, c);
            }
            Ev::Done(dir, r) => {
                self.busy.remove(&dir);
                if let Err(e) = r {
                    self.note = Some((e, Instant::now()));
                }
                self.load_details(&dir);
                if self.page() == Some(Page::Backups) {
                    self.load_backups(&dir);
                }
            }
            Ev::Stats(d, s) => {
                self.stats.insert(d, s);
            }
            Ev::Face(uuid, face) => {
                self.faces.insert(uuid, face);
            }
        }
        cx.notify();
    }

    /// Starts the servers that were running when HorizonX last closed (once, when it opens), and remembers
    /// which run now.
    fn restore_and_remember(&mut self) {
        let running: Vec<PathBuf> = self.list.iter().filter(|e| self.status.get(&e.dir).is_some_and(|s| s.running())).map(|e| e.dir.clone()).collect();
        if !self.restored {
            self.restored = true;
            if self.settings.restore_servers {
                for d in self.settings.were_running.clone() {
                    if !running.contains(&d) && self.list.iter().any(|e| e.dir == d) {
                        self.busy.insert(d.clone(), "Starting".into());
                        let tx = self.tx.clone();
                        std::thread::spawn(move || {
                            let r = servers::run_streamed(&d, &["start"], &|_| {}).map(|_| String::new()).map_err(|e| e.message);
                            let _ = tx.send(Ev::Done(d, r));
                        });
                    }
                }
            }
            return;
        }
        if self.busy.is_empty() && running != self.settings.were_running {
            self.settings.were_running = running;
            let _ = stats::save_settings(&self.settings);
        }
    }

    // ----- Servers --------------------------------------------------------------------------------

    pub fn go_home(&mut self, cx: &mut Context<Self>) {
        self.open_page(Page::Dashboard, cx);
    }

    pub fn open_server(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.view = View::Server(dir.clone());
        self.pages.menu = None;
        // The same server's log carries over (from Setup); another's starts again.
        let old = std::mem::take(&mut self.dash);
        if old.log_dir.as_ref() == Some(&dir) {
            self.dash.log = old.log;
            self.dash.log_next = old.log_next;
            self.dash.log_dir = old.log_dir;
        }
        self.polled = None;
        // Older servers get HorizonsX and the playit plugin too.
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let notes = host::adopt(&dir);
            if !notes.is_empty() {
                let _ = tx.send(Ev::Note(notes.join(" · ")));
            }
            send_details(&tx, &dir);
        });
        cx.notify();
    }

    pub fn load_details(&self, dir: &std::path::Path) {
        let tx = self.tx.clone();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || send_details(&tx, &dir));
    }

    pub fn load_backups(&self, dir: &std::path::Path) {
        let tx = self.tx.clone();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(Ev::Backups(dir.clone(), servers::backups(&dir).unwrap_or_default()));
        });
    }

    /// Runs a horizon-server command on a server in the background (`start`, `stop`, `backup`...).
    pub fn server_action(&mut self, dir: PathBuf, label: &str, args: Vec<String>, cx: &mut Context<Self>) {
        self.busy.insert(dir.clone(), label.to_string());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            let r = servers::run_streamed(&dir, &a, &|_| {}).map(|_| String::new()).map_err(|e| e.message);
            let _ = tx.send(Ev::Done(dir, r));
        });
        self.polled = None;
        cx.notify();
    }

    /// Start: `start` stays attached while the server runs, so its thread lives as long; the status says when
    /// it's up.
    pub fn start(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.server_action(dir, "Starting", vec!["start".into()], cx);
    }

    pub fn stop(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.server_action(dir, "Stopping", vec!["stop".into()], cx);
    }

    pub fn restart(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.server_action(dir, "Restarting", vec!["restart".into()], cx);
    }

    /// A console command on a running server, with what it answered in a note.
    pub fn run_console(&mut self, dir: &std::path::Path, line: String) {
        let tx = self.tx.clone();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let msg = match host::command(&dir, &line) {
                Ok(out) => out.join(" "),
                Err(e) => e.message,
            };
            if !msg.trim().is_empty() {
                let _ = tx.send(Ev::Note(msg));
            }
        });
    }

    pub fn status_of(&self, dir: &std::path::Path) -> Status {
        self.status.get(dir).cloned().unwrap_or_else(Status::stopped)
    }

    pub fn name_of(&self, dir: &std::path::Path) -> String {
        self.list.iter().find(|e| e.dir == dir).map(|e| e.name.clone()).unwrap_or_default()
    }

    /// The address friends type: playit's once it's up, else this network's.
    pub fn lan_address(&self, dir: &std::path::Path) -> String {
        let port = self
            .configs
            .get(dir)
            .cloned()
            .or_else(|| servers::config(dir))
            .and_then(|c| c.pointer("/server/port").and_then(|v| v.as_u64()))
            .unwrap_or(25565);
        let address = match servers::lan_address() {
            Some(ip) if port == 25565 => ip.to_string(),
            Some(ip) => format!("{ip}:{port}"),
            None => format!("localhost:{port}"),
        };
        crate::demo::mask_ips(&address)
    }

    /// The address friends type: playit's when it's up, else this network's.
    pub fn share_address(&self, dir: &std::path::Path) -> String {
        self.playit.get(dir).filter(|p| p.online()).and_then(|p| p.address.clone()).map(|a| crate::demo::mask_ips(&a)).unwrap_or_else(|| self.lan_address(dir))
    }

    pub fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        self.copied = Some((text, Instant::now()));
        cx.notify();
    }

    pub fn copied(&self, text: &str) -> bool {
        self.copied.as_ref().is_some_and(|(t, _)| t == text)
    }

    /// A player's face: asked for once, drawn when it arrives.
    pub fn face(&mut self, uuid: &str) -> Option<Vec<[u8; 4]>> {
        if let Some(f) = self.faces.get(uuid) {
            return f.clone();
        }
        self.faces.insert(uuid.to_string(), None);
        let (tx, uuid) = (self.tx.clone(), uuid.to_string());
        std::thread::spawn(move || {
            let resources = servers::game_files(&servers::runtime_root(), "26.3");
            let face = horizon_launcher::heads::default_face(&resources, &uuid);
            let _ = tx.send(Ev::Face(uuid, face));
        });
        None
    }

    // ----- Text fields -----------------------------------------------------------------------------

    pub fn edit(&mut self, f: Field) -> &mut LineEdit {
        self.edits.entry(f).or_insert_with(|| LineEdit::new(String::new(), 64, 40))
    }

    pub fn text(&self, f: Field) -> String {
        self.edits.get(&f).map(|e| e.text().to_string()).unwrap_or_default()
    }

    pub fn set_text(&mut self, f: Field, t: &str) {
        self.edit(f).set(t.to_string());
    }

    /// A text field: its box, the text with caret and selection, the keyboard and the mouse.
    pub fn field(&mut self, f: Field, placeholder: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let handle = self.focus.entry(f).or_insert_with(|| cx.focus_handle()).clone();
        let focused = self.focused == Some(f);
        let content = line_edit::content(self.edit(f), focused, false, false, SharedString::from(placeholder));
        let h2 = handle.clone();
        let el = ui::field_box(focused)
            .id(SharedString::from(format!("field-{f:?}")))
            .track_focus(&handle)
            .w_full()
            .cursor_text()
            .child(content)
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                window.focus(&h2, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                match line_edit::handle(this.edit(f), ev, cx) {
                    Key::Enter => this.field_enter(f, cx),
                    Key::Escape => window.focus(&this.root, cx),
                    Key::Pass => return,
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let el = crate::demo::mark(format!("field-{f:?}"), el.relative());
        line_edit::mouse(el, cx, move |this: &mut HostApp| this.edit(f))
    }

    fn field_enter(&mut self, f: Field, cx: &mut Context<Self>) {
        match f {
            Field::Command => self.send_command(cx),
            Field::AddPlayer => self.add_player(cx),
            Field::Message => self.send_message(cx),
            Field::PluginSearch => self.market_search(0),
            Field::NetAddress | Field::NetMotd => self.net_save_fields(),
            Field::Name | Field::Seed | Field::NetName => {}
        }
    }

    // ----- The frame ---------------------------------------------------------------------------------

    fn toast(&self) -> Option<Div> {
        let (n, _) = self.note.as_ref()?;
        Some(
            div().absolute().bottom(px(24.)).left_0().right_0().flex().justify_center().child(
                ui::plate(radius::ROW)
                    .relative()
                    .px(px(18.))
                    .py(px(12.))
                    .max_w(px(640.))
                    .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(n.clone())),
            ),
        )
    }
}

/// A server's status as its supervisor reports it, with the engine's memory when the supervisor doesn't.
fn full_status(dir: &std::path::Path) -> (Status, Option<servers::Api>) {
    let api = servers::Api::open(dir);
    let raw = api.as_ref().and_then(|a| a.call("GET", "/api/v1/status", None).ok());
    let mut s = raw.as_ref().map(Status::parse).unwrap_or_else(Status::stopped);
    // The engine's own memory when its process is known (the supervisor's figure can be its working set, which
    // Windows trims to almost nothing while a server is paused); else the supervisor's, where it has one.
    let engine = raw.as_ref().and_then(|v| v["engine_pid"].as_u64()).and_then(|pid| stats::process_memory(pid as u32));
    s.ram_bytes = engine.or(s.ram_bytes.filter(|b| *b > 0));
    (s, api)
}

/// A server's plugins, Java plugins and backups, read in the background.
fn send_details(tx: &Sender<Ev>, dir: &std::path::Path) {
    let dir = dir.to_path_buf();
    let _ = tx.send(Ev::Installed(dir.clone(), host::installed_plugins(&dir)));
    let _ = tx.send(Ev::Plugins(dir.clone(), host::plugin_files(&dir)));
    let _ = tx.send(Ev::Java(dir.clone(), host::java_plugins(&dir), host::java_layer_on(&dir)));
    let _ = tx.send(Ev::Backups(dir.clone(), servers::backups(&dir).unwrap_or_default()));
}

impl Render for HostApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.focused = self.focus.iter().find(|(_, h)| h.is_focused(window)).map(|(f, _)| *f);
        ui::set_scale(f32::from(window.viewport_size().width));
        self.tour_step(window, cx);
        let look = horizon_theme::glass::Glass::default_look();
        crate::glass::set_look(look);
        self.scene.update(cx, |scene, cx| scene.set_look(None, look.blur, cx));
        crate::glass::begin_build(window);
        let body = match self.view.clone() {
            View::Page(p) => self.page_view(p, cx),
            View::Wizard => self.wizard(cx),
            View::Setup => self.setup_view(cx),
            View::Server(dir) => self.dashboard(&dir, window, cx),
        };
        // The setup wizard (and its setup screen) has the window to itself: no sidebar while setting up.
        let setting_up = matches!(self.view, View::Wizard | View::Setup);
        let sidebar = (!setting_up).then(|| self.sidebar(cx));
        let menu_open = self.pages.menu.is_some();
        div()
            .id("root")
            .track_focus(&self.root)
            .relative()
            .size_full()
            .bg(ui::color(horizon_theme::glass::PAGE))
            .font_family(ui::FONT)
            .text_color(white(ink::FULL))
            .child(div().absolute().top_0().left_0().size_full().child(self.scene.clone()))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .gap(px(16.))
                    .p(px(16.))
                    .children(sidebar)
                    .child(div().relative().flex_1().min_w_0().h_full().flex().flex_col().child(body)),
            )
            // A click anywhere else closes an open dropdown.
            .when(menu_open, |d| {
                d.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                        this.pages.menu = None;
                        cx.notify();
                    }),
                )
            })
            .children(self.toast())
            .children(self.tour_overlay())
    }
}

/// Plugins chosen in the wizard, playit always among them.
pub fn with_playit(chosen: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = chosen.clone();
    out.insert(host::PLAYIT.to_string());
    out
}
