//! Plugins: one server at a time. What's installed on it (HorizonsX plugins and Java plugins, each on or off,
//! its settings, remove), a note when a change waits for a restart, and the plugins that come with Horizon
//! Host, one click to install. Java jars come in with Add a .jar.
use crate::app::{Ev, HostApp};
use crate::pages::{self, panel_title, plugin_folder};
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, PathPromptOptions, SharedString};
use horizon_launcher::host;
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
            installed = installed.child(group(if layer { "Java plugins · apply at the next restart" } else { "Java plugins · the Java layer is off on this server" }));
            for (i, r) in java.iter().enumerate() {
                installed = installed.child(self.plugin_row(&dir, 100 + i, r, st.running(), layer, cx));
            }
        }
        let catalog = self.catalog_panel(&dir, &name, &files, cx);
        let body = if ui::wide() {
            div().flex().items_start().gap(px(16.)).child(installed).child(catalog.flex_none().w(px(400.)))
        } else {
            div().flex().flex_col().gap(px(16.)).child(installed).child(catalog)
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
        let control = if r.built_in {
            div().child(ui::chip("Built in", None))
        } else {
            let (d, file, name, on) = (dir.to_path_buf(), r.file.clone(), r.name.clone(), r.on);
            div().child(ui::toggle(SharedString::from(format!("pl-on-{i}")), r.on).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                match host::set_plugin_on(&d, &file, !on) {
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

    /// The plugins that come with Horizon Host: Install, or ticked when the server has them.
    fn catalog_panel(&mut self, dir: &Path, name: &str, files: &[(String, bool)], cx: &mut Context<Self>) -> Div {
        let mut items: Vec<host::Plugin> = self.catalog.iter().filter(|p| !p.built_in).cloned().collect();
        // Not installed first.
        items.sort_by_key(|p| files.iter().any(|(f, _)| f == &p.file));
        let mut list = div().flex().flex_col().mt(px(6.));
        if items.is_empty() {
            list = list.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).py(px(8.)).child("Horizon Host's plugins aren't on this PC (its plugins folder is missing)."));
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
            .child(ui::heading(18.).child("Built into Horizon Host"))
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
