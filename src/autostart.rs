//! Manage login autostart via a freedesktop `.desktop` entry in
//! `~/.config/autostart`.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Basename of the autostart entry.
pub const ENTRY_NAME: &str = "streamdeck.desktop";

/// Build the autostart `.desktop` file contents.
///
/// `exec` is the command to run on login (e.g. `/usr/local/bin/streamdeck tray`)
/// and `icon` is an icon name or absolute path.
pub fn desktop_entry(exec: &str, icon: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Stream Deck\n\
         Comment=Custom Stream Deck control\n\
         Exec={exec}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Utility;\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// Resolve `$XDG_CONFIG_HOME`, falling back to `~/.config`.
fn config_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".config")
}

/// Path to the autostart entry under a given config home (pure).
pub fn entry_path_under(config_home: &Path) -> PathBuf {
    config_home.join("autostart").join(ENTRY_NAME)
}

/// Path to the autostart entry, honouring `$XDG_CONFIG_HOME`.
pub fn entry_path() -> PathBuf {
    entry_path_under(&config_home())
}

/// Whether autostart is currently enabled (the entry exists).
pub fn is_enabled() -> bool {
    entry_path().exists()
}

/// Write the autostart entry under `config_home`, creating dirs as needed.
pub fn enable_under(config_home: &Path, exec: &str, icon: &str) -> Result<PathBuf> {
    let path = entry_path_under(config_home);
    let parent = path
        .parent()
        .ok_or_else(|| Error::ConfigInvalid("autostart path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(&path, desktop_entry(exec, icon))?;
    Ok(path)
}

/// Remove the autostart entry under `config_home`. Returns whether it existed.
pub fn disable_under(config_home: &Path) -> Result<bool> {
    let path = entry_path_under(config_home);
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Write the autostart entry (honouring `$XDG_CONFIG_HOME`).
pub fn enable(exec: &str, icon: &str) -> Result<PathBuf> {
    enable_under(&config_home(), exec, icon)
}

/// Remove the autostart entry if present. Returns whether it existed.
pub fn disable() -> Result<bool> {
    disable_under(&config_home())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_has_required_keys() {
        let entry = desktop_entry("/usr/bin/streamdeck tray", "streamdeck");
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Type=Application\n"));
        assert!(entry.contains("Exec=/usr/bin/streamdeck tray\n"));
        assert!(entry.contains("Icon=streamdeck\n"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true\n"));
    }

    #[test]
    fn entry_path_under_builds_expected_location() {
        assert_eq!(
            entry_path_under(Path::new("/tmp/xdgcfg")),
            PathBuf::from("/tmp/xdgcfg/autostart/streamdeck.desktop")
        );
    }

    #[test]
    fn enable_then_disable_round_trips() {
        let dir = std::env::temp_dir().join(format!("sd-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let path = enable_under(&dir, "/opt/streamdeck tray", "streamdeck").unwrap();
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Exec=/opt/streamdeck tray"));

        assert!(disable_under(&dir).unwrap());
        assert!(!path.exists());
        assert!(!disable_under(&dir).unwrap()); // already gone

        let _ = std::fs::remove_dir_all(&dir);
    }
}
