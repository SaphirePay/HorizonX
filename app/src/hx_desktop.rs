//! Linux: HorizonX in the applications menu, and opening it when the user signs in.
//!
//! The menu entry is `applications/horizonx.desktop` under `$XDG_DATA_HOME` (`~/.local/share`), with the icon
//! as `horizonx` in the `hicolor` theme, so no root is needed. An installed copy (one with its engine beside it)
//! writes it when it starts if it is missing or starts a program that is gone, so a moved folder fixes itself.
//! Opening at sign-in is the same entry in `$XDG_CONFIG_HOME/autostart` (`~/.config/autostart`), which every
//! desktop that follows the XDG autostart spec reads (GNOME, KDE, Xfce, Cinnamon, MATE, LXQt...).
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const ENTRY: &str = "horizonx.desktop";

/// The entry starting the binary at `exe`.
pub fn entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=HorizonX\n\
         GenericName=Minecraft server host\n\
         Comment=Make a Minecraft server on HorizonsX in a few clicks; friends join from anywhere\n\
         Exec={}\n\
         TryExec={}\n\
         Icon=horizonx\n\
         Terminal=false\n\
         Categories=Game;Network;\n\
         Keywords=minecraft;server;host;horizon;\n\
         StartupWMClass=horizonx\n",
        crate::desktop::quote(exe),
        exe.display(),
    )
}

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset or not absolute.
pub fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    if fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Written beside and renamed, so a menu never reads half an entry.
    let temp = path.with_extension("hx-tmp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)?;
    Ok(true)
}

/// The menu entry and icons under `data`. Returns whether anything was written.
pub fn install_in(data: &Path, exe: &Path) -> io::Result<bool> {
    let mut wrote = write_if_changed(&data.join("applications").join(ENTRY), entry(exe).as_bytes())?;
    for (size, bytes) in crate::desktop::ICONS {
        let name = if size == "scalable" { "horizonx.svg" } else { "horizonx.png" };
        wrote |= write_if_changed(&data.join("icons/hicolor").join(size).join("apps").join(name), bytes)?;
    }
    Ok(wrote)
}

/// Whether the menu entry under `data` is missing or starts a program that is gone.
pub fn needs_repair_in(data: &Path) -> bool {
    let Ok(text) = fs::read_to_string(data.join("applications").join(ENTRY)) else { return true };
    let target = text.lines().find_map(|l| l.strip_prefix("TryExec=")).map(|t| PathBuf::from(t.trim()));
    !target.is_some_and(|t| t.is_file())
}

/// Puts HorizonX in the applications menu when it is missing or stale (an installed copy only). Returns whether
/// anything was written.
pub fn repair(exe: &Path) -> Result<bool, String> {
    let data = crate::desktop::data_home().ok_or("No home folder")?;
    if !needs_repair_in(&data) {
        return Ok(false);
    }
    install_in(&data, exe).map_err(|e| format!("Writing the menu entry under {}: {e}", data.display()))
}

fn autostart_path() -> Option<PathBuf> {
    config_home().map(|c| c.join("autostart").join(ENTRY))
}

/// Whether HorizonX opens when the user signs in.
pub fn opens_at_login() -> bool {
    autostart_path().is_some_and(|p| p.is_file())
}

pub fn set_opens_at_login(on: bool) -> Result<(), String> {
    let path = autostart_path().ok_or("No home folder")?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        write_if_changed(&path, entry(&exe).as_bytes()).map(|_| ()).map_err(|e| format!("Writing {}: {e}", path.display()))
    } else {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Removing {}: {e}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_the_binary_and_icon() {
        let e = entry(Path::new("/opt/Horizon X/HorizonX"));
        assert!(e.contains("Exec=\"/opt/Horizon X/HorizonX\"\n"));
        assert!(e.contains("TryExec=/opt/Horizon X/HorizonX\n"));
        assert!(e.contains("Icon=horizonx\n"));
        assert!(e.contains("StartupWMClass=horizonx\n"));
    }

    #[test]
    fn install_then_repair_is_quiet() {
        let dir = std::env::temp_dir().join(format!("hx-desktop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let exe = dir.join("HorizonX");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&exe, b"x").unwrap();
        let data = dir.join("share");
        assert!(needs_repair_in(&data));
        assert!(install_in(&data, &exe).unwrap());
        assert!(!needs_repair_in(&data));
        assert!(!install_in(&data, &exe).unwrap());
        assert!(data.join("icons/hicolor/256x256/apps/horizonx.png").is_file());
        // The program moved: the entry needs repair.
        fs::remove_file(&exe).unwrap();
        assert!(needs_repair_in(&data));
        let _ = fs::remove_dir_all(&dir);
    }
}
