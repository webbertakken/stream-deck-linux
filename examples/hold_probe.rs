//! Hardware/kernel probe for the hold-key latch, independent of the deck.
//!
//! Creates the real uinput virtual keyboard, holds a key/combo down for a
//! while, then releases it. Used to verify (via `xinput test-xi2 --root`) that
//! a held key reaches the desktop globally, and that killing the process while
//! a key is held still releases it (kernel auto-release on fd close).
//!
//! Usage:
//!   cargo run --example hold_probe -- <spec> [hold_ms] [--hang]
//!     <spec>     key or combo, e.g. `f` or `ctrl+shift+f`
//!     hold_ms    how long to hold before releasing (default 1500)
//!     --hang     hold, then sleep forever WITHOUT releasing (kill -9 to test
//!                crash-safety: the kernel must release on fd close)

use std::time::Duration;

use streamdeck::keyboard::{parse_keys, KeyEmitter, UinputKeyboard};

fn main() {
    let spec = std::env::args().nth(1).unwrap_or_else(|| "f".to_string());
    let hold_ms: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500);
    let hang = std::env::args().any(|a| a == "--hang");

    let codes = parse_keys(&spec).expect("valid key spec");
    let mut kbd = UinputKeyboard::open().expect("open /dev/uinput");

    eprintln!("holding {spec} ({} code(s))", codes.len());
    for code in &codes {
        kbd.key_down(*code).expect("key down");
    }

    if hang {
        eprintln!("held; sleeping forever (kill -9 to test crash release)");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    std::thread::sleep(Duration::from_millis(hold_ms));
    for code in codes.iter().rev() {
        kbd.key_up(*code).expect("key up");
    }
    eprintln!("released {spec}");
}
