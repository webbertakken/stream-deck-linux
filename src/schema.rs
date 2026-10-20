//! What this build of the deck lets a key hold, for callers that read it
//! instead of hard-coding it (`GET /api/v1/schema`, `streamdeck schema`).
//!
//! The tests below fail when a button field or a built-in is added without
//! being described here, so the schema cannot drift from the code.

use serde_json::{json, Value};

/// The control API's version.
pub const API_VERSION: u32 = 1;

/// Every field of a `[[buttons]]` entry, with its meaning.
pub const FIELDS: &[(&str, &str)] = &[
    ("key", "Hardware key index, 0-based, left to right then top to bottom (set by the route)."),
    ("image", "Picture file on the host (PNG/JPEG); relative paths resolve next to the config, ~ is $HOME."),
    ("color", "Solid background, #RRGGBB."),
    ("label", "Centred text drawn over the background, shrunk to fit."),
    ("text_color", "Label colour, #RRGGBB (default white)."),
    ("run", "Shell command run on press (sh -c)."),
    ("builtin", "A built-in action on press; see `builtins`."),
    ("macro", "Array of shell commands run in order on press."),
    ("watch", "Shell command whose stdout becomes the label, refreshed every `interval` seconds."),
    ("interval", "Refresh seconds for `watch` (default 5, minimum 1)."),
    ("states", "Toggle key: array of states cycled on press, each with label, color, image, text_color and one of run or builtin."),
    ("hold", "Keyboard key or `+` combo held down on press and released on the next press; see `hold_keys`."),
];

/// Rules a button must meet; the daemon refuses a change that breaks one.
pub const RULES: &[&str] = &[
    "A key needs at least one of image, color, label or watch.",
    "A key has at most one action: run, builtin, macro, hold, or states.",
    "A key appears at most once per page.",
    "Unknown fields are refused.",
];

/// Every built-in action, as `(pattern, meaning)`.
pub const BUILTINS: &[(&str, &str)] = &[
    ("brightness_up", "Brightness +10%."),
    ("brightness_down", "Brightness -10%."),
    ("brightness_max", "Brightness 100%."),
    ("brightness_min", "Brightness 0%."),
    (
        "brightness_set:<0-100>",
        "Brightness to the given percentage.",
    ),
    ("media_play_pause", "Play or pause (playerctl)."),
    ("media_next", "Next track."),
    ("media_prev", "Previous track."),
    ("volume_up", "Volume up."),
    ("volume_down", "Volume down."),
    ("volume_mute", "Toggle mute."),
    (
        "open:<url|file>",
        "Open with the desktop's default handler.",
    ),
    ("page_next", "Next page (wraps)."),
    ("page_prev", "Previous page (wraps)."),
    ("page:<name|index>", "Switch to a page."),
    ("reset", "Reset the device to its standby logo."),
];

/// The schema as the API answers it.
pub fn schema() -> Value {
    let fields: serde_json::Map<String, Value> = FIELDS
        .iter()
        .map(|(name, meaning)| ((*name).to_string(), json!(meaning)))
        .collect();
    let builtins: Vec<Value> = BUILTINS
        .iter()
        .map(|(pattern, meaning)| json!({ "pattern": pattern, "meaning": meaning }))
        .collect();
    json!({
        "api": API_VERSION,
        "fields": fields,
        "rules": RULES,
        "builtins": builtins,
        "hold_keys": crate::keyboard::key_names(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Builtin;
    use crate::config::{ButtonConfig, ButtonState};
    use crate::system::{Media, Volume};

    #[test]
    fn describes_every_button_field_and_nothing_else() {
        let full = ButtonConfig {
            key: 0,
            image: Some("x.png".into()),
            color: Some("#000000".into()),
            run: Some("x".into()),
            builtin: Some("reset".into()),
            macro_steps: Some(vec!["x".into()]),
            label: Some("x".into()),
            text_color: Some("#ffffff".into()),
            watch: Some("x".into()),
            interval: Some(5),
            states: Some(vec![ButtonState::default()]),
            hold: Some("a".into()),
        };
        let value = serde_json::to_value(&full).unwrap();
        let mut serialised: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut described: Vec<&str> = FIELDS.iter().map(|(n, _)| *n).collect();
        serialised.sort_unstable();
        described.sort_unstable();
        assert_eq!(serialised, described);
    }

    /// Exhaustive: a new `Builtin` variant fails to compile here until it is
    /// given its pattern, which must then be listed in `BUILTINS`.
    fn pattern_of(builtin: &Builtin) -> &'static str {
        match builtin {
            Builtin::BrightnessUp => "brightness_up",
            Builtin::BrightnessDown => "brightness_down",
            Builtin::BrightnessMax => "brightness_max",
            Builtin::BrightnessMin => "brightness_min",
            Builtin::BrightnessSet(_) => "brightness_set:<0-100>",
            Builtin::Media(Media::PlayPause) => "media_play_pause",
            Builtin::Media(Media::Next) => "media_next",
            Builtin::Media(Media::Prev) => "media_prev",
            Builtin::Volume(Volume::Up) => "volume_up",
            Builtin::Volume(Volume::Down) => "volume_down",
            Builtin::Volume(Volume::Mute) => "volume_mute",
            Builtin::Open(_) => "open:<url|file>",
            Builtin::PageNext => "page_next",
            Builtin::PagePrev => "page_prev",
            Builtin::Page(_) => "page:<name|index>",
            Builtin::Reset => "reset",
        }
    }

    #[test]
    fn lists_every_builtin_by_a_pattern_that_parses() {
        for (pattern, _) in BUILTINS {
            let example = pattern
                .replace("<0-100>", "50")
                .replace("<url|file>", "https://example.com")
                .replace("<name|index>", "0");
            let parsed = Builtin::parse(&example).unwrap_or_else(|e| panic!("{pattern}: {e}"));
            assert_eq!(pattern_of(&parsed), *pattern);
        }
    }

    #[test]
    fn answers_the_api_version_rules_and_hold_keys() {
        let value = schema();
        assert_eq!(value["api"], 1);
        assert_eq!(value["fields"].as_object().unwrap().len(), FIELDS.len());
        assert_eq!(value["builtins"].as_array().unwrap().len(), BUILTINS.len());
        let keys = value["hold_keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k == "ctrl") && keys.iter().any(|k| k == "f12"));
        assert!(!value["rules"].as_array().unwrap().is_empty());
    }
}
