//! Built-in actions a key can trigger, in addition to shell commands.
//!
//! Built-ins act on the deck itself (brightness, reset) - things a plain shell
//! command cannot reach. Shell commands cover everything else (launch apps,
//! `playerctl`, `wpctl`, `ydotool`, ...).

use crate::error::{Error, Result};
use crate::system::{Media, Volume};

/// Brightness step applied by `brightness_up` / `brightness_down`.
pub const BRIGHTNESS_STEP: u8 = 10;

/// A device-native action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Builtin {
    /// Increase brightness by [`BRIGHTNESS_STEP`].
    BrightnessUp,
    /// Decrease brightness by [`BRIGHTNESS_STEP`].
    BrightnessDown,
    /// Set brightness to an absolute percentage.
    BrightnessSet(u8),
    /// Set brightness to 100%.
    BrightnessMax,
    /// Set brightness to 0%.
    BrightnessMin,
    /// Reset the device to its standby logo.
    Reset,
    /// Open a file or URL with the desktop default handler.
    Open(String),
    /// Media transport control (needs `playerctl`).
    Media(Media),
    /// System volume control (wpctl / pactl / amixer).
    Volume(Volume),
}

impl Builtin {
    /// Parse a built-in from its config string.
    ///
    /// Accepted: `brightness_up`, `brightness_down`, `brightness_set:N`
    /// (also `brightness:N`), `reset`.
    pub fn parse(spec: &str) -> Result<Self> {
        let spec = spec.trim();
        match spec {
            "brightness_up" => return Ok(Builtin::BrightnessUp),
            "brightness_down" => return Ok(Builtin::BrightnessDown),
            "brightness_max" => return Ok(Builtin::BrightnessMax),
            "brightness_min" => return Ok(Builtin::BrightnessMin),
            "reset" => return Ok(Builtin::Reset),
            "media_play_pause" => return Ok(Builtin::Media(Media::PlayPause)),
            "media_next" => return Ok(Builtin::Media(Media::Next)),
            "media_prev" => return Ok(Builtin::Media(Media::Prev)),
            "volume_up" => return Ok(Builtin::Volume(Volume::Up)),
            "volume_down" => return Ok(Builtin::Volume(Volume::Down)),
            "volume_mute" => return Ok(Builtin::Volume(Volume::Mute)),
            _ => {}
        }
        if let Some(value) = spec
            .strip_prefix("brightness_set:")
            .or_else(|| spec.strip_prefix("brightness:"))
        {
            let percent: u8 = value.trim().parse().map_err(|_| {
                Error::ConfigInvalid(format!("brightness value '{value}' is not 0-255"))
            })?;
            if percent > 100 {
                return Err(Error::ConfigInvalid(format!(
                    "brightness {percent} out of range (0-100)"
                )));
            }
            return Ok(Builtin::BrightnessSet(percent));
        }
        if let Some(target) = spec.strip_prefix("open:") {
            let target = target.trim();
            if target.is_empty() {
                return Err(Error::ConfigInvalid("open: needs a file or URL".into()));
            }
            return Ok(Builtin::Open(target.to_string()));
        }
        Err(Error::ConfigInvalid(format!("unknown builtin '{spec}'")))
    }
}

/// What a key does when pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    /// Run a shell command via `sh -c`.
    Run(String),
    /// Trigger a device-native action.
    Builtin(Builtin),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_builtins() {
        assert_eq!(
            Builtin::parse("brightness_up").unwrap(),
            Builtin::BrightnessUp
        );
        assert_eq!(
            Builtin::parse("brightness_down").unwrap(),
            Builtin::BrightnessDown
        );
        assert_eq!(Builtin::parse("reset").unwrap(), Builtin::Reset);
        assert_eq!(Builtin::parse("  reset  ").unwrap(), Builtin::Reset);
    }

    #[test]
    fn parses_brightness_set_forms() {
        assert_eq!(
            Builtin::parse("brightness_set:80").unwrap(),
            Builtin::BrightnessSet(80)
        );
        assert_eq!(
            Builtin::parse("brightness:0").unwrap(),
            Builtin::BrightnessSet(0)
        );
    }

    #[test]
    fn rejects_out_of_range_brightness() {
        let err = Builtin::parse("brightness:150").unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("out of range")));
    }

    #[test]
    fn rejects_non_numeric_brightness() {
        let err = Builtin::parse("brightness:loud").unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("not 0-255")));
    }

    #[test]
    fn rejects_unknown_builtin() {
        let err = Builtin::parse("teleport").unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("unknown builtin")));
    }

    #[test]
    fn parses_brightness_extremes() {
        assert_eq!(
            Builtin::parse("brightness_max").unwrap(),
            Builtin::BrightnessMax
        );
        assert_eq!(
            Builtin::parse("brightness_min").unwrap(),
            Builtin::BrightnessMin
        );
    }

    #[test]
    fn parses_media_and_volume() {
        assert_eq!(
            Builtin::parse("media_play_pause").unwrap(),
            Builtin::Media(Media::PlayPause)
        );
        assert_eq!(
            Builtin::parse("media_next").unwrap(),
            Builtin::Media(Media::Next)
        );
        assert_eq!(
            Builtin::parse("media_prev").unwrap(),
            Builtin::Media(Media::Prev)
        );
        assert_eq!(
            Builtin::parse("volume_up").unwrap(),
            Builtin::Volume(Volume::Up)
        );
        assert_eq!(
            Builtin::parse("volume_down").unwrap(),
            Builtin::Volume(Volume::Down)
        );
        assert_eq!(
            Builtin::parse("volume_mute").unwrap(),
            Builtin::Volume(Volume::Mute)
        );
    }

    #[test]
    fn parses_open_with_target() {
        assert_eq!(
            Builtin::parse("open:https://example.test").unwrap(),
            Builtin::Open("https://example.test".into())
        );
        assert!(matches!(
            Builtin::parse("open:").unwrap_err(),
            Error::ConfigInvalid(_)
        ));
    }
}
