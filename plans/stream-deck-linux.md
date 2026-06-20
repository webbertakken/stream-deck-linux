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
- [ ] `Button` model: image path + action (run command, etc.)
- [ ] Config file format (per-key image + function) + loader
- [ ] Event loop: on key press -> run mapped action
- [ ] Render all configured images on startup; brightness from config

### Daemon + ergonomics
- [ ] `streamdeckd` long-running daemon, graceful reset on exit
- [ ] Hot-reload config on change
- [ ] CLI: list devices, set brightness, set image, reset
- [ ] udev rule doc / install helper (uaccess already present here)
- [ ] CI: fmt + clippy + test
- [ ] README with setup + usage
