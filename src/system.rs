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
    /// Emulates the desktop's `XF86Audio*` keys (gives the native OSD).
    pub xdotool: bool,
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
        xdotool: on_path("xdotool"),
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

/// Command for a media transport action. Prefers `playerctl`, then the desktop
/// media keys (`xdotool`, with OSD), and always falls back to driving the first
/// MPRIS player over D-Bus (`busctl`, present on any systemd desktop).
pub fn media_command(action: Media, tools: &Tools) -> String {
    if tools.playerctl {
        let verb = match action {
            Media::PlayPause => "play-pause",
            Media::Next => "next",
            Media::Prev => "previous",
        };
        return format!("playerctl {verb}");
    }
    if tools.xdotool {
        let key = match action {
            Media::PlayPause => "XF86AudioPlay",
            Media::Next => "XF86AudioNext",
            Media::Prev => "XF86AudioPrev",
        };
        return format!("xdotool key {key}");
    }
    let method = match action {
        Media::PlayPause => "PlayPause",
        Media::Next => "Next",
        Media::Prev => "Previous",
    };
    format!(
        "p=$(busctl --user list --no-legend 2>/dev/null | \
         awk '/org\\.mpris\\.MediaPlayer2\\./{{print $1; exit}}'); \
         [ -n \"$p\" ] && busctl --user call \"$p\" /org/mpris/MediaPlayer2 \
         org.mpris.MediaPlayer2.Player {method}"
    )
}

/// Command for a volume action. Prefers `xdotool` (the desktop media keys, so
/// the volume OSD shows), then wpctl, pactl, amixer.
pub fn volume_command(action: Volume, tools: &Tools) -> Option<String> {
    if tools.xdotool {
        return Some(match action {
            Volume::Up => "xdotool key XF86AudioRaiseVolume".to_string(),
            Volume::Down => "xdotool key XF86AudioLowerVolume".to_string(),
            Volume::Mute => "xdotool key XF86AudioMute".to_string(),
        });
    }
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
    fn media_prefers_playerctl_then_xdotool_then_mpris() {
        let pc = Tools {
            playerctl: true,
            ..Default::default()
        };
        assert_eq!(media_command(Media::PlayPause, &pc), "playerctl play-pause");

        let xd = Tools {
            xdotool: true,
            ..Default::default()
        };
        assert_eq!(media_command(Media::Next, &xd), "xdotool key XF86AudioNext");

        let mpris = media_command(Media::PlayPause, &Tools::default());
        assert!(mpris.contains("busctl") && mpris.contains("PlayPause"));
    }

    #[test]
    fn volume_prefers_xdotool_then_wpctl_then_pactl_then_amixer() {
        let xd = Tools {
            xdotool: true,
            wpctl: true,
            ..Default::default()
        };
        assert!(volume_command(Volume::Up, &xd)
            .unwrap()
            .starts_with("xdotool "));

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
