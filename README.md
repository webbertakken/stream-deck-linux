# stream-deck-linux

Custom Linux control software for the Elgato Stream Deck, written in Rust.
Assign every key a **picture** and a **function** from a simple TOML file.

It talks to the device directly over `/dev/hidraw*` - no `libhidapi`, no
`libusb`, no system services. The wire protocol is grounded in the device's own
HID report descriptor, not guessed.

> Verified end-to-end on a **Stream Deck MK.2** (`0fd9:0080`): device identity,
> brightness, key images (colour, pictures, text), button events and built-in
> actions all confirmed against real hardware.

## Features

- Per-key **pictures** (PNG/JPEG, auto-resized), **solid colours**, and
  **centred text labels** (with a bundled font), or any combination.
- Per-key **functions**: run a shell command, or a device **built-in**
  (brightness up/down/set, reset).
- A single **TOML config** describes the whole layout.
- A long-running `run` mode renders the layout and dispatches actions on press,
  with a graceful, deck-clearing shutdown on Ctrl-C.
- A handful of one-shot CLI commands for quick tweaks and scripting.

## Hardware support

| Model            | USB id      | Status              |
| ---------------- | ----------- | ------------------- |
| Stream Deck MK.2 | `0fd9:0080` | Verified on hardware |

The model registry (`src/model.rs`) is structured so other Stream Decks slot in
by adding their key grid and image spec.

## Install

```bash
cargo build --release
# binary at target/release/streamdeck
```

### Permissions

The device node (`/dev/hidraw*`) must be accessible to your user. The common
approach is a udev rule that grants the logged-in user access via `uaccess`:

```
# /etc/udev/rules.d/70-streamdeck.rules
SUBSYSTEM=="usb", ATTRS{idVendor}=="0fd9", TAG+="uaccess"
```

Then reload rules and replug the device:

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Verify with `streamdeck list`.

## Quick start

```bash
streamdeck list                     # find connected decks
streamdeck info                     # model, firmware, serial
streamdeck brightness 70            # 0-100
streamdeck color 0 1E1E2E           # fill key 0 with a colour
streamdeck image 2 ~/pics/icon.png  # render a picture onto key 2
streamdeck clear                    # blank all keys
streamdeck reset                    # back to the standby logo
streamdeck watch                    # print button press/release events
streamdeck run examples/demo-config.toml   # render layout + dispatch actions
```

`run` with no path looks at `$XDG_CONFIG_HOME/streamdeck/config.toml`
(usually `~/.config/streamdeck/config.toml`).

## Config reference

A config is TOML with an optional `brightness` and a list of `[[buttons]]`.

```toml
brightness = 70          # 0-100, applied on load

[[buttons]]
key = 0                  # hardware key index (0..N-1)
image = "icons/app.png"  # relative to this file; ~ expands to $HOME
label = "App"            # optional centred text, drawn over the background
text_color = "#FFFFFF"   # optional, defaults to white
run = "alacritty"        # shell command on press (via sh -c)

[[buttons]]
key = 5
color = "#444466"        # solid colour background (#RRGGBB or RRGGBB)
label = "Bright+"
builtin = "brightness_up"   # device-native action instead of `run`
```

### Button fields

| Field        | Meaning                                                        |
| ------------ | -------------------------------------------------------------- |
| `key`        | Hardware key index (required).                                 |
| `image`      | Picture file; relative paths resolve next to the config file.  |
| `color`      | Solid background `#RRGGBB` (used if no image, or as text bg).   |
| `label`      | Centred text, auto-shrunk to fit; drawn over the background.   |
| `text_color` | Label colour `#RRGGBB` (default white).                        |
| `run`        | Shell command executed on press.                               |
| `builtin`    | Device-native action on press (mutually exclusive with `run`). |

A key needs at least one of `image`, `color`, or `label`.

### Built-in actions

| `builtin` value         | Effect                                |
| ----------------------- | ------------------------------------- |
| `brightness_up`         | Increase brightness by 10%.           |
| `brightness_down`       | Decrease brightness by 10%.           |
| `brightness_set:N`      | Set brightness to `N`% (0-100).       |
| `reset`                 | Reset device to its standby logo.     |

Everything else (media keys, volume, hotkeys, launching apps) is a `run`
command, e.g. `playerctl play-pause`, `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle`,
or `ydotool key 29:1 46:1 46:0 29:0`.

## How it works

- `protocol` - pure builders/parsers for the HID reports (image packets,
  brightness/reset feature reports, key-state input reports).
- `hid` - a tiny pure-Rust hidraw backend (`ioctl` for feature reports and
  device info, plain `read`/`write` for input/output reports).
- `model` - per-device key grid and image spec (size, format, orientation).
- `image` / `render` - fit, orient and encode key images; compose backgrounds
  with centred text.
- `device` - the high-level `StreamDeck` API.
- `config` / `actions` / `runtime` - the TOML layout, action model, and the
  render + press-to-action loop.

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run --example preview   # writes a contact sheet PNG of composed keys
```

## Licence

Project code is MIT (see `LICENSE`). The bundled font
`assets/LiberationSans-Regular.ttf` is licensed under SIL OFL 1.1
(`assets/LiberationSans-LICENSE.txt`).
