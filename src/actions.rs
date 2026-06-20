//! Built-in actions a key can trigger, in addition to shell commands.
//!
//! Built-ins act on the deck itself (brightness, reset) - things a plain shell
//! command cannot reach. Shell commands cover everything else (launch apps,
//! `playerctl`, `wpctl`, `ydotool`, ...).

use crate::error::{Error, Result};

/// Brightness step applied by `brightness_up` / `brightness_down`.
pub const BRIGHTNESS_STEP: u8 = 10;

/// A device-native action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// Increase brightness by [`BRIGHTNESS_STEP`].
    BrightnessUp,
    /// Decrease brightness by [`BRIGHTNESS_STEP`].
    BrightnessDown,
    /// Set brightness to an absolute percentage.
    BrightnessSet(u8),
    /// Reset the device to its standby logo.
    Reset,
}

impl Builtin {
    /// Parse a built-in from its config string.
    ///
    /// Accepted: `brightness_up`, `brightness_down`, `brightness_set:N`
    /// (also `brightness:N`), `reset`.
    pub fn parse(spec: &str) -> Result<Self> {
        let spec = spec.trim();
        match spec {
            "brightness_up" => Ok(Builtin::BrightnessUp),
            "brightness_down" => Ok(Builtin::BrightnessDown),
            "reset" => Ok(Builtin::Reset),
            _ => {
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
                    Ok(Builtin::BrightnessSet(percent))
                } else {
                    Err(Error::ConfigInvalid(format!("unknown builtin '{spec}'")))
                }
            }
        }
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
}
