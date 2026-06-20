//! `streamdeck` - command-line control for Elgato Stream Deck on Linux.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use streamdeck::events::diff_states;
use streamdeck::runtime::Control;
use streamdeck::tray::StreamDeckTray;
use streamdeck::webui::WebUi;
use streamdeck::{autostart, install, runtime, Error, Model, StreamDeck};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");

    let result = match command {
        "list" => cmd_list(),
        "info" => cmd_info(),
        "brightness" => cmd_brightness(&args),
        "color" | "colour" => cmd_color(&args),
        "image" => cmd_image(&args),
        "clear" => cmd_clear(&args),
        "reset" => cmd_reset(),
        "run" => cmd_run(&args),
        "tray" => cmd_tray(&args),
        "ui" => cmd_ui(&args),
        "autostart" => cmd_autostart(&args),
        "install" => cmd_install(),
        "uninstall" => cmd_uninstall(),
        "watch" => cmd_watch(&args),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}\n");
            print_help();
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            if matches!(err, Error::DeviceNotFound) {
                eprintln!(
                    "hint: is the Stream Deck plugged in and accessible? (try `streamdeck list`)"
                );
            }
            ExitCode::FAILURE
        }
    }
}

fn cmd_list() -> Result<(), Error> {
    let decks = StreamDeck::list()?;
    if decks.is_empty() {
        println!("No supported Stream Deck found.");
        return Ok(());
    }
    println!("Found {} Stream Deck(s):", decks.len());
    for (path, model) in decks {
        println!(
            "  {}  {} ({} keys, {}x{})",
            path.display(),
            model.name,
            model.key_count,
            model.columns,
            model.rows
        );
    }
    Ok(())
}

fn cmd_info() -> Result<(), Error> {
    let deck = StreamDeck::open_first()?;
    let model = deck.model();
    println!("Model:    {}", model.name);
    println!("Path:     {}", deck.path().display());
    println!(
        "Keys:     {} ({}x{})",
        model.key_count, model.columns, model.rows
    );
    println!(
        "Image:    {}x{} {:?}",
        model.image.width, model.image.height, model.image.format
    );
    println!("Firmware: {}", deck.firmware_version()?);
    println!("Serial:   {}", deck.serial_number()?);
    Ok(())
}

fn cmd_brightness(args: &[String]) -> Result<(), Error> {
    let percent = args
        .get(1)
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or_else(|| usage_exit("brightness <0-100>"));
    let deck = StreamDeck::open_first()?;
    deck.set_brightness(percent)?;
    println!("Brightness set to {percent}%.");
    Ok(())
}

fn cmd_color(args: &[String]) -> Result<(), Error> {
    let key = parse_key(args.get(1));
    let rgb = args
        .get(2)
        .and_then(|s| parse_hex_color(s))
        .unwrap_or_else(|| usage_exit("color <key> <RRGGBB>"));
    let mut deck = StreamDeck::open_first()?;
    deck.set_key_color(key, rgb)?;
    println!(
        "Key {key} set to #{:02X}{:02X}{:02X}.",
        rgb[0], rgb[1], rgb[2]
    );
    Ok(())
}

fn cmd_image(args: &[String]) -> Result<(), Error> {
    let key = parse_key(args.get(1));
    let path = args
        .get(2)
        .unwrap_or_else(|| usage_exit("image <key> <path-to-picture>"));
    let mut deck = StreamDeck::open_first()?;
    let picture = image::open(path)?;
    deck.set_key_picture(key, &picture)?;
    println!("Key {key} set from {path}.");
    Ok(())
}

fn cmd_clear(args: &[String]) -> Result<(), Error> {
    let mut deck = StreamDeck::open_first()?;
    match args.get(1) {
        Some(arg) => {
            let key = arg
                .parse::<u8>()
                .unwrap_or_else(|_| usage_exit("clear [key]"));
            deck.clear_key(key)?;
            println!("Key {key} cleared.");
        }
        None => {
            deck.clear_all()?;
            println!("All keys cleared.");
        }
    }
    Ok(())
}

fn cmd_reset() -> Result<(), Error> {
    let deck = StreamDeck::open_first()?;
    deck.reset()?;
    println!("Device reset to standby.");
    Ok(())
}

fn cmd_run(args: &[String]) -> Result<(), Error> {
    let config_path = config_path(args);
    let deck = StreamDeck::open_first()?;
    println!(
        "Loaded {} from {}.",
        deck.model().name,
        config_path.display()
    );
    install_signal_handlers();

    let (_tx, rx) = mpsc::channel();
    let result = runtime::run_with_control(deck, config_path, &SHUTDOWN, &rx);
    println!("\nStopped.");
    result
}

/// Open the device and run the press-to-action daemon in a background thread.
fn start_daemon(config_path: PathBuf) -> Result<(Model, Sender<Control>, JoinHandle<()>), Error> {
    let deck = StreamDeck::open_first()?;
    let model = *deck.model();
    let (tx, rx) = mpsc::channel::<Control>();
    let daemon_path = config_path;
    let handle = std::thread::spawn(move || {
        if let Err(err) = runtime::run_with_control(deck, daemon_path, &SHUTDOWN, &rx) {
            eprintln!("error: daemon stopped: {err}");
        }
        SHUTDOWN.store(true, Ordering::Relaxed);
    });
    Ok((model, tx, handle))
}

/// Start the in-process web editor server on an ephemeral local port.
fn start_web_ui(
    model: Model,
    config_path: PathBuf,
    control: Sender<Control>,
) -> Result<(String, JoinHandle<()>), Error> {
    let web = WebUi::bind("127.0.0.1:0", model, config_path, control)?;
    let url = web.url();
    let handle = std::thread::spawn(move || {
        if let Err(err) = web.serve(&SHUTDOWN) {
            eprintln!("error: web ui stopped: {err}");
        }
    });
    Ok((url, handle))
}

fn cmd_tray(args: &[String]) -> Result<(), Error> {
    let config_path = config_path(args);
    ensure_config_exists(&config_path)?;
    install_signal_handlers();

    let (model, tx, daemon) = start_daemon(config_path.clone())?;
    let (url, web) = start_web_ui(model, config_path, tx.clone())?;
    println!("Tray starting for {}. Editor at {url}", model.name);

    let tray = StreamDeckTray::new(tx, &SHUTDOWN, url, format!("{} connected", model.name));
    let _handle = tray.show()?;
    println!("Tray running. Use the tray menu (or Ctrl-C) to quit.");

    while !SHUTDOWN.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = daemon.join();
    let _ = web.join();
    println!("\nStopped.");
    Ok(())
}

fn cmd_ui(args: &[String]) -> Result<(), Error> {
    let config_path = config_path(args);
    ensure_config_exists(&config_path)?;
    install_signal_handlers();

    let (model, tx, daemon) = start_daemon(config_path.clone())?;
    let web = WebUi::bind("127.0.0.1:0", model, config_path, tx)?;
    let url = web.url();
    println!("Editor for {} at {url}", model.name);
    open_in_browser(&url);

    let result = web.serve(&SHUTDOWN);
    SHUTDOWN.store(true, Ordering::Relaxed);
    let _ = daemon.join();
    println!("\nStopped.");
    result
}

/// Open a URL in the default browser unless suppressed (e.g. in tests).
fn open_in_browser(url: &str) {
    if std::env::var_os("STREAMDECK_NO_BROWSER").is_some() {
        return;
    }
    if let Err(err) = std::process::Command::new("xdg-open").arg(url).spawn() {
        eprintln!("note: could not open browser ({err}); visit {url}");
    }
}

fn cmd_autostart(args: &[String]) -> Result<(), Error> {
    let action = args.get(1).map(String::as_str).unwrap_or("status");
    match action {
        "enable" => {
            let exe = std::env::current_exe()?;
            let exec = format!("{} tray", exe.display());
            let path = autostart::enable(&exec, "streamdeck")?;
            println!("Autostart enabled: {}", path.display());
            println!("Exec: {exec}");
        }
        "disable" => {
            if autostart::disable()? {
                println!("Autostart disabled.");
            } else {
                println!("Autostart was not enabled.");
            }
        }
        "status" => {
            let state = if autostart::is_enabled() {
                "enabled"
            } else {
                "disabled"
            };
            println!("Autostart: {state}");
            println!("Entry: {}", autostart::entry_path().display());
        }
        other => {
            eprintln!("unknown autostart action: {other}");
            usage_exit("autostart enable|disable|status");
        }
    }
    Ok(())
}

fn cmd_install() -> Result<(), Error> {
    let exe = std::env::current_exe()?;
    let exec = format!("{} tray", exe.display());
    let written = install::install(&exec)?;
    println!("Installed {} files:", written.len());
    for path in &written {
        println!("  {}", path.display());
    }
    // Best-effort icon cache refresh (harmless if the tool is absent).
    if let Some(hicolor) = written.first().and_then(|p| p.ancestors().nth(3)) {
        let _ = std::process::Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(hicolor)
            .status();
    }
    println!("Done. The 'streamdeck' icon and launcher are now available.");
    Ok(())
}

fn cmd_uninstall() -> Result<(), Error> {
    install::uninstall()?;
    println!("Removed installed icons and launcher entry.");
    Ok(())
}

/// Write a friendly starter config if none exists yet.
fn ensure_config_exists(path: &PathBuf) -> Result<(), Error> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, STARTER_CONFIG)?;
    println!("Wrote a starter config to {}.", path.display());
    Ok(())
}

const STARTER_CONFIG: &str = "brightness = 60\n\n\
[[buttons]]\nkey = 0\ncolor = \"#1e1e2e\"\nlabel = \"Term\"\nrun = \"x-terminal-emulator\"\n\n\
[[buttons]]\nkey = 5\ncolor = \"#444466\"\nlabel = \"Bright+\"\nbuiltin = \"brightness_up\"\n\n\
[[buttons]]\nkey = 10\ncolor = \"#222244\"\nlabel = \"Bright-\"\nbuiltin = \"brightness_down\"\n";

/// Resolve the config path from `run [path]`, defaulting to the XDG location.
fn config_path(args: &[String]) -> PathBuf {
    if let Some(path) = args.get(1) {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("streamdeck").join("config.toml")
}

extern "C" fn handle_signal(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() {
    let handler = handle_signal as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

fn cmd_watch(args: &[String]) -> Result<(), Error> {
    let seconds = args.get(1).and_then(|s| s.parse::<u64>().ok());
    let mut deck = StreamDeck::open_first()?;
    let key_count = deck.model().key_count as usize;
    println!(
        "Watching {} keys{}. Press buttons on the deck...",
        key_count,
        seconds.map(|s| format!(" for {s}s")).unwrap_or_default()
    );

    let start = Instant::now();
    let mut previous = vec![false; key_count];
    loop {
        if let Some(limit) = seconds {
            if start.elapsed() >= Duration::from_secs(limit) {
                break;
            }
        }
        if let Some(states) = deck.read_button_states(Some(Duration::from_millis(200)))? {
            for event in diff_states(&previous, &states) {
                println!("  key {:>2}  {:?}", event.key, event.kind);
            }
            previous = states;
        }
    }
    Ok(())
}

fn parse_key(arg: Option<&String>) -> u8 {
    arg.and_then(|s| s.parse::<u8>().ok())
        .unwrap_or_else(|| usage_exit("expected a key index"))
}

fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(s, 16).ok()?;
    Some([
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ])
}

fn usage_exit(usage: &str) -> ! {
    eprintln!("usage: streamdeck {usage}");
    std::process::exit(2);
}

fn print_help() {
    println!(
        "streamdeck - control an Elgato Stream Deck on Linux\n\n\
         USAGE:\n\
         \x20 streamdeck <command> [args]\n\n\
         COMMANDS:\n\
         \x20 list                     list connected Stream Decks\n\
         \x20 info                     show model, firmware and serial\n\
         \x20 brightness <0-100>       set display brightness\n\
         \x20 color <key> <RRGGBB>     fill a key with a solid colour\n\
         \x20 image <key> <path>       render a picture onto a key\n\
         \x20 clear [key]              blank one key, or all keys\n\
         \x20 reset                    return device to standby logo\n\
         \x20 run [config.toml]        render config and dispatch key actions\n\
         \x20 tray [config.toml]       run in the system tray with a daemon\n\
         \x20 ui [config.toml]         open the web editor + run the daemon\n\
         \x20 autostart <enable|disable|status>  manage login autostart\n\
         \x20 install                  install icons + app launcher entry\n\
         \x20 uninstall                remove installed icons + launcher\n\
         \x20 watch [seconds]          print button press/release events"
    );
}
