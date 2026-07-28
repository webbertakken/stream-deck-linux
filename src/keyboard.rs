//! Virtual keyboard: hold real keyboard keys down via a Linux uinput device.
//!
//! Splits cleanly into a **pure** part (key-name -> evdev keycode mapping and
//! spec parsing, unit-tested without hardware) and an **impure** part (creating
//! the `/dev/uinput` virtual device and emitting events), mirroring the shape
//! of [`crate::system`].
//!
//! Holding a key via uinput is deliberate: if the daemon dies while a key is
//! held, closing the uinput file descriptor makes the kernel auto-release the
//! key, so a crash can never leave a key stuck down. It emits real evdev events
//! (games and raw-input apps see them) and works identically on X11 and
//! Wayland with no external binary.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use crate::error::{Error, Result};

/// A Linux evdev key code (the `KEY_*` values from `input-event-codes.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCode(pub u16);

/// The key-name -> evdev keycode table: the single source of truth for which
/// keys the app can emit. Names are lower-case; aliases share a code. Values
/// are the `KEY_*` codes from `linux/input-event-codes.h`.
///
/// To support another key, add a `(name, code)` row here. The uinput device
/// enables every code in this table, so a new entry works everywhere
/// automatically.
const KEYS: &[(&str, u16)] = &[
    // Letters.
    ("a", 30),
    ("b", 48),
    ("c", 46),
    ("d", 32),
    ("e", 18),
    ("f", 33),
    ("g", 34),
    ("h", 35),
    ("i", 23),
    ("j", 36),
    ("k", 37),
    ("l", 38),
    ("m", 50),
    ("n", 49),
    ("o", 24),
    ("p", 25),
    ("q", 16),
    ("r", 19),
    ("s", 31),
    ("t", 20),
    ("u", 22),
    ("v", 47),
    ("w", 17),
    ("x", 45),
    ("y", 21),
    ("z", 44),
    // Digits (top row).
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
    ("0", 11),
    // Function keys.
    ("f1", 59),
    ("f2", 60),
    ("f3", 61),
    ("f4", 62),
    ("f5", 63),
    ("f6", 64),
    ("f7", 65),
    ("f8", 66),
    ("f9", 67),
    ("f10", 68),
    ("f11", 87),
    ("f12", 88),
    // Modifiers (aliases share a code).
    ("ctrl", 29), // KEY_LEFTCTRL
    ("control", 29),
    ("shift", 42),  // KEY_LEFTSHIFT
    ("alt", 56),    // KEY_LEFTALT
    ("super", 125), // KEY_LEFTMETA
    ("meta", 125),
    ("win", 125),
    ("altgr", 100), // KEY_RIGHTALT
    // Whitespace / editing.
    ("space", 57),
    ("enter", 28),
    ("return", 28),
    ("tab", 15),
    ("esc", 1),
    ("escape", 1),
    ("backspace", 14),
    ("delete", 111),
    ("insert", 110),
    ("home", 102),
    ("end", 107),
    ("pageup", 104),
    ("pagedown", 109),
    // Arrows.
    ("up", 103),
    ("down", 108),
    ("left", 105),
    ("right", 106),
    // Punctuation.
    ("minus", 12),
    ("equal", 13),
    ("comma", 51),
    ("dot", 52),
    ("period", 52),
    ("slash", 53),
    ("semicolon", 39),
];

/// Map a single (already lower-cased) key name to its evdev keycode.
fn keycode_for(name: &str) -> Option<u16> {
    KEYS.iter().find(|(n, _)| *n == name).map(|(_, code)| *code)
}

/// Every distinct evdev code the app can emit, for enabling on the uinput
/// device. Order is irrelevant; duplicates (from aliases) are dropped.
fn all_keycodes() -> Vec<u16> {
    let mut codes: Vec<u16> = KEYS.iter().map(|(_, code)| *code).collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}

/// Parse a `+`-separated key spec (e.g. `ctrl+shift+f`) into ordered evdev
/// codes, preserving the written order so modifiers press before the main key.
///
/// Names are trimmed and matched case-insensitively. An empty spec or an
/// unknown name is a [`Error::ConfigInvalid`], so a typo fails at config load
/// rather than silently at press time.
pub fn parse_keys(spec: &str) -> Result<Vec<KeyCode>> {
    if spec.trim().is_empty() {
        return Err(Error::ConfigInvalid("hold key spec is empty".into()));
    }
    let mut codes = Vec::new();
    for part in spec.split('+') {
        let name = part.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err(Error::ConfigInvalid(format!(
                "hold key spec '{spec}' has an empty segment"
            )));
        }
        match keycode_for(&name) {
            Some(code) => codes.push(KeyCode(code)),
            None => {
                return Err(Error::ConfigInvalid(format!(
                    "unknown key name '{name}' in hold spec '{spec}'"
                )))
            }
        }
    }
    Ok(codes)
}

/// A sink for key press / release events.
///
/// The runtime latch drives this trait, so it can be unit-tested against a
/// recording fake while the real implementation talks to uinput.
pub trait KeyEmitter {
    /// Press `code` down (evdev value 1).
    fn key_down(&mut self, code: KeyCode) -> Result<()>;
    /// Release `code` (evdev value 0).
    fn key_up(&mut self, code: KeyCode) -> Result<()>;
    /// Release every key currently held. The safety valve on shutdown / reload.
    fn release_all(&mut self) -> Result<()>;
}

/// A test emitter that records every down/up/release call and tracks which
/// codes it believes are held, so the latch state machine is verifiable
/// without a uinput device.
#[derive(Debug, Default)]
pub struct RecordingEmitter {
    /// The ordered log of calls, each `("down"|"up", code)`.
    pub events: Vec<(&'static str, KeyCode)>,
    /// Codes currently held (in press order).
    pub held: Vec<KeyCode>,
}

impl KeyEmitter for RecordingEmitter {
    fn key_down(&mut self, code: KeyCode) -> Result<()> {
        self.events.push(("down", code));
        if !self.held.contains(&code) {
            self.held.push(code);
        }
        Ok(())
    }

    fn key_up(&mut self, code: KeyCode) -> Result<()> {
        self.events.push(("up", code));
        self.held.retain(|c| *c != code);
        Ok(())
    }

    fn release_all(&mut self) -> Result<()> {
        for code in std::mem::take(&mut self.held) {
            self.events.push(("up", code));
        }
        Ok(())
    }
}

// ---- Real uinput virtual keyboard ----

// ioctl direction bits (asm-generic, matching `hid.rs`).
const IOC_WRITE: u64 = 1;
const UINPUT_IOCTL_BASE: u64 = b'U' as u64;

/// Encode a Linux ioctl request number (asm-generic layout).
const fn ioc(dir: u64, ty: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (ty << 8) | nr
}

/// `UI_DEV_CREATE` - `_IO('U', 1)`.
const fn ui_dev_create() -> u64 {
    ioc(0, UINPUT_IOCTL_BASE, 1, 0)
}
/// `UI_DEV_DESTROY` - `_IO('U', 2)`.
const fn ui_dev_destroy() -> u64 {
    ioc(0, UINPUT_IOCTL_BASE, 2, 0)
}
/// `UI_DEV_SETUP` - `_IOW('U', 3, struct uinput_setup)`.
const fn ui_dev_setup() -> u64 {
    ioc(
        IOC_WRITE,
        UINPUT_IOCTL_BASE,
        3,
        core::mem::size_of::<UinputSetup>() as u64,
    )
}
/// `UI_SET_EVBIT` - `_IOW('U', 100, int)`.
const fn ui_set_evbit() -> u64 {
    ioc(IOC_WRITE, UINPUT_IOCTL_BASE, 100, 4)
}
/// `UI_SET_KEYBIT` - `_IOW('U', 101, int)`.
const fn ui_set_keybit() -> u64 {
    ioc(IOC_WRITE, UINPUT_IOCTL_BASE, 101, 4)
}

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const SYN_REPORT: u16 = 0;
const BUS_USB: u16 = 0x03;
const UINPUT_MAX_NAME_SIZE: usize = 80;

/// Mirrors the kernel `struct input_event` (64-bit `timeval`).
#[repr(C)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

/// Mirrors the kernel `struct input_id`.
#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

/// Mirrors the kernel `struct uinput_setup`.
#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; UINPUT_MAX_NAME_SIZE],
    ff_effects_max: u32,
}

/// Kernel settle time after `UI_DEV_CREATE` before the first event, so the
/// event is not dropped while userspace (X/Wayland) binds the new device.
const CREATE_SETTLE: Duration = Duration::from_millis(200);

/// A virtual keyboard backed by `/dev/uinput`.
///
/// Enables every code in [`KEYS`] on creation, then emits real evdev key events.
/// Held keys are tracked so [`Self::release_all`] and `Drop` can free them; the
/// kernel also auto-releases everything when the fd closes, so a crash while a
/// key is held never leaves it stuck.
pub struct UinputKeyboard {
    file: File,
    fd: RawFd,
    held: Vec<KeyCode>,
}

impl UinputKeyboard {
    /// Create the virtual keyboard device.
    pub fn open() -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map_err(|err| {
                Error::ConfigInvalid(format!(
                    "cannot open /dev/uinput ({err}); a hold key needs write access to it \
                     (see the udev rule in the README)"
                ))
            })?;
        let fd = file.as_raw_fd();

        set_int(fd, ui_set_evbit(), EV_KEY as i32)?;
        for code in all_keycodes() {
            set_int(fd, ui_set_keybit(), code as i32)?;
        }

        let mut name = [0u8; UINPUT_MAX_NAME_SIZE];
        let label = b"stream-deck-linux virtual keyboard";
        name[..label.len()].copy_from_slice(label);
        let setup = UinputSetup {
            id: InputId {
                bustype: BUS_USB,
                vendor: 0x1209,
                product: 0x5d00,
                version: 1,
            },
            name,
            ff_effects_max: 0,
        };
        let rc = unsafe { libc::ioctl(fd, ui_dev_setup(), &setup as *const _) };
        if rc < 0 {
            return Err(Error::Io(io::Error::last_os_error()));
        }
        let rc = unsafe { libc::ioctl(fd, ui_dev_create()) };
        if rc < 0 {
            return Err(Error::Io(io::Error::last_os_error()));
        }
        // Let the kernel and desktop bind the device before the first event.
        std::thread::sleep(CREATE_SETTLE);

        Ok(Self {
            file,
            fd,
            held: Vec::new(),
        })
    }

    /// Emit a single key event followed by a `SYN_REPORT`.
    fn emit(&mut self, code: KeyCode, value: i32) -> io::Result<()> {
        self.write_event(EV_KEY, code.0, value)?;
        self.write_event(EV_SYN, SYN_REPORT, 0)?;
        Ok(())
    }

    fn write_event(&mut self, type_: u16, code: u16, value: i32) -> io::Result<()> {
        let event = InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            type_,
            code,
            value,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &event as *const InputEvent as *const u8,
                core::mem::size_of::<InputEvent>(),
            )
        };
        self.file.write_all(bytes)
    }
}

impl KeyEmitter for UinputKeyboard {
    fn key_down(&mut self, code: KeyCode) -> Result<()> {
        self.emit(code, 1)?;
        if !self.held.contains(&code) {
            self.held.push(code);
        }
        Ok(())
    }

    fn key_up(&mut self, code: KeyCode) -> Result<()> {
        self.emit(code, 0)?;
        self.held.retain(|c| *c != code);
        Ok(())
    }

    fn release_all(&mut self) -> Result<()> {
        for code in std::mem::take(&mut self.held) {
            self.emit(code, 0)?;
        }
        Ok(())
    }
}

impl Drop for UinputKeyboard {
    fn drop(&mut self) {
        let _ = self.release_all();
        unsafe {
            libc::ioctl(self.fd, ui_dev_destroy());
        }
        // Closing `self.file` also makes the kernel release any held keys.
    }
}

/// Pass an integer argument to an ioctl (used for `UI_SET_*BIT`).
fn set_int(fd: RawFd, request: u64, value: i32) -> Result<()> {
    let rc = unsafe { libc::ioctl(fd, request, value as libc::c_int) };
    if rc < 0 {
        return Err(Error::Io(io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_emitter_tracks_held_and_logs() {
        let mut emitter = RecordingEmitter::default();
        emitter.key_down(KeyCode(29)).unwrap();
        emitter.key_down(KeyCode(33)).unwrap();
        assert_eq!(emitter.held, vec![KeyCode(29), KeyCode(33)]);
        emitter.key_up(KeyCode(33)).unwrap();
        assert_eq!(emitter.held, vec![KeyCode(29)]);
        emitter.release_all().unwrap();
        assert!(emitter.held.is_empty());
        assert_eq!(
            emitter.events,
            vec![
                ("down", KeyCode(29)),
                ("down", KeyCode(33)),
                ("up", KeyCode(33)),
                ("up", KeyCode(29)),
            ]
        );
    }

    #[test]
    fn parses_single_letter() {
        assert_eq!(parse_keys("f").unwrap(), vec![KeyCode(33)]);
    }

    #[test]
    fn parses_combo_in_written_order() {
        assert_eq!(
            parse_keys("ctrl+shift+f").unwrap(),
            vec![KeyCode(29), KeyCode(42), KeyCode(33)]
        );
    }

    #[test]
    fn is_case_insensitive_and_trims() {
        assert_eq!(
            parse_keys("  Ctrl + Shift + F ").unwrap(),
            vec![KeyCode(29), KeyCode(42), KeyCode(33)]
        );
    }

    #[test]
    fn honours_aliases() {
        assert_eq!(parse_keys("control").unwrap(), parse_keys("ctrl").unwrap());
        assert_eq!(parse_keys("escape").unwrap(), parse_keys("esc").unwrap());
        assert_eq!(parse_keys("return").unwrap(), parse_keys("enter").unwrap());
        assert_eq!(parse_keys("period").unwrap(), parse_keys("dot").unwrap());
        assert_eq!(parse_keys("super").unwrap(), vec![KeyCode(125)]);
        assert_eq!(parse_keys("win").unwrap(), vec![KeyCode(125)]);
    }

    #[test]
    fn maps_representative_table_subset() {
        assert_eq!(parse_keys("a").unwrap(), vec![KeyCode(30)]);
        assert_eq!(parse_keys("z").unwrap(), vec![KeyCode(44)]);
        assert_eq!(parse_keys("0").unwrap(), vec![KeyCode(11)]);
        assert_eq!(parse_keys("1").unwrap(), vec![KeyCode(2)]);
        assert_eq!(parse_keys("f12").unwrap(), vec![KeyCode(88)]);
        assert_eq!(parse_keys("space").unwrap(), vec![KeyCode(57)]);
        assert_eq!(parse_keys("tab").unwrap(), vec![KeyCode(15)]);
        assert_eq!(parse_keys("up").unwrap(), vec![KeyCode(103)]);
        assert_eq!(parse_keys("altgr").unwrap(), vec![KeyCode(100)]);
    }

    #[test]
    fn rejects_unknown_name() {
        let err = parse_keys("boguskey").unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("unknown key name")));
    }

    #[test]
    fn rejects_empty_spec() {
        assert!(matches!(
            parse_keys("").unwrap_err(),
            Error::ConfigInvalid(_)
        ));
        assert!(matches!(
            parse_keys("   ").unwrap_err(),
            Error::ConfigInvalid(_)
        ));
    }

    #[test]
    fn rejects_empty_segment() {
        let err = parse_keys("ctrl+").unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid(m) if m.contains("empty segment")));
    }

    #[test]
    fn all_keycodes_are_unique_and_cover_aliases() {
        let codes = all_keycodes();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes.len(), sorted.len(), "all_keycodes must be unique");
        // Aliases collapse to one code, so distinct codes < table rows.
        assert!(codes.len() < KEYS.len());
        assert!(codes.contains(&33)); // f
        assert!(codes.contains(&125)); // super/meta/win share one code
    }

    // Ground truth: the well-known Linux uinput ioctl request numbers.
    #[test]
    fn uinput_ioctl_numbers_match_linux_constants() {
        assert_eq!(ui_dev_create(), 0x5501);
        assert_eq!(ui_dev_destroy(), 0x5502);
        assert_eq!(ui_dev_setup(), 0x405C_5503);
        assert_eq!(ui_set_evbit(), 0x4004_5564);
        assert_eq!(ui_set_keybit(), 0x4004_5565);
    }

    #[test]
    fn kernel_structs_have_expected_sizes() {
        assert_eq!(core::mem::size_of::<InputEvent>(), 24);
        assert_eq!(core::mem::size_of::<InputId>(), 8);
        assert_eq!(core::mem::size_of::<UinputSetup>(), 92);
    }

    /// Smoke test: only runs where `/dev/uinput` is writable (skips otherwise,
    /// so CI without uinput still passes). Creates the device and emits a
    /// keydown+keyup, asserting no error - it does not assert on focus.
    #[test]
    fn uinput_device_emits_when_available() {
        if OpenOptions::new().write(true).open("/dev/uinput").is_err() {
            eprintln!("skipping: /dev/uinput not writable on this host");
            return;
        }
        let mut kbd = UinputKeyboard::open().expect("create uinput keyboard");
        let f = parse_keys("f").unwrap()[0];
        kbd.key_down(f).expect("key down");
        assert_eq!(kbd.held, vec![f]);
        kbd.key_up(f).expect("key up");
        assert!(kbd.held.is_empty());
    }
}
