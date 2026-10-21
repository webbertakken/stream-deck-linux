//! The deck's design language, as data for programs (`GET /api/v1/style`,
//! `streamdeck style`) and in words in `docs/style.md`.
//!
//! The tests below hold the two together, and prove every palette colour
//! carries white icons and labels at WCAG 2.2 AA contrast.

use serde_json::{json, Value};

use crate::config::parse_hex_color;

/// A background colour and what it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swatch {
    pub name: &'static str,
    pub hex: &'static str,
    pub purpose: &'static str,
}

/// Key backgrounds: dark, so the white icon and label stand out.
pub const PALETTE: &[Swatch] = &[
    Swatch {
        name: "apps",
        hex: "#2E5A88",
        purpose: "launching programs",
    },
    Swatch {
        name: "media",
        hex: "#6B3FA0",
        purpose: "playback and volume",
    },
    Swatch {
        name: "system",
        hex: "#3A3F4B",
        purpose: "deck, screens, power, brightness",
    },
    Swatch {
        name: "home",
        hex: "#2F7D4F",
        purpose: "lights, devices, the house",
    },
    Swatch {
        name: "runa",
        hex: "#7A4E0A",
        purpose: "anything that asks Runa or her plugins",
    },
    Swatch {
        name: "alert",
        hex: "#A33A3A",
        purpose: "stop, mute, anything to press with care",
    },
];

/// The colour of every icon and label: the renderer's default text colour.
pub const FOREGROUND: [u8; 3] = crate::render::DEFAULT_TEXT_COLOR;

/// WCAG 2.2 AA for normal text.
pub const MINIMUM_CONTRAST: f64 = 4.5;

/// The most characters a line of a label may hold; a program's key with a longer line is refused.
pub const LABEL_LINE_MAX: usize = 10;

/// Why a label breaks the style, or `None` when every line fits.
pub fn label_fault(label: &str) -> Option<String> {
    let longest = label.split('\n').map(|line| line.chars().count()).max()?;
    (longest > LABEL_LINE_MAX).then(|| {
        format!(
            "label '{label}' has a line of {longest} characters; the deck's style allows at most {LABEL_LINE_MAX} (streamdeck style)"
        )
    })
}

/// How a label reads.
pub const LABEL_RULES: &[&str] = &[
    "One or two words, in title case.",
    "Say what pressing the key does.",
    "At most 10 characters a line.",
    "White on the key's palette colour: leave text_color unset.",
];

/// How a key's icon is chosen.
pub const ICON_RULES: &[&str] = &[
    "Every key gets an icon when one fits.",
    "One icon per key.",
    "The icon and the label say the same thing.",
    "White, like the label.",
];

/// Where keys go on the deck.
pub const LAYOUT_RULES: &[&str] = &[
    "Related keys sit together in a row.",
    "Page navigation keys sit on the bottom row's ends.",
];

/// The style as the API answers it.
pub fn style() -> Value {
    let palette: Vec<Value> = PALETTE
        .iter()
        .map(|s| json!({ "name": s.name, "hex": s.hex, "for": s.purpose }))
        .collect();
    json!({
        "palette": palette,
        "labels": LABEL_RULES,
        "icons": ICON_RULES,
        "layout": LAYOUT_RULES,
    })
}

/// WCAG 2.2 relative luminance of an sRGB colour.
pub fn relative_luminance(rgb: [u8; 3]) -> f64 {
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

/// WCAG 2.2 contrast ratio between two colours, from 1 to 21.
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The contrast of white icons and labels on a swatch.
pub fn foreground_contrast(swatch: &Swatch) -> f64 {
    let background = parse_hex_color(swatch.hex).expect("a palette hex is #RRGGBB");
    contrast_ratio(FOREGROUND, background)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_a_label_line_up_to_its_limit_and_names_one_past_it() {
        assert_eq!(label_fault("Mail"), None);
        assert_eq!(label_fault("Screens"), None);
        assert_eq!(label_fault("Abcdefghij"), None);
        assert_eq!(label_fault("Two\nLines"), None);
        assert_eq!(label_fault("Kühlschrank"), Some(
            "label 'Kühlschrank' has a line of 11 characters; the deck's style allows at most 10 (streamdeck style)".to_string()
        ));
        assert!(label_fault("Ok\nThunderbird")
            .unwrap()
            .contains("a line of 11 characters"));
    }

    #[test]
    fn says_its_label_limit_in_its_rules() {
        assert!(LABEL_RULES
            .iter()
            .any(|rule| rule.contains(&format!("At most {LABEL_LINE_MAX} characters a line"))));
    }

    const DOC: &str = include_str!("../docs/style.md");

    #[test]
    fn measures_contrast_as_wcag_does() {
        assert!((contrast_ratio([255; 3], [0; 3]) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio([255; 3], [255; 3]) - 1.0).abs() < 1e-9);
        let grey = contrast_ratio([255; 3], [0x77; 3]);
        assert!((grey - 4.48).abs() < 0.01, "#777 on white is {grey}");
        assert_eq!(
            contrast_ratio([0; 3], [0x77; 3]),
            contrast_ratio([0x77; 3], [0; 3])
        );
    }

    #[test]
    fn every_palette_colour_carries_white_at_aa_contrast() {
        assert_eq!(FOREGROUND, [255, 255, 255]);
        for swatch in PALETTE {
            let ratio = foreground_contrast(swatch);
            assert!(
                ratio >= MINIMUM_CONTRAST,
                "white on {} {} is only {ratio:.2}:1",
                swatch.name,
                swatch.hex
            );
        }
    }

    #[test]
    fn names_each_swatch_once_with_a_purpose() {
        let mut names: Vec<&str> = PALETTE.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PALETTE.len());
        assert!(PALETTE
            .iter()
            .all(|s| !s.purpose.is_empty() && parse_hex_color(s.hex).is_some()));
    }

    #[test]
    fn answers_the_palette_and_rules_in_their_shape() {
        let value = style();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["icons", "labels", "layout", "palette"]);
        let apps = &value["palette"][0];
        assert_eq!(
            apps,
            &json!({ "name": "apps", "hex": "#2E5A88", "for": "launching programs" })
        );
        assert_eq!(value["palette"].as_array().unwrap().len(), PALETTE.len());
        assert_eq!(value["labels"].as_array().unwrap().len(), LABEL_RULES.len());
        assert_eq!(value["icons"][0], ICON_RULES[0]);
        assert_eq!(value["layout"][1], LAYOUT_RULES[1]);
    }

    #[test]
    fn the_written_example_is_a_valid_layout() {
        let example = DOC
            .split("```toml\n")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .expect("docs/style.md has a toml example");
        let config = crate::config::Config::from_toml_str(example).unwrap();
        config.validate(&crate::model::Model::MK2).unwrap();
        assert!(config.buttons.iter().all(|b| b.icon.is_some()));
    }

    #[test]
    fn the_written_style_says_what_the_data_says() {
        for swatch in PALETTE {
            let row = format!(
                "| `{}` | `{}` | {} | {:.2}:1 |",
                swatch.name,
                swatch.hex,
                swatch.purpose,
                foreground_contrast(swatch)
            );
            assert!(DOC.contains(&row), "docs/style.md lacks the row:\n{row}");
        }
        for rule in LABEL_RULES.iter().chain(ICON_RULES).chain(LAYOUT_RULES) {
            assert!(
                DOC.contains(&format!("- {rule}\n")),
                "docs/style.md lacks the rule: {rule}"
            );
        }
    }
}
