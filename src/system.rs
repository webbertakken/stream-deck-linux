//! Resolve shell commands for the media / volume / open built-ins, picking
//! whichever tool is available on the host.
//!
//! The command *builders* are pure (given a [`Tools`] snapshot) so they can be
//! unit tested without touching the system; [`detect_tools`] does the impure
//! `PATH` probing.

/// Which helper tools are present on `PATH`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tools {
    pub playerctl: bool,
    pub wpctl: bool,
    pub pactl: bool,
    pub amixer: bool,
}

/// Media transport actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    PlayPause,
    Next,
    Prev,
}

/// Volume actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volume {
    Up,
    Down,
    Mute,
}

/// Percentage step for volume up/down.
const VOLUME_STEP: u32 = 5;

/// Probe `PATH` for the helper tools.
pub fn detect_tools() -> Tools {
    Tools {
        playerctl: on_path("playerctl"),
        wpctl: on_path("wpctl"),
        pactl: on_path("pactl"),
        amixer: on_path("amixer"),
    }
}

/// Whether an executable named `bin` exists on `PATH`.
fn on_path(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
}

/// Single-quote a string for safe use in a `sh -c` command.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Command to open a file or URL with the desktop default handler.
pub fn open_command(target: &str) -> String {
    format!("xdg-open {}", shell_quote(target))
}

/// Command for a media transport action, if a player controller is available.
pub fn media_command(action: Media, tools: &Tools) -> Option<String> {
    if !tools.playerctl {
        return None;
    }
    let verb = match action {
        Media::PlayPause => "play-pause",
        Media::Next => "next",
        Media::Prev => "previous",
    };
    Some(format!("playerctl {verb}"))
}

/// Command for a volume action, preferring wpctl, then pactl, then amixer.
pub fn volume_command(action: Volume, tools: &Tools) -> Option<String> {
    if tools.wpctl {
        return Some(match action {
            // `-l 1.0` clamps so a key can't push past 100%.
            Volume::Up => format!("wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ {VOLUME_STEP}%+"),
            Volume::Down => format!("wpctl set-volume @DEFAULT_AUDIO_SINK@ {VOLUME_STEP}%-"),
            Volume::Mute => "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle".to_string(),
        });
    }
    if tools.pactl {
        return Some(match action {
            Volume::Up => format!("pactl set-sink-volume @DEFAULT_SINK@ +{VOLUME_STEP}%"),
            Volume::Down => format!("pactl set-sink-volume @DEFAULT_SINK@ -{VOLUME_STEP}%"),
            Volume::Mute => "pactl set-sink-mute @DEFAULT_SINK@ toggle".to_string(),
        });
    }
    if tools.amixer {
        return Some(match action {
            Volume::Up => format!("amixer -q set Master {VOLUME_STEP}%+"),
            Volume::Down => format!("amixer -q set Master {VOLUME_STEP}%-"),
            Volume::Mute => "amixer -q set Master toggle".to_string(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_command_shell_quotes_target() {
        assert_eq!(
            open_command("https://x.test/a b"),
            "xdg-open 'https://x.test/a b'"
        );
        assert_eq!(open_command("a'b"), "xdg-open 'a'\\''b'");
    }

    #[test]
    fn media_needs_playerctl() {
        let none = Tools::default();
        assert_eq!(media_command(Media::PlayPause, &none), None);
        let with = Tools {
            playerctl: true,
            ..Default::default()
        };
        assert_eq!(
            media_command(Media::PlayPause, &with).as_deref(),
            Some("playerctl play-pause")
        );
        assert_eq!(
            media_command(Media::Next, &with).as_deref(),
            Some("playerctl next")
        );
        assert_eq!(
            media_command(Media::Prev, &with).as_deref(),
            Some("playerctl previous")
        );
    }

    #[test]
    fn volume_prefers_wpctl_then_pactl_then_amixer() {
        let wp = Tools {
            wpctl: true,
            pactl: true,
            amixer: true,
            ..Default::default()
        };
        assert!(volume_command(Volume::Up, &wp)
            .unwrap()
            .starts_with("wpctl "));

        let pa = Tools {
            pactl: true,
            amixer: true,
            ..Default::default()
        };
        assert!(volume_command(Volume::Up, &pa)
            .unwrap()
            .starts_with("pactl "));

        let am = Tools {
            amixer: true,
            ..Default::default()
        };
        assert!(volume_command(Volume::Mute, &am)
            .unwrap()
            .starts_with("amixer "));

        assert_eq!(volume_command(Volume::Up, &Tools::default()), None);
    }
}
