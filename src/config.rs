//! Declarative configuration: map each key to a picture and a function.
//!
//! A config is TOML, e.g.:
//!
//! ```toml
//! brightness = 70
//!
//! [[buttons]]
//! key = 0
//! image = "icons/terminal.png"   # relative to the config file
//! run = "alacritty"
//!
//! [[buttons]]
//! key = 4
//! color = "#1e1e2e"
//! run = "amixer set Master toggle"
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::actions::Builtin;
use crate::error::{Error, Result};
use crate::model::Model;

/// A full Stream Deck layout: optional brightness, plus either a single page of
/// `buttons` or several named `pages`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Display brightness percentage to apply on load (`0..=100`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// Single-page shorthand: the buttons of the one and only page. Mutually
    /// exclusive with `pages`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<ButtonConfig>,
    /// Multi-page layout. Switch with the `page_next` / `page_prev` /
    /// `page:<name|index>` built-ins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,
}

/// One page of a multi-page layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// Optional page name (referenced by the `page:<name>` built-in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The page's per-key buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<ButtonConfig>,
}

/// One key's picture and function.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonConfig {
    /// Hardware key index.
    pub key: u8,
    /// Picture file to render onto the key (relative paths resolve against the
    /// config file's directory; `~` expands to `$HOME`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
    /// Solid colour fallback as `#RRGGBB` or `RRGGBB`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Shell command to run when the key is pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Built-in action to trigger when the key is pressed (e.g.
    /// `brightness_up`, `brightness_down`, `brightness_set:70`, `reset`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    /// A sequence of shell commands run in order on press.
    #[serde(default, rename = "macro", skip_serializing_if = "Option::is_none")]
    pub macro_steps: Option<Vec<String>>,
    /// Optional text label drawn centred on the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Colour of the label text as `#RRGGBB` (defaults to white).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    /// Shell command whose stdout becomes the key's label, refreshed every
    /// [`Self::interval`] seconds (a "live" key, e.g. a clock or CPU meter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<String>,
    /// Refresh interval in seconds for `watch` (default 5, minimum 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    /// Toggle key: a list of states cycled on each press, each with its own
    /// visual and action. Mutually exclusive with run/builtin/macro.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub states: Option<Vec<ButtonState>>,
}

/// One state of a toggle key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
}

impl ButtonState {
    /// Build a plain [`ButtonConfig`] for this state on `key`, for rendering /
    /// dispatch / validation reuse.
    pub fn to_button(&self, key: u8) -> ButtonConfig {
        ButtonConfig {
            key,
            image: self.image.clone(),
            color: self.color.clone(),
            label: self.label.clone(),
            text_color: self.text_color.clone(),
            run: self.run.clone(),
            builtin: self.builtin.clone(),
            ..Default::default()
        }
    }
}

impl Config {
    /// Parse a config from a TOML string.
    pub fn from_toml_str(toml_str: &str) -> Result<Self> {
        Ok(toml::from_str(toml_str)?)
    }

    /// Load and parse a config file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_toml_str(&contents)
    }

    /// Serialise the config back to a TOML string.
    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|err| Error::ConfigInvalid(err.to_string()))
    }

    /// The normalised page list: explicit `pages` if present, otherwise the
    /// top-level `buttons` wrapped into a single unnamed page.
    pub fn pages(&self) -> Vec<Page> {
        if self.pages.is_empty() {
            vec![Page {
                name: None,
                buttons: self.buttons.clone(),
            }]
        } else {
            self.pages.clone()
        }
    }

    /// Validate the config against a model: keys in range, no duplicates per
    /// page, brightness sane, every button visually defined, and page targets
    /// that exist.
    pub fn validate(&self, model: &Model) -> Result<()> {
        if let Some(b) = self.brightness {
            if b > 100 {
                return Err(Error::ConfigInvalid(format!(
                    "brightness {b} out of range (0-100)"
                )));
            }
        }
        if !self.buttons.is_empty() && !self.pages.is_empty() {
            return Err(Error::ConfigInvalid(
                "set either top-level `buttons` or `pages`, not both".into(),
            ));
        }

        let pages = self.pages();
        let names: Vec<Option<String>> = pages.iter().map(|p| p.name.clone()).collect();
        for page in &pages {
            validate_buttons(&page.buttons, model, pages.len(), &names)?;
        }
        Ok(())
    }
}

/// Resolve a `page:` target (a 0-based index or a page name) to a page index.
pub fn resolve_page_target(
    target: &str,
    page_count: usize,
    names: &[Option<String>],
) -> Option<usize> {
    let target = target.trim();
    if let Ok(index) = target.parse::<usize>() {
        return (index < page_count).then_some(index);
    }
    names.iter().position(|n| n.as_deref() == Some(target))
}

fn validate_buttons(
    buttons: &[ButtonConfig],
    model: &Model,
    page_count: usize,
    names: &[Option<String>],
) -> Result<()> {
    let mut seen = vec![false; model.key_count as usize];
    for button in buttons {
        if !model.is_valid_key(button.key) {
            return Err(Error::ConfigInvalid(format!(
                "key {} out of range (device has {} keys)",
                button.key, model.key_count
            )));
        }
        let slot = &mut seen[button.key as usize];
        if *slot {
            return Err(Error::ConfigInvalid(format!(
                "key {} configured more than once",
                button.key
            )));
        }
        *slot = true;

        if let Some(states) = &button.states {
            if states.is_empty() {
                return Err(Error::ConfigInvalid(format!(
                    "key {} has no states",
                    button.key
                )));
            }
            if button.run.is_some() || button.builtin.is_some() || button.macro_steps.is_some() {
                return Err(Error::ConfigInvalid(format!(
                    "key {} sets both states and another action",
                    button.key
                )));
            }
            for state in states {
                validate_one(&state.to_button(button.key), page_count, names)?;
            }
        } else {
            validate_one(button, page_count, names)?;
        }
    }
    Ok(())
}

/// Validate a single button's visual + action (shared by plain buttons and
/// toggle states).
fn validate_one(button: &ButtonConfig, page_count: usize, names: &[Option<String>]) -> Result<()> {
    if button.image.is_none()
        && button.color.is_none()
        && button.label.is_none()
        && button.watch.is_none()
    {
        return Err(Error::ConfigInvalid(format!(
            "key {} has no image, color, label or watch",
            button.key
        )));
    }
    let _ = button.rgb()?;
    let _ = button.text_rgb()?;

    let action_count = [
        button.run.is_some(),
        button.builtin.is_some(),
        button.macro_steps.is_some(),
    ]
    .iter()
    .filter(|set| **set)
    .count();
    if action_count > 1 {
        return Err(Error::ConfigInvalid(format!(
            "key {} sets more than one action (run/builtin/macro)",
            button.key
        )));
    }
    if let Some(steps) = &button.macro_steps {
        if steps.iter().all(|s| s.trim().is_empty()) {
            return Err(Error::ConfigInvalid(format!(
                "key {} macro has no steps",
                button.key
            )));
        }
    }
    if let Some(spec) = &button.builtin {
        if let Builtin::Page(target) = Builtin::parse(spec)? {
            if resolve_page_target(&target, page_count, names).is_none() {
                return Err(Error::ConfigInvalid(format!(
                    "key {} targets unknown page '{target}'",
                    button.key
                )));
            }
        }
    }
    Ok(())
}

impl ButtonConfig {
    /// Resolve the image path against the config's base directory, expanding
    /// a leading `~` to `$HOME`.
    pub fn resolved_image(&self, base_dir: &Path) -> Option<PathBuf> {
        self.image.as_ref().map(|path| {
            let expanded = expand_tilde(path);
            if expanded.is_absolute() {
                expanded
            } else {
                base_dir.join(expanded)
            }
        })
    }

    /// Parse the colour field into RGB, if present.
    pub fn rgb(&self) -> Result<Option<[u8; 3]>> {
        parse_optional_hex(self.color.as_deref())
    }

    /// Parse the text colour field into RGB, if present.
    pub fn text_rgb(&self) -> Result<Option<[u8; 3]>> {
        parse_optional_hex(self.text_color.as_deref())
    }
}

fn parse_optional_hex(value: Option<&str>) -> Result<Option<[u8; 3]>> {
    match value {
        None => Ok(None),
        Some(hex) => parse_hex_color(hex)
            .map(Some)
            .ok_or_else(|| Error::ConfigInvalid(format!("invalid colour '{hex}'"))),
    }
}

/// Parse `#RRGGBB` or `RRGGBB` into an RGB triple.
pub fn parse_hex_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some([
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ])
}

/// Expand a leading `~` in a path to `$HOME`.
fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
brightness = 70

[[buttons]]
key = 0
image = "icons/terminal.png"
run = "alacritty"

[[buttons]]
key = 4
color = "#1e1e2e"
run = "amixer set Master toggle"
"##;

    #[test]
    fn toml_round_trips() {
        let config = Config::from_toml_str(SAMPLE).unwrap();
        let serialised = config.to_toml_string().unwrap();
        let reparsed = Config::from_toml_str(&serialised).unwrap();
        assert_eq!(config, reparsed);
        // None fields must not be emitted as empty keys.
        assert!(!serialised.contains("image = \"\""));
    }

    #[test]
    fn parses_brightness_and_buttons() {
        let config = Config::from_toml_str(SAMPLE).unwrap();
        assert_eq!(config.brightness, Some(70));
        assert_eq!(config.buttons.len(), 2);
        assert_eq!(config.buttons[0].key, 0);
        assert_eq!(
            config.buttons[0].image.as_deref(),
            Some(Path::new("icons/terminal.png"))
        );
        assert_eq!(config.buttons[0].run.as_deref(), Some("alacritty"));
        assert_eq!(config.buttons[1].color.as_deref(), Some("#1e1e2e"));
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = Config::from_toml_str("[[buttons]]\nkey = 0\nbogus = 1\n").unwrap_err();
        assert!(matches!(err, Error::ConfigParse(_)));
    }

    #[test]
    fn validate_accepts_well_formed_config() {
        let config = Config::from_toml_str(SAMPLE).unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
    }

    #[test]
    fn validate_rejects_key_out_of_range() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 99\ncolor = \"#fff000\"\n").unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("out of range")));
    }

    #[test]
    fn validate_rejects_duplicate_keys() {
        let toml = "[[buttons]]\nkey = 1\ncolor = \"#ffffff\"\n\n[[buttons]]\nkey = 1\ncolor = \"#000000\"\n";
        let config = Config::from_toml_str(toml).unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("more than once")));
    }

    #[test]
    fn validate_rejects_button_without_visual() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 2\nrun = \"true\"\n").unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("no image, color")));
    }

    #[test]
    fn validate_accepts_toggle_states() {
        let toml = "[[buttons]]\nkey = 0\n[[buttons.states]]\nlabel = \"On\"\ncolor = \"#40a02b\"\nrun = \"echo on\"\n[[buttons.states]]\nlabel = \"Off\"\ncolor = \"#e64553\"\nrun = \"echo off\"\n";
        let config = Config::from_toml_str(toml).unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
        assert_eq!(config.buttons[0].states.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn validate_rejects_empty_states() {
        let toml = "[[buttons]]\nkey = 0\nstates = []\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("no states")));
    }

    #[test]
    fn validate_rejects_state_without_visual() {
        let toml = "[[buttons]]\nkey = 0\n[[buttons.states]]\nrun = \"echo x\"\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("no image, color")));
    }

    #[test]
    fn validate_rejects_states_with_run() {
        let toml = "[[buttons]]\nkey = 0\nrun = \"x\"\n[[buttons.states]]\nlabel = \"A\"\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(
            matches!(err, Error::ConfigInvalid(m) if m.contains("both states and another action"))
        );
    }

    #[test]
    fn validate_accepts_watch_as_visual() {
        let toml = "[[buttons]]\nkey = 0\nwatch = \"date +%H:%M\"\ninterval = 60\n";
        let config = Config::from_toml_str(toml).unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
        assert_eq!(config.buttons[0].watch.as_deref(), Some("date +%H:%M"));
        assert_eq!(config.buttons[0].interval, Some(60));
    }

    #[test]
    fn validate_accepts_builtin_button() {
        let config = Config::from_toml_str(
            "[[buttons]]\nkey = 1\nlabel = \"Dim\"\nbuiltin = \"brightness_down\"\n",
        )
        .unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
    }

    #[test]
    fn validate_rejects_unknown_builtin() {
        let config =
            Config::from_toml_str("[[buttons]]\nkey = 1\nlabel = \"x\"\nbuiltin = \"nope\"\n")
                .unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("unknown builtin")));
    }

    #[test]
    fn validate_rejects_run_and_builtin_together() {
        let config = Config::from_toml_str(
            "[[buttons]]\nkey = 1\nlabel = \"x\"\nrun = \"true\"\nbuiltin = \"reset\"\n",
        )
        .unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("more than one action")));
    }

    #[test]
    fn validate_accepts_macro() {
        let toml = "[[buttons]]\nkey = 0\nlabel = \"Macro\"\nmacro = [\"echo a\", \"echo b\"]\n";
        let config = Config::from_toml_str(toml).unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
        assert_eq!(
            config.buttons[0].macro_steps.as_deref(),
            Some(["echo a".to_string(), "echo b".to_string()].as_slice())
        );
    }

    #[test]
    fn validate_rejects_macro_with_run() {
        let toml = "[[buttons]]\nkey = 0\nlabel = \"x\"\nrun = \"true\"\nmacro = [\"echo a\"]\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("more than one action")));
    }

    #[test]
    fn validate_rejects_empty_macro() {
        let toml = "[[buttons]]\nkey = 0\nlabel = \"x\"\nmacro = [\"  \"]\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("macro has no steps")));
    }

    #[test]
    fn validate_accepts_label_only_button() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 2\nlabel = \"Mute\"\n").unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
    }

    #[test]
    fn pages_wraps_top_level_buttons() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 0\nlabel = \"A\"\n").unwrap();
        let pages = config.pages();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].name, None);
        assert_eq!(pages[0].buttons.len(), 1);
    }

    #[test]
    fn parses_and_validates_multi_page() {
        let toml = "[[pages]]\nname = \"main\"\n[[pages.buttons]]\nkey = 0\nlabel = \"Go\"\nbuiltin = \"page:media\"\n\n[[pages]]\nname = \"media\"\n[[pages.buttons]]\nkey = 0\nlabel = \"Back\"\nbuiltin = \"page_prev\"\n";
        let config = Config::from_toml_str(toml).unwrap();
        assert_eq!(config.pages().len(), 2);
        assert!(config.validate(&Model::MK2).is_ok());
    }

    #[test]
    fn validate_rejects_both_buttons_and_pages() {
        let toml = "[[buttons]]\nkey = 0\nlabel = \"x\"\n[[pages]]\nname = \"p\"\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("not both")));
    }

    #[test]
    fn validate_rejects_unknown_page_target() {
        let toml = "[[pages]]\nname = \"main\"\n[[pages.buttons]]\nkey = 0\nlabel = \"x\"\nbuiltin = \"page:ghost\"\n";
        let err = Config::from_toml_str(toml)
            .unwrap()
            .validate(&Model::MK2)
            .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("unknown page")));
    }

    #[test]
    fn page_target_resolves_by_index_and_name() {
        let names = vec![Some("main".to_string()), Some("media".to_string())];
        assert_eq!(resolve_page_target("1", 2, &names), Some(1));
        assert_eq!(resolve_page_target("media", 2, &names), Some(1));
        assert_eq!(resolve_page_target("9", 2, &names), None);
        assert_eq!(resolve_page_target("ghost", 2, &names), None);
    }

    #[test]
    fn validate_rejects_bad_text_colour() {
        let config =
            Config::from_toml_str("[[buttons]]\nkey = 2\nlabel = \"x\"\ntext_color = \"zzz\"\n")
                .unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("invalid colour")));
    }

    #[test]
    fn validate_rejects_bad_colour() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 2\ncolor = \"nothex\"\n").unwrap();
        let err = config.validate(&Model::MK2).unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("invalid colour")));
    }

    #[test]
    fn hex_colour_parsing_handles_prefix_and_validation() {
        assert_eq!(parse_hex_color("#FF8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex_color("00ff00"), Some([0, 255, 0]));
        assert_eq!(parse_hex_color("  #000000 "), Some([0, 0, 0]));
        assert_eq!(parse_hex_color("fff"), None);
        assert_eq!(parse_hex_color("gggggg"), None);
    }

    #[test]
    fn relative_image_resolves_against_base_dir() {
        let button = ButtonConfig {
            image: Some(PathBuf::from("icons/x.png")),
            ..Default::default()
        };
        let resolved = button.resolved_image(Path::new("/etc/streamdeck")).unwrap();
        assert_eq!(resolved, PathBuf::from("/etc/streamdeck/icons/x.png"));
    }

    #[test]
    fn absolute_image_path_is_kept() {
        let button = ButtonConfig {
            image: Some(PathBuf::from("/abs/x.png")),
            ..Default::default()
        };
        let resolved = button.resolved_image(Path::new("/etc/streamdeck")).unwrap();
        assert_eq!(resolved, PathBuf::from("/abs/x.png"));
    }

    #[test]
    fn tilde_image_path_expands_to_home() {
        std::env::set_var("HOME", "/home/tester");
        let button = ButtonConfig {
            image: Some(PathBuf::from("~/pics/x.png")),
            ..Default::default()
        };
        let resolved = button.resolved_image(Path::new("/ignored")).unwrap();
        assert_eq!(resolved, PathBuf::from("/home/tester/pics/x.png"));
    }
}
