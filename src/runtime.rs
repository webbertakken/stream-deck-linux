//! Apply a [`Config`] to a device and run the press-to-action loop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::actions::{Builtin, KeyAction, BRIGHTNESS_STEP};
use crate::config::Config;
use crate::device::StreamDeck;
use crate::error::Result;
use crate::events::{diff_states, KeyEventKind};
use crate::render;

/// Brightness assumed when the config does not specify one (for up/down steps).
const DEFAULT_BRIGHTNESS: u8 = 50;

/// How long each button read blocks before re-checking the shutdown flag.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Build the key -> action map from a config. Errors on an unparseable builtin
/// (validation should have caught it first).
pub fn action_map(config: &Config) -> Result<HashMap<u8, KeyAction>> {
    let mut map = HashMap::new();
    for button in &config.buttons {
        if let Some(run) = &button.run {
            map.insert(button.key, KeyAction::Run(run.clone()));
        } else if let Some(spec) = &button.builtin {
            map.insert(button.key, KeyAction::Builtin(Builtin::parse(spec)?));
        }
    }
    Ok(map)
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
        let surface = render::compose(&spec, base_dir, button);
        deck.set_key_image(button.key, &surface.encode()?)?;
    }
    Ok(())
}

/// Spawn a shell command detached from the daemon.
fn spawn(command: &str) -> std::io::Result<Child> {
    Command::new("sh").arg("-c").arg(command).spawn()
}

/// Apply a device-native built-in, updating tracked brightness.
fn apply_builtin(deck: &StreamDeck, builtin: Builtin, brightness: &mut u8) -> Result<()> {
    let new_brightness = match builtin {
        Builtin::BrightnessUp => Some((*brightness).saturating_add(BRIGHTNESS_STEP).min(100)),
        Builtin::BrightnessDown => Some(brightness.saturating_sub(BRIGHTNESS_STEP)),
        Builtin::BrightnessSet(value) => Some(value.min(100)),
        Builtin::Reset => None,
    };
    match new_brightness {
        Some(value) => {
            *brightness = value;
            deck.set_brightness(value)?;
            println!("brightness -> {value}%");
        }
        None => {
            deck.reset()?;
            println!("device reset");
        }
    }
    Ok(())
}

/// A command sent to the running daemon (e.g. from a tray menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    BrightnessUp,
    BrightnessDown,
    SetBrightness(u8),
    Reset,
    /// Re-read the config file and re-render.
    Reload,
    /// Stop the daemon.
    Quit,
}

/// Live daemon state: the device plus the rendered config and action map.
struct Session {
    deck: StreamDeck,
    config_path: PathBuf,
    base_dir: PathBuf,
    actions: HashMap<u8, KeyAction>,
    brightness: u8,
    previous: Vec<bool>,
}

impl Session {
    /// Load and render the config onto the device.
    fn load(mut deck: StreamDeck, config_path: PathBuf) -> Result<Self> {
        let base_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let config = Config::load(&config_path)?;
        config.validate(deck.model())?;
        render(&mut deck, &config, &base_dir)?;
        let actions = action_map(&config)?;
        let brightness = config.brightness.unwrap_or(DEFAULT_BRIGHTNESS);
        let previous = vec![false; deck.model().key_count as usize];
        Ok(Self {
            deck,
            config_path,
            base_dir,
            actions,
            brightness,
            previous,
        })
    }

    /// Re-read the config from disk and re-render.
    fn reload(&mut self) -> Result<()> {
        let config = Config::load(&self.config_path)?;
        config.validate(self.deck.model())?;
        render(&mut self.deck, &config, &self.base_dir)?;
        self.actions = action_map(&config)?;
        if let Some(brightness) = config.brightness {
            self.brightness = brightness;
        }
        println!("config reloaded ({} action(s))", self.actions.len());
        Ok(())
    }

    /// Apply a control command. `Quit` is handled by the caller.
    fn handle_control(&mut self, control: Control) -> Result<()> {
        match control {
            Control::BrightnessUp => {
                apply_builtin(&self.deck, Builtin::BrightnessUp, &mut self.brightness)
            }
            Control::BrightnessDown => {
                apply_builtin(&self.deck, Builtin::BrightnessDown, &mut self.brightness)
            }
            Control::SetBrightness(value) => apply_builtin(
                &self.deck,
                Builtin::BrightnessSet(value),
                &mut self.brightness,
            ),
            Control::Reset => apply_builtin(&self.deck, Builtin::Reset, &mut self.brightness),
            Control::Reload => self.reload(),
            Control::Quit => Ok(()),
        }
    }

    /// Poll the device once and dispatch any key presses.
    fn poll(&mut self) -> Result<()> {
        let Some(states) = self.deck.read_button_states(Some(POLL_INTERVAL))? else {
            return Ok(());
        };
        let events = diff_states(&self.previous, &states);
        self.previous = states;
        for event in events {
            if event.kind != KeyEventKind::Pressed {
                continue;
            }
            // Clone the action out so the borrow of `self.actions` is released
            // before we mutate other fields.
            match self.actions.get(&event.key).cloned() {
                Some(KeyAction::Run(command)) => {
                    println!("key {} pressed -> {command}", event.key);
                    if let Err(err) = spawn(&command) {
                        eprintln!("error: failed to run '{command}': {err}");
                    }
                }
                Some(KeyAction::Builtin(builtin)) => {
                    println!("key {} pressed -> builtin {builtin:?}", event.key);
                    if let Err(err) = apply_builtin(&self.deck, builtin, &mut self.brightness) {
                        eprintln!("error: builtin {builtin:?} failed: {err}");
                    }
                }
                None => {}
            }
        }
        Ok(())
    }
}

/// Render the config and run the press-to-action loop, also servicing control
/// messages, until `shutdown` is set or a `Quit` arrives.
pub fn run_with_control(
    deck: StreamDeck,
    config_path: PathBuf,
    shutdown: &AtomicBool,
    control: &Receiver<Control>,
) -> Result<()> {
    // Auto-reap launched processes so the daemon never accumulates zombies.
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_IGN);
    }

    let mut session = Session::load(deck, config_path)?;
    println!("Running with {} mapped action(s).", session.actions.len());

    while !shutdown.load(Ordering::Relaxed) {
        while let Ok(control) = control.try_recv() {
            if control == Control::Quit {
                shutdown.store(true, Ordering::Relaxed);
                break;
            }
            if let Err(err) = session.handle_control(control) {
                eprintln!("error: control {control:?} failed: {err}");
            }
        }
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        session.poll()?;
    }

    let _ = session.deck.clear_all();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn action_map_collects_runs_and_builtins() {
        let config = Config::from_toml_str(
            "[[buttons]]\nkey = 0\ncolor = \"#ffffff\"\nrun = \"echo hi\"\n\n[[buttons]]\nkey = 1\ncolor = \"#111111\"\nbuiltin = \"brightness_up\"\n\n[[buttons]]\nkey = 3\ncolor = \"#000000\"\n",
        )
        .unwrap();

        let actions = action_map(&config).unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions.get(&0),
            Some(&KeyAction::Run("echo hi".to_string()))
        );
        assert_eq!(
            actions.get(&1),
            Some(&KeyAction::Builtin(Builtin::BrightnessUp))
        );
        assert!(!actions.contains_key(&3));
    }

    #[test]
    fn action_map_is_empty_without_actions() {
        let config = Config::from_toml_str("[[buttons]]\nkey = 0\ncolor = \"#ffffff\"\n").unwrap();
        assert!(action_map(&config).unwrap().is_empty());
    }
}
