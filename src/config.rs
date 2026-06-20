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

/// A full Stream Deck layout: optional brightness plus per-key buttons.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Display brightness percentage to apply on load (`0..=100`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// Per-key configuration.
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
    /// Optional text label drawn centred on the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Colour of the label text as `#RRGGBB` (defaults to white).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
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

    /// Validate the config against a model: keys in range, no duplicates,
    /// brightness sane, and every button visually defined.
    pub fn validate(&self, model: &Model) -> Result<()> {
        if let Some(b) = self.brightness {
            if b > 100 {
                return Err(Error::ConfigInvalid(format!(
                    "brightness {b} out of range (0-100)"
                )));
            }
        }

        let mut seen = vec![false; model.key_count as usize];
        for button in &self.buttons {
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

            if button.image.is_none() && button.color.is_none() && button.label.is_none() {
                return Err(Error::ConfigInvalid(format!(
                    "key {} has no image, color or label",
                    button.key
                )));
            }
            // Surface bad colours at validation time, not mid-render.
            let _ = button.rgb()?;
            let _ = button.text_rgb()?;

            if button.run.is_some() && button.builtin.is_some() {
                return Err(Error::ConfigInvalid(format!(
                    "key {} sets both run and builtin (choose one)",
                    button.key
                )));
            }
            if let Some(spec) = &button.builtin {
                Builtin::parse(spec)?;
            }
        }
        Ok(())
    }
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
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("no image, color or label")));
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
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("both run and builtin")));
    }

    #[test]
    fn validate_accepts_label_only_button() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 2\nlabel = \"Mute\"\n").unwrap();
        assert!(config.validate(&Model::MK2).is_ok());
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
