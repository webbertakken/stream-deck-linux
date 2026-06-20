//! `streamdeck` - command-line control for Elgato Stream Deck on Linux.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use streamdeck::events::diff_states;
use streamdeck::{Error, StreamDeck};

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
         \x20 watch [seconds]          print button press/release events"
    );
}
