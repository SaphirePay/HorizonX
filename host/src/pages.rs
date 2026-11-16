//! The sidebar's pages: what each remembers while the app runs, the header they share, and what each reads
//! when it opens. The pages themselves are in home.rs (Dashboard), servers_page.rs, players_page.rs,
//! plugins_page.rs, backups_page.rs, schedules_page.rs and settings_page.rs.
use crate::app::{Ev, HostApp};
use crate::frame::Page;
use crate::ui::{self, px};
use gpui::{div, prelude::*, Context, Div, FontWeight, SharedString};
use horizon_launcher::servers;
use horizon_launcher::stats::Range;
use horizon_theme::glass::ink;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayersTab {
    #[default]
    Online,
    Everyone,
    Operators,
    Banned,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Servers,
    Network,
    Java,
    Ai,
    Updates,
    About,
}

/// The Schedules page's form for a new schedule.
#[derive(Clone, Default)]
pub struct ScheduleForm {
    pub server: Option<PathBuf>,
    /// Restart (false) or backup (true).
    pub backup: bool,
    /// 6 or 12 hours, or 24 = daily at `hour`.
    pub every: u32,
    pub hour: u32,
}

#[derive(Default)]
pub struct PageState {
    /// The dropdown open now, by id.
    pub menu: Option<&'static str>,
    /// Dashboard: the server in the players chart (None: all together), its range, the sample under the mouse.
    pub chart_server: Option<PathBuf>,
    pub range: Range,
    pub hover: Option<usize>,
    pub spot: crate::chart::Spot,
    /// Dashboard: only the servers that need a look in Performance.
    pub perf_issues: bool,
    /// Servers: 0 all, 1 running, 2 stopped.
    pub servers_filter: u8,
    pub players_tab: PlayersTab,
    pub players_server: Option<PathBuf>,
    /// The player open on the Players page: (server, UUID or name).
    pub player: Option<(PathBuf, String)>,
    pub players_page: usize,
    pub plugins_server: Option<PathBuf>,
    /// Plugins: 0 all, 1 HorizonsX, 2 Java.
    pub plugins_filter: u8,
    pub backups_server: Option<PathBuf>,
    pub form: ScheduleForm,
    pub settings_tab: SettingsTab,
    /// A destructive action waiting for its second press ("ban:Name", "remove:file").
    pub confirm: Option<String>,
    /// Plugin changes on a running server that wait for its restart, by server.
    pub pending: std::collections::HashMap<PathBuf, std::collections::BTreeSet<String>>,
    /// Plugins: the Modrinth search and its results.
    pub market: crate::plugins_page::Market,
    /// The Network page.
    pub net: crate::network_page::NetState,
    /// Settings > AI assistants: which clients have HorizonX (read when the tab opens and after Connect).
    pub ai: Option<Vec<bool>>,
}

impl HostApp {
    pub fn page_view(&mut self, p: Page, cx: &mut Context<Self>) -> Div {
        match p {
            Page::Dashboard => self.home(cx),
            Page::Servers => self.servers_page(cx),
            Page::Players => self.players_page(cx),
            Page::Plugins => self.plugins_page(cx),
            Page::Network => self.network_page(cx),
            Page::Backups => self.backups_page(cx),
            Page::Schedules => self.schedules_page(cx),
            Page::Settings => self.settings_page(cx),
        }
    }

    /// What a page reads when it opens.
    pub fn page_opened(&mut self, p: Page) {
        let dirs: Vec<PathBuf> = self.list.iter().map(|e| e.dir.clone()).collect();
        let tx = self.tx.clone();
        match p {
            Page::Plugins => {
                // Chosen once the servers' states are known (a running one first); the page picks then.
                if self.pages.plugins_server.as_ref().is_some_and(|d| !dirs.contains(d)) {
                    self.pages.plugins_server = None;
                }
                if self.pages.plugins_server.is_none() && !self.status.is_empty() {
                    self.pages.plugins_server = self.first_server();
                }
                if let Some(d) = self.pages.plugins_server.clone() {
                    self.load_details(&d);
                }
            }
            Page::Network => self.net_poll(true),
            Page::Backups => {
                for d in dirs {
                    self.load_backups(&d);
                }
            }
            Page::Schedules | Page::Settings | Page::Servers => {
                std::thread::spawn(move || {
                    for d in dirs {
                        if let Some(c) = servers::config(&d) {
                            let _ = tx.send(Ev::Settings(d, c));
                        }
                    }
                });
                if self.pages.form.server.is_none() {
                    self.pages.form = crate::pages::ScheduleForm { server: self.first_server(), backup: false, every: 6, hour: 4 };
                }
            }
            Page::Players | Page::Dashboard => {
                let java: Vec<PathBuf> = dirs;
                std::thread::spawn(move || {
                    for d in java {
                        let _ = tx.send(Ev::Stats(d.clone(), horizon_launcher::stats::load(&d)));
                    }
                });
            }
        }
    }

    /// The server to show first where a page needs one: a running one, else the first.
    pub fn first_server(&self) -> Option<PathBuf> {
        self.list
            .iter()
            .find(|e| self.status.get(&e.dir).is_some_and(|s| s.running()))
            .or_else(|| self.list.first())
            .map(|e| e.dir.clone())
    }

    /// The servers as dropdown choices: (name, dot).
    pub fn server_choices(&self) -> Vec<(SharedString, Option<ui::Tone>)> {
        self.list.iter().map(|e| (SharedString::from(e.name.clone()), Some(crate::frame::server_tone(&self.status_of(&e.dir))))).collect()
    }

    /// A dropdown of the servers. `all` adds "All servers" first (index 0 = None).
    pub fn server_picker(
        &mut self,
        id: &'static str,
        current: Option<PathBuf>,
        all: bool,
        cx: &mut Context<Self>,
        on_pick: impl Fn(&mut HostApp, Option<PathBuf>, &mut Context<HostApp>) + 'static,
    ) -> Div {
        let mut choices = self.server_choices();
        if all {
            choices.insert(0, ("All servers".into(), None));
        }
        let label = current.as_ref().map(|d| self.name_of(d)).unwrap_or_else(|| "All servers".into());
        let tone = current.as_ref().map(|d| crate::frame::server_tone(&self.status_of(d)));
        let dirs: Vec<PathBuf> = self.list.iter().map(|e| e.dir.clone()).collect();
        let open = self.pages.menu == Some(id);
        let entity = cx.entity();
        let e2 = entity.clone();
        let on_pick = std::rc::Rc::new(on_pick);
        ui::picker(
            id,
            label,
            tone,
            choices,
            open,
            move |_, cx| {
                entity.update(cx, |this, cx| {
                    this.pages.menu = if this.pages.menu == Some(id) { None } else { Some(id) };
                    cx.notify();
                })
            },
            move |i, _, cx| {
                let pick = on_pick.clone();
                let dirs = dirs.clone();
                e2.update(cx, |this, cx| {
                    this.pages.menu = None;
                    let chosen = if all { if i == 0 { None } else { dirs.get(i - 1).cloned() } } else { dirs.get(i).cloned() };
                    pick(this, chosen, cx);
                    cx.notify();
                })
            },
        )
    }
}

/// A page's top: a quiet line over the title, and whatever goes on the right.
pub fn header(over: impl Into<SharedString>, title: &'static str, right: Option<Div>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_end()
        .justify_between()
        .gap(px(16.))
        .child(
            div()
                .flex()
                .flex_col()
                .child(ui::text(14., FontWeight::MEDIUM, ink::QUIET).child(over.into()))
                .child(ui::display(34.).mt(px(3.)).child(title)),
        )
        .children(right)
}

/// A page: its header over its content, padded like the mockups, scrolling when it doesn't fit.
pub fn page(id: &'static str, head: Div, content: Div) -> Div {
    div().flex_1().min_h_0().flex().flex_col().child(
        div()
            .id(id)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pt(px(8.))
            .pr(px(4.))
            .child(head)
            .child(content),
    )
}

/// A panel's heading with an optional count chip.
pub fn panel_title(title: impl Into<SharedString>, count: Option<usize>) -> Div {
    div().flex().items_center().gap(px(10.)).child(ui::heading(17.).child(title.into())).children(count.map(|n| ui::chip(n.to_string(), None)))
}

/// A server's plugin data folder relative to the server, when it has one (by its plugin name or file).
pub fn plugin_folder(dir: &std::path::Path, name: &str, file: &str) -> Option<String> {
    let stem = file.trim_end_matches(".dll").trim_end_matches(".so").trim_end_matches(".jar").trim_start_matches("lib").trim_end_matches("_rs");
    let plugins = dir.join("plugins");
    let folders: Vec<String> = std::fs::read_dir(&plugins)
        .map(|r| r.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let simple = |s: &str| s.to_lowercase().replace([' ', '-', '_', '.'], "");
    folders.into_iter().find(|f| simple(f) == simple(name) || simple(f) == simple(stem)).map(|f| format!("plugins/{f}"))
}
