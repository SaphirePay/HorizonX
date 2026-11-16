//! HorizonX: make a Minecraft server on HorizonsX in a few clicks with a setup wizard, run it
//! from a dashboard, and let friends join from anywhere with no port forwarding (playit.gg built
//! in). Its own program, in the Horizon launcher's style: the pixel-sunset scene behind frosted
//! plates, Geist and Bricolage, one violet accent.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod art;
mod backups_page;
mod chart;
mod dashboard;
mod demo;
mod frame;
mod glass;
mod highlight;
mod home;
mod line_edit;
mod mcp;
mod network_page;
mod motion;
mod pages;
mod players_page;
mod plugins_page;
mod scene;
mod schedules_page;
mod servers_page;
mod settings_page;
mod sky;
mod ui;
mod update;
mod wizard;

use gpui::{px, size, App, AppContext, Bounds, SharedString, WindowBounds, WindowOptions};
use std::borrow::Cow;

/// The launcher's icons, served to `svg()` as `icons/glass/<name>.svg`.
struct Assets;

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("icons/glass/").and_then(|p| p.strip_suffix(".svg"));
        Ok(name.and_then(horizon_theme::glass_icons::get).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(horizon_theme::glass_icons::ALL.iter().map(|(name, _)| format!("icons/glass/{name}.svg").into()).collect())
    }
}

fn main() {
    // `HorizonX mcp`: the MCP server for AI assistants, on stdin and stdout (no window).
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        if let Err(e) = mcp::serve() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    // `HorizonX update`: check GitHub and install a newer release now, with no window (also for a PC without a
    // screen). `HorizonX version` prints this copy's version.
    match std::env::args().nth(1).as_deref() {
        Some("update") => std::process::exit(update::run_cli()),
        Some("version" | "--version") => {
            println!("HorizonX {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    // New releases from GitHub, in the background (an installed copy; a build from source is left alone).
    update::start_checking();
    gpui_platform::application().with_assets(Assets).run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(horizon_theme::fonts::GEIST_REGULAR),
                Cow::Borrowed(horizon_theme::fonts::GEIST_MEDIUM),
                Cow::Borrowed(horizon_theme::fonts::GEIST_SEMIBOLD),
                Cow::Borrowed(horizon_theme::fonts::GEIST_MONO),
                Cow::Borrowed(horizon_theme::fonts::BRICOLAGE_DISPLAY_SEMIBOLD),
                Cow::Borrowed(horizon_theme::fonts::BRICOLAGE_HEADING_SEMIBOLD),
                Cow::Borrowed(horizon_theme::fonts::BRICOLAGE_SEMIBOLD),
            ])
            .expect("bundled fonts");
        let bounds = Bounds::centered(None, size(px(1560.), px(940.)), cx);
        cx.open_window(
            WindowOptions {
                // The demo records a full-screen window.
                window_bounds: Some(if demo::on() { WindowBounds::Fullscreen(bounds) } else { WindowBounds::Windowed(bounds) }),
                window_min_size: Some(size(px(1180.), px(720.))),
                titlebar: Some(gpui::TitlebarOptions { title: Some("HorizonX".into()), ..Default::default() }),
                app_id: Some("horizonx".into()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| app::HostApp::new(window, cx)),
        )
        .expect("window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
