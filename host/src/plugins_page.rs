//! Plugins: one server at a time. What's installed on it (HorizonsX plugins and Java plugins, each on or off,
//! its settings, remove), a note when a change waits for a restart, and the plugins that come with Horizon
//! Host, one click to install. Java jars come in with Add a .jar.
use crate::app::{Ev, Field, HostApp};
use crate::pages::{self, panel_title, plugin_folder};
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, PathPromptOptions, SharedString};
use horizon_launcher::{host, plugin_market};
use horizon_theme::glass::{ink, radius};
use std::path::{Path, PathBuf};

/// One row of the installed list.
struct Row {
    file: String,
    name: String,
    version: String,
    about: String,
    on: bool,
    built_in: bool,
    java: bool,
    folder: Option<String>,
}

impl HostApp {
    pub fn plugins_page(&mut self, cx: &mut Context<Self>) -> Div {
        if self.pages.plugins_server.is_none() && self.status.is_empty() {
            // The servers' states aren't in yet: wait a moment to show a running one first.
            let head = pages::header("Manage what runs on each server", "Plugins", None);
            return pages::page("plugins-scroll", head, div());
        }
        let Some(dir) = self.pages.plugins_server.clone().filter(|d| self.list.iter().any(|e| &e.dir == d)).or_else(|| self.first_server()) else {
            let head = pages::header("Manage what runs on each server", "Plugins", None);
            return pages::page("plugins-scroll", head, ui::panel().child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child("Make a server first: New server, at the bottom left.")));
        };
        if self.pages.plugins_server.as_ref() != Some(&dir) {
            self.pages.plugins_server = Some(dir.clone());
            self.load_details(&dir);
        }
        let name = self.name_of(&dir);
        let st = self.status_of(&dir);
        let picker = self.server_picker("plugins-server", Some(dir.clone()), false, cx, |this, d, _| {
            if let Some(d) = d {
                this.pages.plugins_server = Some(d.clone());
                this.pages.confirm = None;
                this.load_details(&d);
            }
        });
        let head = pages::header(
            "Manage what runs on each server",
            "Plugins",
            Some(div().flex().items_center().gap(px(10.)).child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child("Server")).child(picker)),
        );
        // What's installed: HorizonsX libraries by the catalog's names, then the jars.
        let files = self.plugin_files.get(&dir).cloned().unwrap_or_default();
        let (jars, layer) = self.java.get(&dir).cloned().unwrap_or_default();
        let mut horizon: Vec<Row> = files
            .iter()
            .filter(|(f, _)| !f.to_ascii_lowercase().ends_with(".jar"))
            .filter(|(f, _)| !f.contains("horizon_java"))
            .map(|(f, on)| {
                let known = self.catalog.iter().find(|p| &p.file == f);
                let name = known.map(|p| p.name.clone()).unwrap_or_else(|| f.trim_end_matches(".dll").trim_end_matches(".so").trim_start_matches("lib").to_string());
                Row {
                    file: f.clone(),
                    folder: plugin_folder(&dir, &name, f),
                    name,
                    version: String::new(),
                    about: known.map(|p| p.description.clone()).unwrap_or_default(),
                    on: *on,
                    built_in: known.is_some_and(|p| p.built_in),
                    java: false,
                }
            })
            .collect();
        horizon.sort_by_key(|r| (!r.built_in, r.name.to_lowercase()));
        let java: Vec<Row> = jars
            .iter()
            .map(|j| Row {
                file: j.file.clone(),
                name: j.name.clone(),
                version: j.version.clone(),
                about: j.description.clone(),
                on: j.on,
                built_in: false,
                java: true,
                folder: j.folder.clone(),
            })
            .collect();
        let filter = self.pages.plugins_filter;
        let total = horizon.len() + java.len();
        let tabs = ui::tabs(
            "plugins-filter",
            &[(0u8, format!("All {total}")), (1, format!("HorizonsX {}", horizon.len())), (2, format!("Java {}", java.len()))],
            filter,
            {
                let e = cx.entity();
                move |v, _, cx| {
                    e.update(cx, |this, cx| {
                        this.pages.plugins_filter = v;
                        cx.notify();
                    })
                }
            },
        );
        let add_jar = self.add_jar_button("plugins-add-jar", &dir, cx);
        let mut installed = ui::panel()
            .flex_1()
            .min_w_0()
            .px(px(22.))
            .py(px(20.))
            .child(div().flex().items_center().justify_between().gap(px(10.)).child(panel_title(format!("Installed on {name}"), Some(total))).child(div().flex().items_center().gap(px(8.)).child(tabs).child(add_jar)));
        // A change that waits for the server's restart.
        if let Some(waiting) = self.pages.pending.get(&dir).filter(|w| !w.is_empty() && st.running()) {
            let what = waiting.iter().cloned().collect::<Vec<_>>().join(", ");
            let d = dir.clone();
            installed = installed.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .mt(px(12.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(radius::CONTROL))
                    .bg(ui::color(ui::WARN).opacity(0.09))
                    .child(ui::icon("refresh", 15., ui::color(ui::WARN)))
                    .child(div().flex_1().min_w_0().child(ui::text(12.5, FontWeight::NORMAL, ink::FULL).child(format!("{what} changed. The server picks it up when it restarts."))))
                    .child(ui::sized_button("plugins-restart", "Restart now", Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pages.pending.remove(&d);
                        this.restart(d.clone(), cx);
                    }))),
            );
        }
        if total == 0 {
            installed = installed.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).mt(px(14.)).child("No plugins on this server yet. Install one from the list on the right, or add a Java plugin jar."));
        }
        if filter != 2 && !horizon.is_empty() {
            installed = installed.child(group("HorizonsX plugins · on at once, off after a restart"));
            for (i, r) in horizon.iter().enumerate() {
                installed = installed.child(self.plugin_row(&dir, i, r, st.running(), layer, cx));
            }
        }
        if filter != 1 && !java.is_empty() {
            let d = dir.clone();
            let switch = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Java layer"))
                .child(ui::toggle("java-layer-on", layer).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    match host::set_java_layer(&d, !layer) {
                        Ok(()) => {
                            if this.status_of(&d).running() {
                                this.pages.pending.entry(d.clone()).or_default().insert("The Java layer".into());
                            }
                        }
                        Err(e) => this.note = Some((e, std::time::Instant::now())),
                    }
                    this.load_details(&d);
                    cx.notify();
                })));
            installed = installed.child(group(if layer { "Java plugins · apply at the next restart" } else { "Java plugins · the Java layer is off on this server" }).child(switch));
            for (i, r) in java.iter().enumerate() {
                installed = installed.child(self.plugin_row(&dir, 100 + i, r, st.running(), layer, cx));
            }
        }
        let catalog = self.catalog_panel(&dir, &name, &files, cx);
        let market = self.market_panel(&dir, &name, cx);
        let body = if ui::wide() {
            let left = div().flex().flex_col().gap(px(16.)).flex_1().min_w_0().child(installed).child(market);
            div().flex().items_start().gap(px(16.)).child(left).child(catalog.flex_none().w(px(400.)))
        } else {
            div().flex().flex_col().gap(px(16.)).child(installed).child(market).child(catalog)
        };
        pages::page("plugins-scroll", head, body)
    }

    fn plugin_row(&mut self, dir: &Path, i: usize, r: &Row, running: bool, layer: bool, cx: &mut Context<Self>) -> Div {
        let pending = self.pages.pending.get(dir).is_some_and(|p| p.contains(&r.name));
        let (state, tone): (String, Tone) = match () {
            _ if r.java && !layer => ("Java layer off".into(), Tone::Off),
            _ if pending => (if r.on { "Starts at restart".into() } else { "Off at restart".into() }, Tone::Busy),
            _ if !r.on => ("Off".into(), Tone::Off),
            _ if !running => ("Server stopped".into(), Tone::Off),
            _ if r.built_in => ("Running · built in".into(), Tone::Live),
            _ => ("Running".into(), Tone::Live),
        };
        let state_color = match tone {
            Tone::Busy => ui::color(ui::WARN),
            Tone::Live => white(ink::FULL),
            _ => white(ink::QUIET),
        };
        let control = {
            let (d, file, name, on) = (dir.to_path_buf(), r.file.clone(), r.name.clone(), r.on);
            div().child(ui::toggle(SharedString::from(format!("pl-on-{i}")), r.on).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let done = if file == host::PLAYIT { host::set_playit(&d, !on) } else { host::set_plugin_on(&d, &file, !on) };
                match done {
                    Ok(()) => {
                        if this.status_of(&d).running() {
                            this.pages.pending.entry(d.clone()).or_default().insert(name.clone());
                        }
                    }
                    Err(e) => this.note = Some((e, std::time::Instant::now())),
                }
                this.load_details(&d);
                cx.notify();
            })))
        };
        let settings = match r.folder.clone() {
            Some(folder) => {
                let d = dir.to_path_buf();
                ui::sized_button(SharedString::from(format!("pl-set-{i}")), "Settings", Kind::Secondary, 30., 12.5)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_server(d.clone(), cx);
                        let first = horizon_launcher::host::java_plugins(&d).into_iter().find(|j| j.folder.as_deref() == Some(folder.as_str())).and_then(|j| j.configs.into_iter().next());
                        this.open_plugin_file(&d, &folder, first, cx);
                    }))
                    .into_any_element()
            }
            None => ui::text(11.5, FontWeight::NORMAL, ink::FAINT).child("no settings").into_any_element(),
        };
        // The same width with or without a Settings button, so every row's columns line up.
        let settings = div().flex_none().w(px(96.)).flex().justify_center().child(settings);
        let confirm_key = format!("remove:{}", r.file);
        let confirming = self.pages.confirm.as_deref() == Some(confirm_key.as_str());
        let remove = if r.built_in {
            div().w(px(30.))
        } else {
            let (d, file, key) = (dir.to_path_buf(), r.file.clone(), confirm_key.clone());
            let file_off = format!("{}.disabled", r.file);
            div().child(
                div()
                    .id(SharedString::from(format!("pl-rm-{i}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(30.))
                    .px(px(if confirming { 10. } else { 0. }))
                    .w(if confirming { gpui::Length::Auto } else { px(30.).into() })
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .bg(if confirming { ui::color(ui::DANGER).opacity(0.14) } else { white(0.08) })
                    .hover(|s| s.bg(ui::color(ui::DANGER).opacity(0.2)))
                    .child(if confirming { ui::text(12., FontWeight::MEDIUM, ink::FULL).text_color(ui::color(ui::DANGER)).child("Remove?").into_any_element() } else { ui::icon("delete-02", 14., white(ink::BODY)).into_any_element() })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if this.pages.confirm.as_deref() != Some(key.as_str()) {
                            this.pages.confirm = Some(key.clone());
                        } else {
                            this.pages.confirm = None;
                            let r1 = host::remove_plugin(&d, &file);
                            let r2 = host::remove_plugin(&d, &file_off);
                            if let Err(e) = r1.and(r2) {
                                this.note = Some((e, std::time::Instant::now()));
                            } else {
                                this.note = Some(("Removed. Its settings folder stays, in case you add it again.".into(), std::time::Instant::now()));
                            }
                            this.load_details(&d);
                        }
                        cx.notify();
                    })),
            )
        };
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .py(px(10.))
            .px(px(4.))
            .border_b_1()
            .border_color(white(0.06))
            .child(div().flex().flex_none().items_center().justify_center().size(px(34.)).rounded(px(9.)).bg(white(0.07)).child(ui::icon(if r.java { "package-01" } else { "puzzle" }, 16., white(ink::BODY))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(div().flex().items_end().gap(px(8.)).child(ui::text(14., FontWeight::SEMIBOLD, if r.on { ink::FULL } else { ink::BODY }).child(r.name.clone())).when(!r.version.is_empty(), |d| d.child(ui::mono(11., ink::QUIET).mb(px(1.)).child(r.version.clone()))))
                    .when(!r.about.is_empty(), |d| d.child(ui::text(12., FontWeight::NORMAL, ink::QUIET).truncate().child(r.about.clone()))),
            )
            .child(div().flex_none().w(px(150.)).flex().items_center().gap(px(7.)).child(ui::dot(tone)).child(ui::text(12.5, FontWeight::NORMAL, ink::FULL).text_color(state_color).truncate().child(state)))
            .child(div().flex_none().w(px(64.)).flex().justify_center().child(control))
            .child(settings)
            .child(remove)
    }

    /// "Add a .jar": the system's file picker, then the jars go into the server's plugins folder.
    fn add_jar_button(&mut self, id: &'static str, dir: &Path, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let d = dir.to_path_buf();
        ui::icon_button(id, "upload-04", "Add a .jar", Kind::Secondary).on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Add".into()) });
            let d = d.clone();
            cx.spawn(async move |this, cx| {
                let Ok(Ok(Some(paths))) = paths.await else { return };
                let _ = this.update(cx, |this, cx| {
                    match host::add_jars(&d, &paths) {
                        Ok(0) => this.note = Some(("Pick .jar files: Bukkit, Spigot or Paper plugins.".into(), std::time::Instant::now())),
                        Ok(n) => {
                            if this.status_of(&d).running() {
                                for p in &paths {
                                    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                                    this.pages.pending.entry(d.clone()).or_default().insert(name);
                                }
                            }
                            this.note = Some((format!("{n} Java plugin{} added.", if n == 1 { "" } else { "s" }), std::time::Instant::now()));
                        }
                        Err(e) => this.note = Some((e, std::time::Instant::now())),
                    }
                    this.load_details(&d);
                    cx.notify();
                });
            })
            .detach();
        }))
    }

    /// The plugins that come with HorizonX: Install, or ticked when the server has them.
    fn catalog_panel(&mut self, dir: &Path, name: &str, files: &[(String, bool)], cx: &mut Context<Self>) -> Div {
        let mut items: Vec<host::Plugin> = self.catalog.iter().filter(|p| !p.built_in).cloned().collect();
        // Not installed first.
        items.sort_by_key(|p| files.iter().any(|(f, _)| f == &p.file));
        let mut list = div().flex().flex_col().mt(px(6.));
        if items.is_empty() {
            list = list.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).py(px(8.)).child("HorizonX's plugins aren't on this PC (its plugins folder is missing)."));
        }
        let n = items.len();
        for (i, p) in items.into_iter().enumerate() {
            let have = files.iter().find(|(f, _)| f == &p.file).map(|(_, on)| *on);
            let action = match have {
                Some(on) => div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(ui::icon("tick-02", 13., if on { ui::color(ui::LIVE) } else { white(ink::QUIET) }))
                    .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(if on { "Installed" } else { "Installed · off" }))
                    .into_any_element(),
                None => {
                    let (d, file, tx) = (dir.to_path_buf(), p.file.clone(), self.tx.clone());
                    ui::sized_button(SharedString::from(format!("cat-install-{i}")), "Install", Kind::Secondary, 30., 12.5)
                        .on_click(move |_, _, _| {
                            let (d, file, tx) = (d.clone(), file.clone(), tx.clone());
                            std::thread::spawn(move || {
                                match host::install_plugin(&d, &file) {
                                    Ok(()) => {
                                        let _ = tx.send(Ev::Note("Installed. On HorizonsX it starts right away.".into()));
                                    }
                                    Err(e) => {
                                        let _ = tx.send(Ev::Note(e));
                                    }
                                }
                                let _ = tx.send(Ev::Plugins(d.clone(), host::plugin_files(&d)));
                                let _ = tx.send(Ev::Installed(d.clone(), host::installed_plugins(&d)));
                            });
                        })
                        .into_any_element()
                }
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(11.))
                    .px(px(2.))
                    .when(i + 1 < n, |d| d.border_b_1().border_color(white(0.06)))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(ui::text(13.5, FontWeight::SEMIBOLD, if have.is_some() { ink::BODY } else { ink::FULL }).child(p.name.clone())).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(p.description.clone())))
                    .child(div().flex_none().child(action)),
            );
        }
        let jar = self.add_jar_button("cat-add-jar", dir, cx).h(px(32.)).text_size(px(12.5));
        ui::panel()
            .px(px(20.))
            .py(px(20.))
            .child(ui::heading(18.).child("Built into HorizonX"))
            .child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).mt(px(2.)).child(format!("Made for HorizonsX. One click puts it on {name}; it starts right away.")))
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .mt(px(14.))
                    .pt(px(14.))
                    .border_t_1()
                    .border_color(white(0.07))
                    .child(ui::icon("package-01", 16., white(ink::QUIET)))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child("A Paper or Spigot jar?")).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("It runs unchanged.")))
                    .child(jar),
            )
    }
}

/// A group's line: a quiet label with a rule after it.
fn group(label: &'static str) -> Div {
    div().flex().items_center().gap(px(10.)).pt(px(16.)).pb(px(4.)).px(px(4.)).child(ui::mono(10.5, ink::QUIET).child(label.to_uppercase())).child(div().flex_1().h(px(1.)).bg(white(0.07)))
}

#[allow(unused)]
fn _keep(_: PathBuf) {}

// ----- Modrinth: plugins for the server's game version ---------------------------------------------

/// The Modrinth search on the Plugins page.
#[derive(Default)]
pub struct Market {
    /// The search the results are for (None: nothing asked yet).
    pub query: Option<String>,
    pub hits: Vec<plugin_market::Hit>,
    pub total: usize,
    pub loading: bool,
    pub error: Option<String>,
    /// Plugins being put in (project ids).
    pub adding: std::collections::BTreeSet<String>,
}

impl HostApp {
    /// Searches Modrinth (from the first result, or the next page with `offset`).
    pub fn market_search(&mut self, offset: usize) {
        let query = if offset == 0 { self.text(Field::PluginSearch).trim().to_owned() } else { self.pages.market.query.clone().unwrap_or_default() };
        let m = &mut self.pages.market;
        if m.loading {
            return;
        }
        m.loading = true;
        m.error = None;
        if offset == 0 {
            m.query = Some(query.clone());
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = plugin_market::search(&query, offset);
            let _ = tx.send(Ev::Market(query, offset, r));
        });
    }

    pub fn market_event(&mut self, query: String, offset: usize, r: Result<plugin_market::Results, String>) {
        let m = &mut self.pages.market;
        m.loading = false;
        if m.query.as_deref() != Some(query.as_str()) {
            return;
        }
        match r {
            Ok(res) => {
                if offset == 0 {
                    m.hits = res.hits;
                } else {
                    m.hits.extend(res.hits);
                }
                m.total = res.total;
            }
            Err(e) => m.error = Some(e),
        }
    }

    /// Puts a Modrinth plugin into these servers in the background.
    fn market_add(&mut self, dirs: Vec<PathBuf>, id: String, title: String) {
        if !self.pages.market.adding.insert(id.clone()) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let results = dirs.iter().map(|d| plugin_market::install(d, &id)).collect();
            let _ = tx.send(Ev::MarketDone(dirs, format!("{id}\u{0}{title}"), results));
        });
    }

    pub fn market_done(&mut self, dirs: Vec<PathBuf>, key: String, results: Vec<Result<plugin_market::Installed, String>>) {
        let (id, title) = key.split_once('\u{0}').map(|(a, b)| (a.to_owned(), b.to_owned())).unwrap_or((key.clone(), key));
        self.pages.market.adding.remove(&id);
        let mut ok = Vec::new();
        let mut errors = Vec::new();
        let mut with = std::collections::BTreeSet::new();
        for (d, r) in dirs.iter().zip(results) {
            match r {
                Ok(i) => {
                    with.extend(i.with.iter().cloned());
                    if self.status_of(d).running() {
                        self.pages.pending.entry(d.clone()).or_default().insert(i.name.clone());
                    }
                    ok.push(self.name_of(d).to_string());
                }
                Err(e) => errors.push(format!("{}: {e}", self.name_of(d))),
            }
            self.load_details(d);
        }
        let mut note = String::new();
        if !ok.is_empty() {
            note = format!("{title} added to {}", ok.join(", "));
            if !with.is_empty() {
                note.push_str(&format!(" (with {}, which it needs)", with.into_iter().collect::<Vec<_>>().join(", ")));
            }
            note.push_str(". It starts at the server's next restart.");
        }
        if !errors.is_empty() {
            if !note.is_empty() {
                note.push(' ');
            }
            note.push_str(&errors.join(" · "));
        }
        self.note = Some((note, std::time::Instant::now()));
    }

    /// Search box, results with Add and Add to all, and More.
    fn market_panel(&mut self, dir: &Path, name: &str, cx: &mut Context<Self>) -> Div {
        if self.pages.market.query.is_none() && !self.pages.market.loading {
            // The most downloaded first, before any search.
            self.market_search(0);
        }
        let field = self.field(Field::PluginSearch, "Search: shops, ranks, chat, protection...", cx);
        let search = ui::sized_button("market-search", "Search", Kind::Secondary, 36., 12.5).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.market_search(0);
            cx.notify();
        }));
        let have = plugin_market::installed(dir);
        let all: Vec<PathBuf> = self.list.iter().map(|e| e.dir.clone()).collect();
        let many = all.len() > 1;
        let m = &self.pages.market;
        let (hits, total, loading, error, adding) = (m.hits.clone(), m.total, m.loading, m.error.clone(), m.adding.clone());
        let searched = m.query.clone().unwrap_or_default();
        let mut list = div().flex().flex_col().mt(px(8.));
        if let Some(e) = error {
            list = list.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).py(px(8.)).child(e));
        } else if hits.is_empty() && loading {
            list = list.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).py(px(8.)).child("Looking on Modrinth..."));
        } else if hits.is_empty() {
            list = list.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).py(px(8.)).child(format!("No plugins for Minecraft {} match \"{searched}\".", plugin_market::GAME_VERSION)));
        }
        let n = hits.len();
        for (i, h) in hits.into_iter().enumerate() {
            let installed = have.contains_key(&h.id);
            let busy = adding.contains(&h.id);
            let action = if busy {
                ui::chip("Adding...", Some(Tone::Busy)).into_any_element()
            } else if installed {
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(ui::icon("tick-02", 13., ui::color(ui::LIVE)))
                    .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Installed"))
                    .into_any_element()
            } else {
                let (d, id, title) = (dir.to_path_buf(), h.id.clone(), h.title.clone());
                let add = ui::sized_button(SharedString::from(format!("mk-add-{i}")), "Add", Kind::Secondary, 28., 12.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.market_add(vec![d.clone()], id.clone(), title.clone());
                    cx.notify();
                }));
                let mut col = div().flex().flex_col().items_end().gap(px(4.)).child(add);
                if many {
                    let (dirs, id, title) = (all.clone(), h.id.clone(), h.title.clone());
                    col = col.child(
                        div()
                            .id(SharedString::from(format!("mk-all-{i}")))
                            .cursor_pointer()
                            .child(ui::text(11., FontWeight::NORMAL, ink::QUIET).hover(|s| s.text_color(white(ink::FULL))).child("Add to all"))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.market_add(dirs.clone(), id.clone(), title.clone());
                                cx.notify();
                            })),
                    );
                }
                col.into_any_element()
            };
            let url = h.url();
            let title = div()
                .id(SharedString::from(format!("mk-open-{i}")))
                .cursor_pointer()
                .child(ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).truncate().hover(|s| s.underline()).child(h.title.clone()))
                .on_click(move |_, _, cx| cx.open_url(&url));
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(10.))
                    .px(px(2.))
                    .when(i + 1 < n, |d| d.border_b_1().border_color(white(0.06)))
                    .child(div().flex().flex_none().items_center().justify_center().size(px(32.)).rounded(px(9.)).bg(white(0.07)).child(ui::text(13., FontWeight::SEMIBOLD, ink::BODY).child(h.title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(title)
                            .child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).truncate().child(format!("by {} · {} downloads", h.author, plugin_market::short_count(h.downloads))))
                            .child(ui::text(12., FontWeight::NORMAL, ink::BODY).line_clamp(2).child(h.description.clone())),
                    )
                    .child(div().flex_none().child(action)),
            );
        }
        let shown = list_len(n, total);
        let more = (n < total && n > 0).then(|| {
            ui::sized_button("market-more", if loading { "Loading..." } else { "More" }, Kind::Ghost, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.market_search(n);
                cx.notify();
            }))
        });
        ui::panel()
            .px(px(20.))
            .py(px(20.))
            .child(div().flex().items_center().gap(px(8.)).child(ui::icon("globe-02", 17., white(ink::BODY))).child(ui::heading(18.).child("Get plugins from Modrinth")))
            .child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).mt(px(2.)).child(format!("Paper, Spigot and Bukkit plugins made for Minecraft {}. Add puts one on {name}, with the plugins it needs.", plugin_market::GAME_VERSION)))
            .child(div().flex().items_center().gap(px(8.)).mt(px(12.)).child(div().flex_1().min_w_0().child(field)).child(search))
            .child(list)
            .child(div().flex().items_center().justify_between().mt(px(10.)).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(shown)).children(more))
    }
}

fn list_len(shown: usize, total: usize) -> String {
    if total == 0 { String::new() } else { format!("{shown} of {} plugins", plugin_market::short_count(total as u64)) }
}
