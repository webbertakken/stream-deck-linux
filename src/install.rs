//! Install the app into the desktop: themed icons under the hicolor theme and
//! a launcher `.desktop` entry, so the tray/autostart icon resolves by name and
//! the app appears in the application menu.
//!
//! Icons are embedded in the binary, so install works regardless of where the
//! binary lives.

use std::path::{Path, PathBuf};

use crate::error::Result;

/// Freedesktop icon name used by the tray and desktop entries.
pub const ICON_NAME: &str = "streamdeck";
/// Basename of the application launcher entry.
pub const DESKTOP_NAME: &str = "streamdeck.desktop";

/// Embedded PNG icons by square size (committed under `assets/icons/`).
const ICONS: &[(u32, &[u8])] = &[
    (16, include_bytes!("../assets/icons/streamdeck-16.png")),
    (24, include_bytes!("../assets/icons/streamdeck-24.png")),
    (32, include_bytes!("../assets/icons/streamdeck-32.png")),
    (48, include_bytes!("../assets/icons/streamdeck-48.png")),
    (64, include_bytes!("../assets/icons/streamdeck-64.png")),
    (128, include_bytes!("../assets/icons/streamdeck-128.png")),
    (256, include_bytes!("../assets/icons/streamdeck-256.png")),
    (512, include_bytes!("../assets/icons/streamdeck-512.png")),
];

/// Build the application launcher `.desktop` contents.
pub fn launcher_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Stream Deck\n\
         Comment=Custom Stream Deck control\n\
         Exec={exec}\n\
         Icon={ICON_NAME}\n\
         Terminal=false\n\
         Categories=Utility;\n"
    )
}

/// Resolve `$XDG_DATA_HOME`, falling back to `~/.local/share`.
fn data_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".local").join("share")
}

/// Path to a themed icon of `size` under a data home (pure).
pub fn icon_path_under(data_home: &Path, size: u32) -> PathBuf {
    data_home
        .join("icons")
        .join("hicolor")
        .join(format!("{size}x{size}"))
        .join("apps")
        .join(format!("{ICON_NAME}.png"))
}

/// Path to the launcher entry under a data home (pure).
pub fn desktop_path_under(data_home: &Path) -> PathBuf {
    data_home.join("applications").join(DESKTOP_NAME)
}

/// Install icons + launcher under `data_home`. Returns the written paths.
pub fn install_under(data_home: &Path, exec: &str) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (size, bytes) in ICONS {
        let path = icon_path_under(data_home, *size);
        write_file(&path, bytes)?;
        written.push(path);
    }
    let desktop = desktop_path_under(data_home);
    write_file(&desktop, launcher_entry(exec).as_bytes())?;
    written.push(desktop);
    Ok(written)
}

/// Remove installed icons + launcher under `data_home`.
pub fn uninstall_under(data_home: &Path) -> Result<()> {
    for (size, _) in ICONS {
        remove_if_present(&icon_path_under(data_home, *size))?;
    }
    remove_if_present(&desktop_path_under(data_home))?;
    Ok(())
}

/// Install using the resolved data home.
pub fn install(exec: &str) -> Result<Vec<PathBuf>> {
    install_under(&data_home(), exec)
}

/// Uninstall using the resolved data home.
pub fn uninstall() -> Result<()> {
    uninstall_under(&data_home())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_entry_has_required_keys() {
        let entry = launcher_entry("/usr/bin/streamdeck tray");
        assert!(entry.contains("Type=Application\n"));
        assert!(entry.contains("Exec=/usr/bin/streamdeck tray\n"));
        assert!(entry.contains("Icon=streamdeck\n"));
    }

    #[test]
    fn icon_and_desktop_paths_follow_hicolor_layout() {
        let home = Path::new("/tmp/share");
        assert_eq!(
            icon_path_under(home, 48),
            PathBuf::from("/tmp/share/icons/hicolor/48x48/apps/streamdeck.png")
        );
        assert_eq!(
            desktop_path_under(home),
            PathBuf::from("/tmp/share/applications/streamdeck.desktop")
        );
    }

    #[test]
    fn install_then_uninstall_round_trips() {
        let dir = std::env::temp_dir().join(format!("sd-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let written = install_under(&dir, "/opt/streamdeck tray").unwrap();
        assert!(written.iter().all(|p| p.exists()));
        // 8 icon sizes + 1 desktop entry.
        assert_eq!(written.len(), 9);
        let desktop = std::fs::read_to_string(desktop_path_under(&dir)).unwrap();
        assert!(desktop.contains("Exec=/opt/streamdeck tray"));

        uninstall_under(&dir).unwrap();
        assert!(written.iter().all(|p| !p.exists()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
