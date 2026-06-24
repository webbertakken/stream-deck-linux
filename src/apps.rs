//! Discover installed desktop applications so a key can "open an application".
//!
//! Apps are launched via `gtk-launch <id>`, which respects the `.desktop`
//! entry (working dir, env, etc.). Icons are resolved to a raster (PNG) file
//! when one exists in the icon theme; SVG-only icons are left unresolved.

use std::path::{Path, PathBuf};

/// An installed desktop application.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DesktopApp {
    /// Desktop id (filename without `.desktop`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Shell command that launches it (`gtk-launch <id>`).
    pub command: String,
    /// Absolute path to a PNG icon, if one could be resolved.
    pub icon: Option<String>,
}

/// Fields pulled from a `.desktop` `[Desktop Entry]` group.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: Option<String>,
    pub exec: Option<String>,
    pub icon: Option<String>,
    pub kind: Option<String>,
    pub no_display: bool,
    pub hidden: bool,
}

/// Parse the `[Desktop Entry]` group of a `.desktop` file.
///
/// Localised keys (`Name[xx]=`) are ignored; only the plain keys are read.
pub fn parse_entry(contents: &str) -> Entry {
    let mut entry = Entry::default();
    let mut in_group = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // Only read the main group; stop at the next group.
            if in_group {
                break;
            }
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "Name" if entry.name.is_none() => entry.name = Some(value),
            "Exec" if entry.exec.is_none() => entry.exec = Some(value),
            "Icon" if entry.icon.is_none() => entry.icon = Some(value),
            "Type" if entry.kind.is_none() => entry.kind = Some(value),
            "NoDisplay" => entry.no_display = value.eq_ignore_ascii_case("true"),
            "Hidden" => entry.hidden = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    entry
}

/// Whether an entry should be offered to the user.
pub fn is_listable(entry: &Entry) -> bool {
    entry.kind.as_deref() == Some("Application")
        && entry.exec.is_some()
        && entry.name.is_some()
        && !entry.no_display
        && !entry.hidden
}

/// Strip `.desktop` Exec field codes (`%u`, `%F`, `%i`, ...).
pub fn clean_exec(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|token| !(token.len() == 2 && token.starts_with('%')))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Directories that may contain `applications/*.desktop`.
fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data_home));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local").join("share"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    dirs.into_iter().map(|d| d.join("applications")).collect()
}

/// Common roots for resolving icon names to PNG files.
fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".local/share/icons"));
        roots.push(PathBuf::from(&home).join(".icons"));
    }
    roots.push(PathBuf::from("/usr/share/icons"));
    roots.push(PathBuf::from("/usr/share/pixmaps"));
    roots
}

/// Resolve an icon name (or path) to an existing PNG file, best-effort.
pub fn resolve_icon_png(icon: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    let direct = Path::new(icon);
    if direct.is_absolute() {
        return (direct.extension().is_some_and(|e| e == "png") && direct.exists())
            .then(|| direct.to_path_buf());
    }

    const SIZES: &[&str] = &[
        "512x512", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32",
    ];
    for root in icon_roots() {
        // Flat dirs like /usr/share/pixmaps.
        let flat = root.join(format!("{icon}.png"));
        if flat.exists() {
            return Some(flat);
        }
        // hicolor theme apps dirs.
        for size in SIZES {
            let themed = root
                .join("hicolor")
                .join(size)
                .join("apps")
                .join(format!("{icon}.png"));
            if themed.exists() {
                return Some(themed);
            }
        }
    }
    None
}

/// List installed, launchable applications, sorted by name and de-duplicated.
pub fn list() -> Vec<DesktopApp> {
    let mut apps: Vec<DesktopApp> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for dir in application_dirs() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if seen.contains(&id) {
                continue; // earlier (more local) dirs win
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            let parsed = parse_entry(&contents);
            if !is_listable(&parsed) {
                continue;
            }
            seen.insert(id.clone());
            apps.push(DesktopApp {
                command: format!("gtk-launch {id}"),
                name: parsed.name.unwrap_or_else(|| id.clone()),
                icon: parsed
                    .icon
                    .as_deref()
                    .and_then(resolve_icon_png)
                    .map(|p| p.to_string_lossy().into_owned()),
                id,
            });
        }
    }

    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Desktop Entry]\n\
        Type=Application\n\
        Name=Firefox\n\
        Name[fr]=Renard\n\
        Exec=firefox %u\n\
        Icon=firefox\n\
        [Desktop Action new-window]\n\
        Name=New Window\n";

    #[test]
    fn parses_main_group_only() {
        let entry = parse_entry(SAMPLE);
        assert_eq!(entry.name.as_deref(), Some("Firefox")); // not the action's Name
        assert_eq!(entry.exec.as_deref(), Some("firefox %u"));
        assert_eq!(entry.icon.as_deref(), Some("firefox"));
        assert_eq!(entry.kind.as_deref(), Some("Application"));
        assert!(is_listable(&entry));
    }

    #[test]
    fn hidden_and_nodisplay_are_not_listable() {
        let mut e = parse_entry(SAMPLE);
        e.no_display = true;
        assert!(!is_listable(&e));
        let mut e2 = parse_entry(SAMPLE);
        e2.hidden = true;
        assert!(!is_listable(&e2));
    }

    #[test]
    fn non_application_is_not_listable() {
        let entry = parse_entry("[Desktop Entry]\nType=Link\nName=X\nExec=x\n");
        assert!(!is_listable(&entry));
    }

    #[test]
    fn clean_exec_strips_field_codes() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(clean_exec("code --new-window %F"), "code --new-window");
        assert_eq!(clean_exec("foo %i %c bar"), "foo bar");
    }

    #[test]
    fn resolve_icon_rejects_missing_and_non_png() {
        assert_eq!(resolve_icon_png(""), None);
        assert_eq!(resolve_icon_png("/no/such/icon.svg"), None);
        assert_eq!(resolve_icon_png("/no/such/icon.png"), None);
    }
}
