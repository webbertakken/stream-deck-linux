//! Apply a [`Config`] to a device and run the press-to-action loop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::actions::{Builtin, KeyAction, BRIGHTNESS_STEP};
use crate::config::{ButtonConfig, Config, Page};
use crate::device::StreamDeck;
use crate::error::Result;
use crate::events::{diff_states, KeyEventKind};
use crate::render;

/// Brightness assumed when the config does not specify one (for up/down steps).
const DEFAULT_BRIGHTNESS: u8 = 50;

/// How long each button read blocks before re-checking the shutdown flag.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Build the key -> action map for a page's buttons. Errors on an unparseable
/// builtin (validation should have caught it first).
pub fn action_map(buttons: &[ButtonConfig]) -> Result<HashMap<u8, KeyAction>> {
    let mut map = HashMap::new();
    for button in buttons {
        if let Some(run) = &button.run {
            map.insert(button.key, KeyAction::Run(run.clone()));
        } else if let Some(spec) = &button.builtin {
            map.insert(button.key, KeyAction::Builtin(Builtin::parse(spec)?));
        } else if let Some(steps) = &button.macro_steps {
            map.insert(button.key, KeyAction::Macro(steps.clone()));
        }
    }
    Ok(map)
}

/// Render a page's buttons onto the device. Unconfigured keys are blanked.
/// Image-load failures fall back to the key's colour or an error tile.
fn render_page(deck: &mut StreamDeck, buttons: &[ButtonConfig], base_dir: &Path) -> Result<()> {
    deck.clear_all()?;
    let spec = deck.model().image;
    for button in buttons {
        let surface = render::compose(&spec, base_dir, button);
        deck.set_key_image(button.key, &surface.encode()?)?;
    }
    Ok(())
}

/// Where a page switch should land.
enum PageTarget {
    Next,
    Prev,
    To(usize),
}

/// Spawn a shell command detached from the daemon.
fn spawn(command: &str) -> std::io::Result<Child> {
    Command::new("sh").arg("-c").arg(command).spawn()
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

/// Live daemon state: the device plus the rendered pages and current actions.
struct Session {
    deck: StreamDeck,
    config_path: PathBuf,
    base_dir: PathBuf,
    pages: Vec<Page>,
    current_page: usize,
    actions: HashMap<u8, KeyAction>,
    brightness: u8,
    previous: Vec<bool>,
    tools: crate::system::Tools,
}

impl Session {
    /// Load the config, set brightness and render the first page.
    fn load(deck: StreamDeck, config_path: PathBuf) -> Result<Self> {
        let base_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let config = Config::load(&config_path)?;
        config.validate(deck.model())?;
        let brightness = config.brightness.unwrap_or(DEFAULT_BRIGHTNESS);
        if let Some(b) = config.brightness {
            deck.set_brightness(b)?;
        }
        let previous = vec![false; deck.model().key_count as usize];
        let mut session = Self {
            deck,
            config_path,
            base_dir,
            pages: config.pages(),
            current_page: 0,
            actions: HashMap::new(),
            brightness,
            previous,
            tools: crate::system::detect_tools(),
        };
        session.show_page(0)?;
        Ok(session)
    }

    /// Render a page and rebuild its action map.
    fn show_page(&mut self, index: usize) -> Result<()> {
        if self.pages.is_empty() {
            return Ok(());
        }
        self.current_page = index.min(self.pages.len() - 1);
        let buttons = self.pages[self.current_page].buttons.clone();
        render_page(&mut self.deck, &buttons, &self.base_dir)?;
        self.actions = action_map(&buttons)?;
        Ok(())
    }

    /// Switch to another page and re-render.
    fn switch_page(&mut self, target: PageTarget) -> Result<()> {
        let n = self.pages.len();
        if n == 0 {
            return Ok(());
        }
        let index = match target {
            PageTarget::Next => (self.current_page + 1) % n,
            PageTarget::Prev => (self.current_page + n - 1) % n,
            PageTarget::To(i) => i.min(n - 1),
        };
        self.show_page(index)?;
        let name = self.pages[self.current_page]
            .name
            .clone()
            .unwrap_or_else(|| self.current_page.to_string());
        println!("page -> {name}");
        Ok(())
    }

    /// Run a built-in action: deck-native ones act on the device directly;
    /// open/media/volume resolve a system command and spawn it.
    fn run_builtin(&mut self, builtin: &Builtin) -> Result<()> {
        use crate::system;
        let new_brightness = match builtin {
            Builtin::BrightnessUp => Some(self.brightness.saturating_add(BRIGHTNESS_STEP).min(100)),
            Builtin::BrightnessDown => Some(self.brightness.saturating_sub(BRIGHTNESS_STEP)),
            Builtin::BrightnessSet(value) => Some((*value).min(100)),
            Builtin::BrightnessMax => Some(100),
            Builtin::BrightnessMin => Some(0),
            _ => None,
        };
        if let Some(value) = new_brightness {
            self.brightness = value;
            self.deck.set_brightness(value)?;
            println!("brightness -> {value}%");
            return Ok(());
        }
        match builtin {
            Builtin::Reset => {
                self.deck.reset()?;
                println!("device reset");
            }
            Builtin::Open(target) => self.spawn_command(&system::open_command(target)),
            Builtin::Media(action) => match system::media_command(*action, &self.tools) {
                Some(cmd) => self.spawn_command(&cmd),
                None => eprintln!("error: media control needs `playerctl` (not installed)"),
            },
            Builtin::Volume(action) => match system::volume_command(*action, &self.tools) {
                Some(cmd) => self.spawn_command(&cmd),
                None => eprintln!("error: volume control needs wpctl/pactl/amixer (none found)"),
            },
            Builtin::PageNext => self.switch_page(PageTarget::Next)?,
            Builtin::PagePrev => self.switch_page(PageTarget::Prev)?,
            Builtin::Page(target) => {
                let names: Vec<Option<String>> =
                    self.pages.iter().map(|p| p.name.clone()).collect();
                match crate::config::resolve_page_target(target, self.pages.len(), &names) {
                    Some(index) => self.switch_page(PageTarget::To(index))?,
                    None => eprintln!("error: unknown page '{target}'"),
                }
            }
            // Brightness variants handled above.
            _ => {}
        }
        Ok(())
    }

    /// Spawn a resolved system command, logging failures.
    fn spawn_command(&self, command: &str) {
        println!("builtin -> {command}");
        if let Err(err) = spawn(command) {
            eprintln!("error: failed to run '{command}': {err}");
        }
    }

    /// Re-read the config from disk and re-render the current page.
    fn reload(&mut self) -> Result<()> {
        let config = Config::load(&self.config_path)?;
        config.validate(self.deck.model())?;
        if let Some(brightness) = config.brightness {
            self.brightness = brightness;
            self.deck.set_brightness(brightness)?;
        }
        self.pages = config.pages();
        let page = self.current_page.min(self.pages.len().saturating_sub(1));
        self.show_page(page)?;
        println!("config reloaded ({} page(s))", self.pages.len());
        Ok(())
    }

    /// Apply a control command. `Quit` is handled by the caller.
    fn handle_control(&mut self, control: Control) -> Result<()> {
        match control {
            Control::BrightnessUp => self.run_builtin(&Builtin::BrightnessUp),
            Control::BrightnessDown => self.run_builtin(&Builtin::BrightnessDown),
            Control::SetBrightness(value) => self.run_builtin(&Builtin::BrightnessSet(value)),
            Control::Reset => self.run_builtin(&Builtin::Reset),
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
                    if let Err(err) = self.run_builtin(&builtin) {
                        eprintln!("error: builtin {builtin:?} failed: {err}");
                    }
                }
                Some(KeyAction::Macro(steps)) => {
                    println!("key {} pressed -> macro ({} steps)", event.key, steps.len());
                    // Run the steps in order inside one detached shell.
                    let script = steps.join("\n");
                    if let Err(err) = spawn(&script) {
                        eprintln!("error: macro failed: {err}");
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

        let actions = action_map(&config.buttons).unwrap();
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
        assert!(action_map(&config.buttons).unwrap().is_empty());
    }
}
