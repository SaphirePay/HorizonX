//! HorizonX's parts of the Servers page, in the glass look: the public address (the playit
//! plugin's claim link, then the address to copy), and the tabs the launcher doesn't have: Plugins,
//! Files, Performance and Schedule. The work is in `horizon_launcher::host`; this draws it.
use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use horizon_launcher::host::{self, FileEntry, History, Playit, Schedule};
use horizon_theme::glass::{fill, ink, radius};

/// What the background sends the page.
pub(crate) enum HostEvent {
    Playit(PathBuf, Option<Playit>),
    Installed(PathBuf, Vec<String>),
    Files(PathBuf, String, Result<Vec<FileEntry>, String>),
    FileText(String, Result<String, String>),
    Notes(Vec<String>),
}

/// The page's own state for these parts.
#[derive(Default)]
pub(crate) struct HostUi {
    pub history: HashMap<PathBuf, History>,
    pub playit: HashMap<PathBuf, Playit>,
    pub catalog: Option<Vec<host::Plugin>>,
    pub installed: HashMap<PathBuf, Vec<String>>,
    /// The Files tab: the folder shown (relative to the server), its entries, a file opened.
    pub files_dir: Option<PathBuf>,
    pub files_rel: String,
    pub files: Option<Result<Vec<FileEntry>, String>>,
    pub viewing: Option<(String, Result<String, String>)>,
    pub schedule: Option<(PathBuf, Schedule)>,
    pub schedule_checked: Option<Instant>,
    pub copied: Option<Instant>,
}

/// Backup intervals offered, in hours (0 = off).
const BACKUP_HOURS: [(u32, &str); 6] = [(0, "Off"), (1, "1 h"), (3, "3 h"), (6, "6 h"), (12, "12 h"), (24, "Daily")];
/// Daily restart hours offered (24 = off).
const RESTART_HOURS: [(u32, &str); 6] = [(24, "Off"), (3, "3:00"), (4, "4:00"), (5, "5:00"), (6, "6:00"), (12, "12:00")];

impl Launcher {
    /// After a server is opened: make it HorizonX's (HorizonsX, the playit plugin), then read
    /// its plugins and schedule.
    pub(super) fn host_open(&mut self, dir: &Path) {
        let tx = self.servers.tx.clone();
        let dir = dir.to_path_buf();
        self.servers.host.schedule = Some((dir.clone(), host::schedule(&dir)));
        self.servers.host.files_dir = None;
        self.servers.host.files = None;
        self.servers.host.viewing = None;
        self.servers.host.files_rel.clear();
        std::thread::spawn(move || {
            let notes = host::adopt(&dir);
            if !notes.is_empty() {
                let _ = tx.send(Event::Host(HostEvent::Notes(notes)));
            }
            let _ = tx.send(Event::Host(HostEvent::Installed(dir.clone(), host::installed_plugins(&dir))));
            let _ = tx.send(Event::Host(HostEvent::Playit(dir.clone(), host::playit(&dir))));
        });
    }

    /// Every poll of the open server: its playit status (beside the status and log).
    pub(super) fn host_poll(tx: &Sender<Event>, dir: &Path) {
        let _ = tx.send(Event::Host(HostEvent::Playit(dir.to_path_buf(), host::playit(dir))));
    }

    /// Once a minute: the schedules of the servers that run.
    pub(super) fn host_tick(&mut self) {
        if self.servers.host.schedule_checked.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
            return;
        }
        self.servers.host.schedule_checked = Some(Instant::now());
        let running: Vec<PathBuf> = self
            .servers
            .statuses
            .iter()
            .filter(|(_, s)| s.running())
            .map(|(d, _)| d.clone())
            .collect();
        if running.is_empty() {
            return;
        }
        let tx = self.servers.tx.clone();
        std::thread::spawn(move || {
            use chrono::Timelike;
            let hour = chrono::Local::now().hour();
            // The launcher-style page has no uptimes at hand: its daily restart and backups only.
            let notes: Vec<String> = running.iter().flat_map(|d| host::run_schedule(d, hour, None)).collect();
            if !notes.is_empty() {
                let _ = tx.send(Event::Host(HostEvent::Notes(notes)));
            }
        });
    }

    pub(super) fn host_event(&mut self, event: HostEvent) -> bool {
        let h = &mut self.servers.host;
        match event {
            HostEvent::Playit(dir, p) => {
                let changed = h.playit.get(&dir) != p.as_ref();
                match p {
                    Some(p) => h.playit.insert(dir, p),
                    None => h.playit.remove(&dir),
                };
                changed
            }
            HostEvent::Installed(dir, list) => {
                h.installed.insert(dir, list);
                true
            }
            HostEvent::Files(dir, rel, list) => {
                if h.files_dir.as_ref() == Some(&dir) && h.files_rel == rel {
                    h.files = Some(list);
                }
                true
            }
            HostEvent::FileText(rel, text) => {
                h.viewing = Some((rel, text));
                true
            }
            HostEvent::Notes(notes) => {
                self.servers.note = Some(notes.join(" · "));
                true
            }
        }
    }

    // ----- The public address -----------------------------------------------------------------

    /// "Anywhere on the internet · playit.gg": the address with Copy once the tunnel is up, the
    /// link to set it up while it waits for one, and what it's doing in between.
    pub(super) fn host_public_address(&mut self, dir: &Path, status: &Status, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let p = self.servers.host.playit.get(dir).cloned();
        let heading = ui::card_heading(tr("Anywhere on the internet · playit.gg"));
        let mut col = div().flex().flex_col().child(heading);
        let note = |s: &str| ui::card_note(tr(s).to_string()).mt(px(4.));
        if !status.running() {
            return col
                .child(ui::text(15., FontWeight::MEDIUM, ink::QUIET).mt(px(4.)).child(tr("Starts with the server")))
                .child(note("Start the server and it gets an address anyone can join from, with nothing to change on your router."));
        }
        match p {
            Some(p) if p.online() => {
                let address = p.address.clone().unwrap_or_default();
                let copied = self.servers.host.copied.is_some_and(|at| at.elapsed() < Duration::from_secs(2));
                let text = address.clone();
                let copy = ui::icon_button("public-copy", if copied { "tick-02" } else { "copy-01" }, 36., 17., 10., false, &t)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        copy_to_clipboard(cx, gpui::ClipboardItem::new_string(text.clone()));
                        this.servers.host.copied = Some(Instant::now());
                        cx.notify();
                    }));
                col = col
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .min_w_0()
                                    .child(ui::dot(Tone::Live))
                                    .child(ui::text(22., FontWeight::MEDIUM, ink::FULL).min_w_0().truncate().child(address)),
                            )
                            .child(copy),
                    )
                    .child(note("Friends anywhere: Multiplayer, Add server, paste. No port forwarding."));
                if p.guest == Some(true) {
                    col = col.child(note("playit.gg is on a guest account: claim it on playit.gg to keep this address."));
                } else if p.email_verified == Some(false) {
                    col = col.child(note("Verify your email on playit.gg to keep this address."));
                }
                if let Some(n) = p.notice.filter(|n| !n.is_empty()) {
                    col = col.child(note(&n));
                }
                col
            }
            Some(p) if p.waiting_for_claim() => {
                let url = p.claim_url.clone().unwrap_or_default();
                col.child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .child(
                            ui::solid("playit-claim", tr("Get a public address"), 36., 14., &t)
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        ),
                )
                .child(note("Opens playit.gg: sign in or continue as a guest and accept. It's free, and done once per server."))
            }
            Some(p) => col
                .child(
                    div()
                        .mt(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::dot(if p.state == 13 || p.state == 5 || p.state == 15 { Tone::Warn } else { Tone::Busy }))
                        .child(ui::text(15., FontWeight::MEDIUM, ink::BODY).child(capitalised(&p.state_text))),
                )
                .child(note(match p.state {
                    15 => "The playit.gg key was refused. Reset it in the console: playit agent reset.",
                    5 => "The claim was declined. Restart the server to get a new link.",
                    _ => "Setting up the tunnel: a few seconds.",
                })),
            None => col
                .child(ui::text(15., FontWeight::MEDIUM, ink::QUIET).mt(px(4.)).child(tr("Not available")))
                .child(note(if host::plugin_installed(dir, host::PLAYIT) {
                    "The playit plugin hasn't started yet: give it a moment."
                } else {
                    "The playit plugin isn't in this server. Add it on the Plugins tab."
                })),
        }
    }

    // ----- Plugins ------------------------------------------------------------------------------

    pub(super) fn host_plugins_tab(&mut self, dir: &Path, status: &Status, _cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let catalog = self.servers.host.catalog.get_or_insert_with(host::catalog).clone();
        let installed = self.servers.host.installed.get(dir).cloned().unwrap_or_default();
        let mut list = div().flex().flex_col();
        let head = ui::list_head(if status.running() {
            tr("Added plugins start at once. Removing one takes effect after a restart.")
        } else {
            tr("Horizon's plugins, made for HorizonsX.")
        });
        list = list.child(head);
        if catalog.is_empty() {
            return list.child(ui::empty().child(tr("No plugins came with HorizonX.")));
        }
        for p in catalog {
            let on = installed.contains(&p.file);
            let file = p.file.clone();
            let d = dir.to_path_buf();
            let tx = self.servers.tx.clone();
            let action = ui::quiet(
                SharedString::from(format!("plugin-{}", p.file)),
                if on { tr("Remove") } else { tr("Add") },
                32.,
                &t,
            )
            .on_click(move |_, _, _| {
                let (d, file, tx) = (d.clone(), file.clone(), tx.clone());
                std::thread::spawn(move || {
                    let result = if on { host::remove_plugin(&d, &file) } else { host::install_plugin(&d, &file) };
                    if let Err(e) = result {
                        let _ = tx.send(Event::Host(HostEvent::Notes(vec![e])));
                    }
                    let _ = tx.send(Event::Host(HostEvent::Installed(d.clone(), host::installed_plugins(&d))));
                });
            });
            let note = if p.built_in {
                format!("{} · built in", p.description)
            } else {
                p.description.clone()
            };
            list = list.child(
                ui::list_row(SharedString::from(format!("plugin-row-{}", p.file)), false, |_, _, _| {}, &t)
                    .child(ui::dot(if on { Tone::Live } else { Tone::Off }))
                    .child(ui::list_text(p.name.clone(), note))
                    .child(action),
            );
        }
        list
    }

    // ----- Files --------------------------------------------------------------------------------

    fn host_load_files(&mut self, dir: &Path, rel: String) {
        let h = &mut self.servers.host;
        h.files_dir = Some(dir.to_path_buf());
        h.files_rel = rel.clone();
        h.files = None;
        h.viewing = None;
        let tx = self.servers.tx.clone();
        let d = dir.to_path_buf();
        std::thread::spawn(move || {
            let list = host::list_files(&d, &rel);
            let _ = tx.send(Event::Host(HostEvent::Files(d, rel, list)));
        });
    }

    pub(super) fn host_files_tab(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        if self.servers.host.files_dir.as_deref() != Some(dir) {
            self.host_load_files(dir, String::new());
        }
        let rel = self.servers.host.files_rel.clone();
        let mut crumbs = div().flex().items_center().gap(px(6.)).px(px(12.)).pb(px(8.));
        crumbs = crumbs.child(
            ui::link("files-root", tr("Server"))
                .text_size(px(13.))
                .on_click(cx.listener({
                    let d = dir.to_path_buf();
                    move |this, _: &ClickEvent, _, cx| {
                        this.host_load_files(&d, String::new());
                        cx.notify();
                    }
                })),
        );
        let mut acc = String::new();
        for (i, part) in rel.split('/').filter(|p| !p.is_empty()).enumerate() {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            let target = acc.clone();
            let d = dir.to_path_buf();
            crumbs = crumbs
                .child(ui::text(13., FontWeight::NORMAL, ink::FAINT).child("/"))
                .child(
                    ui::link(SharedString::from(format!("files-crumb-{i}")), part.to_string())
                        .text_size(px(13.))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.host_load_files(&d, target.clone());
                            cx.notify();
                        })),
                );
        }
        let folder = dir.join(&rel);
        crumbs = crumbs.child(div().flex_1()).child(
            ui::quiet("files-open-folder", tr("Open the folder"), 30., &t)
                .on_click(move |_, _, cx| crate::glass_toast::open(cx, &folder)),
        );
        let mut col = div().flex().flex_col().child(crumbs);
        if let Some((name, text)) = self.servers.host.viewing.clone() {
            let path = dir.join(&name);
            let body = match text {
                Ok(t) => t,
                Err(e) => e,
            };
            let mut lines = div().flex().flex_col().p(px(16.)).rounded(px(radius::ROW)).bg(white(fill::ROW_HOVER));
            for line in body.lines().take(400) {
                lines = lines.child(
                    div()
                        .font_family(crate::theme::MONO)
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(white(ink::BODY))
                        .child(if line.is_empty() { " ".to_string() } else { line.to_string() }),
                );
            }
            return col
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(12.))
                        .pb(px(8.))
                        .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(name.clone()))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    ui::quiet("file-edit", tr("Edit"), 30., &t)
                                        .on_click(move |_, _, cx| crate::glass_toast::open(cx, &path)),
                                )
                                .child(ui::quiet("file-close", tr("Close"), 30., &t).on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| {
                                        this.servers.host.viewing = None;
                                        cx.notify();
                                    },
                                ))),
                        ),
                )
                .child(lines);
        }
        match self.servers.host.files.clone() {
            None => col.child(ui::empty().child(tr("Reading…"))),
            Some(Err(e)) => col.child(ui::empty().child(e)),
            Some(Ok(entries)) if entries.is_empty() => col.child(ui::empty().child(tr("This folder is empty."))),
            Some(Ok(entries)) => {
                for (i, e) in entries.into_iter().take(500).enumerate() {
                    let child = if rel.is_empty() { e.name.clone() } else { format!("{rel}/{}", e.name) };
                    let d = dir.to_path_buf();
                    let editable = !e.dir && host::editable(&e.name);
                    let note: SharedString = if e.dir { tr("Folder").into() } else { size_text(e.bytes).into() };
                    let row = ui::list_row(SharedString::from(format!("file-{i}")), false, |_, _, _| {}, &t)
                        .cursor_pointer()
                        .child(ui::icon(if e.dir { "folder-01" } else if editable { "pencil-edit-02" } else { "package-01" }, 16., white(ink::QUIET)))
                        .child(ui::list_text(e.name.clone(), note));
                    let row = if e.dir {
                        row.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.host_load_files(&d, child.clone());
                            cx.notify();
                        }))
                    } else if editable {
                        let tx = self.servers.tx.clone();
                        row.on_click(move |_, _, _| {
                            let (d, child, tx) = (d.clone(), child.clone(), tx.clone());
                            std::thread::spawn(move || {
                                let text = host::read_text(&d, &child);
                                let _ = tx.send(Event::Host(HostEvent::FileText(child, text)));
                            });
                        })
                    } else {
                        let path = d.join(&child);
                        row.on_click(move |_, _, cx| crate::glass_toast::open(cx, &path))
                    };
                    col = col.child(row);
                }
                col
            }
        }
    }

    // ----- Performance ----------------------------------------------------------------------------

    pub(super) fn host_performance_tab(&mut self, dir: &Path, status: &Status) -> Div {
        let h = self.servers.host.history.get(dir).cloned().unwrap_or_default();
        let mut col = div().flex().flex_col().gap(px(20.));
        if h.tps.is_empty() {
            return col.child(ui::empty().child(if status.running() {
                tr("Collecting: the graphs fill in over the next minutes.")
            } else {
                tr("Start the server to see how it runs.")
            }));
        }
        let last = |v: &Vec<f32>| v.last().copied().unwrap_or(0.0);
        col = col
            .child(graph("Ticks a second", format!("{:.1} · 20 is full speed", last(&h.tps)), &h.tps, 20.0))
            .child(graph("Tick time", format!("{:.1} ms · under 50 keeps up", last(&h.mspt)), &h.mspt, 50.0))
            .child(graph("Memory", format!("{:.2} GB", last(&h.ram_gb)), &h.ram_gb, h.ram_gb.iter().cloned().fold(1.0, f32::max)))
            .child(graph("Processor", format!("{:.0} %", last(&h.cpu)), &h.cpu, h.cpu.iter().cloned().fold(100.0, f32::max)))
            .child(graph("Players", format!("{:.0} online", last(&h.players)), &h.players, h.players.iter().cloned().fold(5.0, f32::max)));
        col
    }

    // ----- Schedule -------------------------------------------------------------------------------

    pub(super) fn host_schedule_tab(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let s = match &self.servers.host.schedule {
            Some((d, s)) if d == dir => s.clone(),
            _ => {
                let s = host::schedule(dir);
                self.servers.host.schedule = Some((dir.to_path_buf(), s.clone()));
                s
            }
        };
        let entity = cx.entity();
        let save = {
            let entity = entity.clone();
            let dir = dir.to_path_buf();
            move |f: &dyn Fn(&mut Schedule), cx: &mut App| {
                entity.update(cx, |this, cx| {
                    let mut s = host::schedule(&dir);
                    f(&mut s);
                    if let Err(e) = host::save_schedule(&dir, &s) {
                        this.servers.note = Some(e);
                    }
                    this.servers.host.schedule = Some((dir.clone(), s));
                    cx.notify();
                });
            }
        };
        let backup = {
            let save = save.clone();
            ui::segments("schedule-backup", &BACKUP_HOURS, s.backup_hours, &t, move |v, _, cx| {
                save(&|s: &mut Schedule| s.backup_hours = v, cx)
            })
        };
        let restart = {
            let save = save.clone();
            ui::segments("schedule-restart", &RESTART_HOURS, s.restart_hour.unwrap_or(24), &t, move |v, _, cx| {
                save(&|s: &mut Schedule| s.restart_hour = (v < 24).then_some(v), cx)
            })
        };
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::card_heading(tr("Automatic backups")))
                    .child(backup)
                    .child(ui::card_note(tr("While the server runs. Backups appear on the Backups tab."))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::card_heading(tr("Daily restart")))
                    .child(restart)
                    .child(ui::card_note(tr("Local time. A fresh start each day keeps a long-running server quick."))),
            )
    }
}

/// One graph: its name and current value, then the samples as bars, scaled to `max`.
fn graph(name: &'static str, value: String, samples: &[f32], max: f32) -> Div {
    const BARS: usize = 120;
    let start = samples.len().saturating_sub(BARS);
    let max = if max <= 0.0 { 1.0 } else { max };
    let mut bars = div().flex().items_end().gap(px(1.)).h(px(56.)).w_full();
    for v in &samples[start..] {
        let f = (v / max).clamp(0.02, 1.0);
        bars = bars.child(div().flex_1().h(gpui::relative(f)).rounded(px(1.)).bg(white(0.32)));
    }
    for _ in samples.len().min(BARS)..BARS {
        bars = bars.child(div().flex_1().h(px(1.)).bg(white(0.08)));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .justify_between()
                .child(ui::card_heading(tr(name)))
                .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(value)),
        )
        .child(bars)
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
