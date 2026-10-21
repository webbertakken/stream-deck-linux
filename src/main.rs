//! `streamdeck` - command-line control for Elgato Stream Deck on Linux.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use streamdeck::events::diff_states;
use streamdeck::remote::{self, Problem};
use streamdeck::runtime::Control;
use streamdeck::tokens::{self, TokenStore};
use streamdeck::tray::StreamDeckTray;
use streamdeck::view::LiveView;
use streamdeck::webui::WebUi;
use streamdeck::{autostart, install, runtime, Error, Model, StreamDeck};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");

    if remote::VERBS.contains(&command) {
        return cmd_remote(&args);
    }

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
        "token" => cmd_token(&args),
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
    let result = runtime::run_with_control(deck, config_path, &SHUTDOWN, &rx, LiveView::new());
    println!("\nStopped.");
    result
}

/// The press-to-action daemon running in its own thread.
struct Daemon {
    model: Model,
    control: Sender<Control>,
    view: LiveView,
    handle: JoinHandle<()>,
}

/// Open the device and run the press-to-action daemon in a background thread.
fn start_daemon(config_path: PathBuf) -> Result<Daemon, Error> {
    let deck = StreamDeck::open_first()?;
    let model = *deck.model();
    let (control, rx) = mpsc::channel::<Control>();
    let view = LiveView::new();
    let daemon_view = view.clone();
    let handle = std::thread::spawn(move || {
        if let Err(err) = runtime::run_with_control(deck, config_path, &SHUTDOWN, &rx, daemon_view)
        {
            eprintln!("error: daemon stopped: {err}");
        }
        SHUTDOWN.store(true, Ordering::Relaxed);
    });
    Ok(Daemon {
        model,
        control,
        view,
        handle,
    })
}

/// Preferred local port for the editor, so the URL is stable across restarts.
const PREFERRED_WEB_ADDR: &str = "127.0.0.1:57617";

/// Bind the editor to the preferred port, falling back to an ephemeral one.
fn bind_web(daemon: &Daemon, config_path: PathBuf) -> Result<WebUi, Error> {
    let bind = |addr: &str, config_path: PathBuf| {
        WebUi::bind(
            addr,
            daemon.model,
            config_path,
            daemon.control.clone(),
            daemon.view.clone(),
        )
    };
    match bind(PREFERRED_WEB_ADDR, config_path.clone()) {
        Ok(web) => Ok(web),
        Err(_) => bind("127.0.0.1:0", config_path),
    }
}

/// Start the in-process web editor server.
fn start_web_ui(daemon: &Daemon, config_path: PathBuf) -> Result<(String, JoinHandle<()>), Error> {
    let web = bind_web(daemon, config_path)?;
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

    let daemon = start_daemon(config_path.clone())?;
    let (url, web) = start_web_ui(&daemon, config_path)?;
    let model = daemon.model;
    println!("Tray starting for {}. Editor at {url}", model.name);

    let tray = StreamDeckTray::new(
        daemon.control.clone(),
        &SHUTDOWN,
        url,
        format!("{} connected", model.name),
    );
    let _handle = tray.show()?;
    println!("Tray running. Use the tray menu (or Ctrl-C) to quit.");

    while !SHUTDOWN.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = daemon.handle.join();
    let _ = web.join();
    println!("\nStopped.");
    Ok(())
}

fn cmd_ui(args: &[String]) -> Result<(), Error> {
    let config_path = config_path(args);
    ensure_config_exists(&config_path)?;
    install_signal_handlers();

    let daemon = start_daemon(config_path.clone())?;
    let web = bind_web(&daemon, config_path)?;
    let url = web.url();
    println!("Editor for {} at {url}", daemon.model.name);
    open_in_browser(&url);

    let result = web.serve(&SHUTDOWN);
    SHUTDOWN.store(true, Ordering::Relaxed);
    let _ = daemon.handle.join();
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
         \x20 watch [seconds]          print button press/release events\n\n\
         LAYOUT (through the running daemon; docs/control-api.md):\n\
         \x20 layout                   the layout, as JSON\n\
         \x20 schema                   what a key may hold, as JSON\n\
         \x20 view                     what the deck shows now, as JSON (keys as PNG)\n\
         \x20 layout brightness <0-100>  store and apply brightness\n\
         \x20 key set <page> <key> <json|->  replace a key whole\n\
         \x20 key clear <page> <key>   blank a key\n\
         \x20 page add <name>          append a page\n\
         \x20 page remove <page>       remove a page\n\
         \x20 (options: --revision <rev>, --url <url>; env STREAMDECK_URL, STREAMDECK_TOKEN, STREAMDECK_TOKEN_FILE)\n\
         \x20 token issue <caller> [--out <file>]  mint a caller's token (never printed)\n\
         \x20 token revoke <caller>    delete a caller's token\n\
         \x20 token list               name the callers holding a token"
    );
}

/// The layout verbs: one call to the running daemon. Exit 0 done, 1 failed
/// before it answered, 2 refused by it (docs/control-api.md).
fn cmd_remote(args: &[String]) -> ExitCode {
    match remote_call(args) {
        Ok(answer) => {
            println!("{answer}");
            ExitCode::SUCCESS
        }
        Err((code, problem)) => {
            eprintln!("{}", problem.to_json());
            ExitCode::from(code)
        }
    }
}

fn remote_call(args: &[String]) -> Result<String, (u8, Problem)> {
    let failed = |problem: Problem| (1u8, problem);
    let call = remote::plan(args).map_err(failed)?;
    let url = call
        .url
        .clone()
        .or_else(|| {
            std::env::var("STREAMDECK_URL")
                .ok()
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| remote::DEFAULT_URL.to_string());
    let authority = remote::authority(&url).map_err(failed)?;
    let token = client_token().map_err(failed)?;
    let body = match call.body.as_deref() {
        Some("-") => {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                .map_err(|e| failed(Problem::new("usage", format!("standard input: {e}"))))?;
            Some(text)
        }
        other => other.map(String::from),
    };
    let (status, answer) =
        remote::send(&authority, &token, &call, body.as_deref()).map_err(failed)?;
    remote::judge(status, &answer).map_err(|problem| {
        let code = if problem.name == "unreachable" { 1 } else { 2 };
        (code, problem)
    })
}

/// `$STREAMDECK_TOKEN`, else the file `$STREAMDECK_TOKEN_FILE`, else the `cli` caller's.
fn client_token() -> Result<String, Problem> {
    if let Some(token) = std::env::var("STREAMDECK_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(token.trim().to_string());
    }
    if let Some(file) = std::env::var_os("STREAMDECK_TOKEN_FILE").filter(|v| !v.is_empty()) {
        return std::fs::read_to_string(&file)
            .map(|t| t.trim().to_string())
            .map_err(|e| {
                Problem::new(
                    "no-token",
                    format!("{}: {e}", PathBuf::from(file).display()),
                )
            });
    }
    TokenStore::new(tokens::default_dir())
        .read("cli")
        .map_err(|_| {
            Problem::new(
                "no-token",
                "no token found; issue one with `streamdeck token issue cli`",
            )
        })
}

fn cmd_token(args: &[String]) -> Result<(), Error> {
    let store = TokenStore::new(tokens::default_dir());
    let bad = |why: &str| Error::ConfigInvalid(why.to_string());
    match args.get(1).map(String::as_str) {
        Some("issue") => {
            let caller = args
                .get(2)
                .ok_or_else(|| bad("token issue <caller> [--out <file>]"))?;
            let out = match (args.get(3).map(String::as_str), args.get(4)) {
                (Some("--out"), Some(file)) => Some(PathBuf::from(file)),
                (None, None) => None,
                _ => return Err(bad("token issue <caller> [--out <file>]")),
            };
            let path = store.issue(caller, out.as_deref())?;
            println!("issued a token for '{caller}' in {}", path.display());
            if let Some(out) = out {
                println!("and wrote the same token to {}", out.display());
            }
            Ok(())
        }
        Some("revoke") => {
            let caller = args.get(2).ok_or_else(|| bad("token revoke <caller>"))?;
            if store.revoke(caller)? {
                println!("revoked '{caller}'");
            } else {
                println!("'{caller}' held no token");
            }
            Ok(())
        }
        Some("list") => {
            for caller in store.list()? {
                println!("{caller}");
            }
            Ok(())
        }
        _ => Err(bad("token issue|revoke|list")),
    }
}
