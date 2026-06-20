//! Apply a [`Config`] to a device and run the press-to-action loop.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::device::StreamDeck;
use crate::error::Result;
use crate::events::{diff_states, KeyEventKind};
use crate::render::KeySurface;

/// Colour shown on a key whose image failed to load and that has no colour
/// fallback, so a broken tile is loud rather than silent.
const ERROR_TILE: [u8; 3] = [255, 0, 255];
/// Default label colour when none is configured.
const DEFAULT_TEXT_COLOR: [u8; 3] = [255, 255, 255];

/// How long each button read blocks before re-checking the shutdown flag.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Build the key -> shell-command map from a config.
pub fn action_map(config: &Config) -> HashMap<u8, String> {
    config
        .buttons
        .iter()
        .filter_map(|button| button.run.as_ref().map(|cmd| (button.key, cmd.clone())))
        .collect()
}

/// Render a config onto the device: brightness, then each key's picture or
/// colour. Unconfigured keys are blanked. Image-load failures are reported and
/// fall back to the key's colour, or an error tile, never a silent blank.
pub fn render(deck: &mut StreamDeck, config: &Config, base_dir: &Path) -> Result<()> {
    if let Some(brightness) = config.brightness {
        deck.set_brightness(brightness)?;
    }
    deck.clear_all()?;

    let spec = deck.model().image;
    for button in &config.buttons {
        let mut surface = KeySurface::new(&spec);

        // Background: picture if present and loadable, else colour, else an
        // error tile for a broken image with no colour fallback.
        let drew_image = match button.resolved_image(base_dir) {
            Some(path) => match image::open(&path) {
                Ok(picture) => {
                    surface.draw_image(&picture);
                    true
                }
                Err(err) => {
                    eprintln!(
                        "warning: key {} image '{}' failed to load: {err}",
                        button.key,
                        path.display()
                    );
                    false
                }
            },
            None => false,
        };
        if !drew_image {
            match button.rgb()? {
                Some(rgb) => surface.fill(rgb),
                None if button.label.is_some() => {} // keep black background
                None => surface.fill(ERROR_TILE),
            }
        }

        // Foreground: optional centred text label.
        if let Some(label) = &button.label {
            let color = button.text_rgb()?.unwrap_or(DEFAULT_TEXT_COLOR);
            surface.draw_text_centered(label, color);
        }

        deck.set_key_image(button.key, &surface.encode()?)?;
    }
    Ok(())
}

/// Spawn a shell command detached from the daemon.
fn spawn(command: &str) -> std::io::Result<Child> {
    Command::new("sh").arg("-c").arg(command).spawn()
}

/// Render the config and run the press-to-action loop until `shutdown` is set.
pub fn run(
    deck: &mut StreamDeck,
    config: &Config,
    base_dir: &Path,
    shutdown: &AtomicBool,
) -> Result<()> {
    // Auto-reap launched processes so the daemon never accumulates zombies.
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_IGN);
    }

    render(deck, config, base_dir)?;
    let actions = action_map(config);
    let key_count = deck.model().key_count as usize;
    let mut previous = vec![false; key_count];

    println!(
        "Running with {} mapped action(s). Press Ctrl-C to stop.",
        actions.len()
    );

    while !shutdown.load(Ordering::Relaxed) {
        let Some(states) = deck.read_button_states(Some(POLL_INTERVAL))? else {
            continue;
        };
        for event in diff_states(&previous, &states) {
            if event.kind != KeyEventKind::Pressed {
                continue;
            }
            if let Some(command) = actions.get(&event.key) {
                println!("key {} pressed -> {command}", event.key);
                if let Err(err) = spawn(command) {
                    eprintln!("error: failed to run '{command}': {err}");
                }
            }
        }
        previous = states;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn action_map_collects_only_keys_with_commands() {
        let config = Config::from_toml_str(
            "[[buttons]]\nkey = 0\ncolor = \"#ffffff\"\nrun = \"echo hi\"\n\n[[buttons]]\nkey = 3\ncolor = \"#000000\"\n",
        )
        .unwrap();

        let actions = action_map(&config);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions.get(&0).map(String::as_str), Some("echo hi"));
        assert!(!actions.contains_key(&3));
    }

    #[test]
    fn action_map_is_empty_without_commands() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 0\ncolor = \"#ffffff\"\n").unwrap();
        assert!(action_map(&config).is_empty());
    }
}
