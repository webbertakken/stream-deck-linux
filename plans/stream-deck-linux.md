# Stream Deck Linux - custom control software

Custom Rust software to drive an Elgato Stream Deck on Linux: assign each key a
picture and a function. Target hardware verified present: **Stream Deck MK.2**
(`0fd9:0080`) on `/dev/hidraw0`, reachable via `uaccess` ACL (no root needed).

## Protocol facts (verified against the device's HID report descriptor)

- Output report ID `0x02`, 1023-byte payload -> 1024-byte image packets.
- Input report ID `0x01`, 511-byte payload -> 15 key states (states at offset 4).
- Feature reports ID `0x03`..`0x0c`, 31-byte payload (32 incl. report id).
  - Brightness: `[0x03, 0x08, percent]`. Reset: `[0x03, 0x02]`.
  - Firmware: get-feature `0x05`, string at offset 6. Serial: `0x06`, offset 2.
- MK.2 image: 72x72 JPEG, rotated 180 deg (flip both axes).

## Approach

Pure-Rust hidraw backend (direct `/dev/hidraw*` open + ioctls) so there is no
libudev/hidapi system dependency. `image` crate for the picture pipeline.

## Tasks

### Foundation - device library (no hardware needed, TDD)
- [x] Scaffold Cargo lib+bin project, `.gitignore`, minimal deps
- [x] `protocol`: brightness/reset feature builders (unit tested)
- [x] `protocol`: image report packetiser for report `0x02` (unit tested)
- [x] `protocol`: parse 15 key states from input report (unit tested)
- [x] `model`: MK.2 spec (keys, grid, image size/format/rotation) + key indexing
- [x] `image`: load file -> resize 72x72 -> rotate 180 -> JPEG bytes (unit tested)
- [x] `hid`: ioctl numbers (`HIDIOCGRAWINFO`/`SFEATURE`/`GFEATURE`) computed + tested
- [x] `hid`: RawHidDevice open/enumerate/write/read/feature wrappers

### Prove against hardware (the real test)
- [x] Enumerate + identify the MK.2, print firmware + serial (fw 1.02.000)
- [x] Set brightness, reset
- [x] Push a solid-colour image to a key, then a real picture
- [x] Read button press/release events in a loop (verified vs raw hidraw)

### Configuration + action engine
- [x] `Button` model: image path + action (run command, etc.)
- [x] Config file format (per-key image + function) + loader (TOML)
- [x] Event loop: on key press -> run mapped action (proven on hardware)
- [x] Render all configured images on startup; brightness from config
- [x] Graceful shutdown clears deck, EINTR-safe (clean exit 0 on Ctrl-C)
- [x] Built-in actions (deck brightness up/down/set, reset) alongside `run`
- [x] On-key text labels (bundled font, rendered onto keys, verified)

### Daemon + ergonomics
- [x] CLI: list devices, set brightness, set image, reset
- [x] udev rule doc (uaccess already present here) - in README
- [x] CI: fmt + clippy + test
- [x] README with setup + usage
- [ ] `run` mode hot-reloads config on file change

### Desktop integration (icon, tray, UI, autostart)
Desktop verified: Linux Mint 22.3 Cinnamon, X11. SNI tray host present
(`xapp-sn-watcher`), so a pure-Rust `ksni` tray works without GTK.

- [x] App icon generated (5x3 key grid PNGs, `assets/icons/`)
- [ ] System tray (ksni/SNI): status + menu (open editor, brightness, reload,
      reset, quit); uses the app icon
- [ ] Tray app architecture: `streamdeck tray` runs embedded device daemon in a
      thread + tray on the main thread; actions via a control channel
- [ ] Autostart on login: install/remove `~/.config/autostart/streamdeck.desktop`
      (`streamdeck autostart enable|disable|status`)
- [ ] Config editor UI: 5x3 key grid; click a key to set image/colour/label/
      action; live-apply to device + save TOML (toolkit pending decision)
- [ ] `streamdeck install` helper: copy icon to hicolor, write .desktop entries
- [ ] Update CI to install GUI build deps once the UI toolkit lands
