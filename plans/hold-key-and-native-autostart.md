# Hold-a-keyboard-key toggle + native autostart & dev auto-reload

Two bodies of work:

1. **Ops**: switch the daemon from PM2 to the app's own **native autostart**
   (`.desktop` on login), and add a **dev loop** so every rebuild replaces the
   running instance (always latest), while still starting on boot.
2. **Feature**: a Stream Deck key that, on press, **holds a keyboard key down**
   (e.g. `F`) and, on the next press of the same key, **releases** it. A
   press-to-toggle latch for one key or a modifier combo.

Work each task with TDD (red -> green -> commit -> refactor -> commit). Tick one
checkbox at a time: read the task, implement it the most idiomatic way, verify,
tick, commit, move on. Never batch ticks.

---

## Background the implementer must absorb first

Read these before touching code; they define the seams you will extend.

- `src/actions.rs` - `KeyAction` enum (`Run`/`Builtin`/`Macro`/`Toggle`) and the
  `Builtin` parser. You will add a `Hold` variant here.
- `src/config.rs` - `ButtonConfig` / `ButtonState`, `validate()` /
  `validate_one()` (mutual-exclusion rules, "at least one visual"), TOML
  round-trip. You will add a `hold` field and its validation.
- `src/runtime.rs` - `Session`: the daemon loop, `poll()` press/release
  dispatch, `render_key()`/`effective_button()` (toggle-aware rendering),
  `render_current()`, `reload()`, and shutdown/cleanup in `run_with_control()`.
  This is where the hold state machine and safe release-on-exit live.
- `src/system.rs` - the pattern to copy for a **pure builder + impure
  detection** module with unit tests. The new `keyboard`/uinput module mirrors
  this shape (pure key-name -> keycode map; impure device creation).
- `src/hid.rs` - the existing raw-`ioctl`/`libc` backend. The uinput emitter
  follows the same low-level style (structs, `ioctl`, `write`).
- `src/webui.rs` + `assets/web/{index.html,app.js,app.css}` - the editor. It
  round-trips the whole `Config` as JSON; adding a `hold` action type means a
  new radio option + text input + the load/collect wiring in `app.js`.
- `src/main.rs` - `cmd_autostart` (already wires `autostart enable` to
  `<exe> tray`) and `cmd_tray`. `src/autostart.rs` writes the `.desktop`.

### Environment facts (verified on this machine)

- Session is **X11 / Cinnamon**, but the design MUST also work on Wayland.
- `/dev/uinput` exists and is **ACL-granted to the user** (`user:<you>:rw-`), so
  a virtual keyboard can be created **without root**. Confirm with
  `getfacl /dev/uinput`. If a fresh machine lacks the ACL, document the udev
  rule needed (see Phase 6).
- Verified Stream Deck: **MK.2** on `/dev/hidraw0` (15 keys, 5x3).

### Why uinput (not xdotool/ydotool) - decision, do not relitigate

- **Safety**: if the daemon dies while a key is held, closing the uinput fd
  makes the **kernel auto-release** the key. `xdotool keydown` leaves the key
  **stuck down in X forever** after the process exits - unacceptable for a
  latch. This alone decides it.
- **Reach**: uinput emits real evdev events at the kernel level, so games and
  raw-input apps see the held key; XTEST synthetic events often do not.
- **Portability**: one code path for X11 and Wayland; no external binary to
  install.
- **Fit**: pure-Rust `ioctl`/`write`, matching the project's `hid.rs` ethos and
  its "no libusb/libhidapi/system services" stance.

---

## Decisions baked into this plan (implement as stated)

- **D1 - Config field name**: `hold = "F"` on a `[[buttons]]` (and on a toggle
  `state`? NO - keep `hold` a top-level button action only, mutually exclusive
  with `run`/`builtin`/`macro`/`states`, exactly like the others).
- **D2 - Combo syntax**: `+`-separated, e.g. `ctrl+shift+f`, `alt+Tab`,
  `space`. Case-insensitive for names; canonical internal keycodes.
- **D3 - Latch semantics**: first press = press-and-hold all codes (modifiers
  first, main key last); second press of the **same deck key** = release all
  (reverse order). State is per deck key and per page; leaving a page while a
  key is held keeps it held (the physical keyboard state is global), but a held
  key MUST be released on daemon shutdown and on `reload` if its button no
  longer defines that hold.
- **D4 - Visual for the held state**: while latched-on, the key renders with a
  **persistent highlight** (reuse `Surface::brighten(0.3)`), distinct from the
  momentary press-flash. Optional nicety (only if cheap): honour an
  `active_color` / `active_label` on the button for the held state; if you do
  not implement it, do not leave dead config.
- **D5 - Emitter abstraction**: a `KeyEmitter` trait (`key_down(code)` /
  `key_up(code)`) with a real `UinputKeyboard` and a test `RecordingEmitter`.
  The uinput device is created **lazily** on first hold use (decks with no hold
  keys never create a virtual device; `run`/tests without uinput access never
  fail to start). Creation failure logs a clear, actionable error and the press
  is a no-op (no panic, no crash).
- **D6 - Native autostart** replaces PM2. The PM2 process has already been
  removed by the operator; do NOT re-add PM2.
- **D7 - Dev loop**: a `scripts/dev.sh` using `cargo watch` that on change
  rebuilds `--release` and restarts the running `tray` (kills the previous
  instance first, since the single HID device and the fixed web port cannot be
  shared). Boot start stays via the native `.desktop`.

## Ambiguities to confirm with the operator BEFORE the phase that needs them

- **A1 (before Phase 4)**: held-key visual - persistent brighten (default) vs a
  configurable `active_color`/`active_label`. Escalate only if you want to add
  the configurable form; otherwise ship the default and move on.
- **A2 (before Phase 0 dev loop)**: dev auto-reload mechanism - `cargo watch`
  restart script (planned default) vs a systemd user path-unit vs a git hook.
  Default to `cargo watch`; escalate only if `cargo watch` cannot be installed.
- **A3 (key-name coverage)**: which key names must the first release support?
  Default set below (Phase 1). Escalate only if the operator needs an exotic
  key not in the table.

---

## Phase 0 - Ops: native autostart + dev auto-reload + boot

- [x] Confirm PM2 no longer manages the app (`pm2 list` shows no
      `stream-deck-linux`); if present, `pm2 delete stream-deck-linux && pm2 save`.
- [x] Build release (`cargo build --release`) so `autostart enable` points the
      `.desktop` `Exec` at the just-built binary (`std::env::current_exe()`).
- [x] Enable native autostart: `./target/release/streamdeck autostart enable`,
      then confirm `autostart status` shows enabled and the `.desktop` `Exec`
      ends in ` tray`. Note the `.desktop` `Exec` is an absolute path to the
      built binary; document that re-enabling after moving the binary is needed.
- [x] Add `scripts/dev.sh` (executable, `set -euo pipefail`): builds
      `--release`, kills any running `streamdeck` instance (precise match, not a
      broad `pkill`), then launches `streamdeck tray` in the foreground; on
      change it rebuilds and restarts. Use `cargo watch -w src -w assets -s
      'bash scripts/dev.sh --once'` style, or implement the watch inside the
      script - pick the simpler, robust option. The running instance must always
      be the latest build. Guard against two instances fighting over the single
      HID device / fixed web port.
- [x] Add `scripts/dev.sh` prerequisite note (needs `cargo watch`; if absent,
      print an actionable install hint - do not silently no-op).
- [x] Document both in `README.md` (Development section): native autostart for
      boot, `scripts/dev.sh` for the always-latest dev loop, and that the two
      must not run simultaneously (the dev loop should stop the autostarted
      instance first).
- [x] Manually verify: run `scripts/dev.sh`, touch a source file, confirm the
      device-driving process is replaced by the new build (log line / pid
      change), and the deck still responds.
- [x] Commit.

## Phase 1 - Keyboard emitter (uinput) + keycode map (TDD, pure where possible)

- [x] New module `src/keyboard.rs` (add `pub mod keyboard;` to `src/lib.rs`).
- [x] Define `KeyCode(u16)` (Linux evdev code) and a **pure** `parse_keys(spec:
      &str) -> Result<Vec<KeyCode>>` that splits on `+`, trims, lower-cases
      names, maps each to its evdev code, and preserves order (modifiers as
      written). Reject empty spec and unknown names with `Error::ConfigInvalid`.
      Tests: single key (`f` -> `KEY_F`=33), combo (`ctrl+shift+f` -> ctrl,
      shift, f codes in order), case-insensitive, aliases (`ctrl`==`control`,
      `esc`==`escape`, `enter`==`return`), unknown name errors, empty errors.
- [x] Key-name table (default coverage set - A3): letters `a`-`z`, digits
      `0`-`9`, `f1`-`f12`, modifiers `ctrl`/`control`, `shift`, `alt`,
      `super`/`meta`/`win` (LEFTMETA), `altgr` (RIGHTALT); `space`, `enter`/
      `return`, `tab`, `esc`/`escape`, `backspace`, `delete`, `insert`, `home`,
      `end`, `pageup`, `pagedown`, arrows `up`/`down`/`left`/`right`, `minus`,
      `equal`, `comma`, `dot`/`period`, `slash`, `semicolon`. Keep the table a
      single source of truth (a `match` or a static slice), tested for a
      representative subset. Document how to extend it.
- [x] Define `trait KeyEmitter { fn key_down(&mut self, code: KeyCode) ->
      Result<()>; fn key_up(&mut self, code: KeyCode) -> Result<()>; fn
      release_all(&mut self) -> Result<()>; }` (the trait tracks or is told
      which are held; `release_all` is the safety valve).
- [x] `RecordingEmitter` (test-only, behind `#[cfg(test)]` or a small pub
      test util): records a `Vec<(&'static str, KeyCode)>` of down/up calls, so
      the runtime latch can be unit-tested without hardware.
- [x] `UinputKeyboard` (real): opens `/dev/uinput`, `UI_SET_EVBIT(EV_KEY)`,
      `UI_SET_KEYBIT` for every code the app can emit (the whole table, so any
      configured hold works), `UI_DEV_SETUP`+`UI_DEV_CREATE`, then emits
      `input_event { EV_KEY, code, value }` + `EV_SYN/SYN_REPORT` for down(1)/
      up(0). Include the settle delay after `UI_DEV_CREATE` (~200ms or poll) so
      the first event is not dropped. `Drop` calls `release_all` then
      `UI_DEV_DESTROY` + close (kernel also releases on fd close - belt and
      braces). Use `libc` ioctls in the `hid.rs` style; define the needed
      `UI_*` request constants and `input_event`/`uinput_setup` structs.
- [x] Gate the real-device test behind availability of a writable `/dev/uinput`
      (skip with a clear message otherwise) so CI without uinput still passes.
      This test: create device, emit a keydown+keyup, assert no error; it is a
      smoke test, not an assertion on downstream focus.
- [x] `cargo fmt` + `cargo clippy --all-targets -- -D warnings` + `cargo test`.
- [x] Commit.

## Phase 2 - Config surface: the `hold` field (TDD)

- [x] Add `hold: Option<String>` to `ButtonConfig` (serde: default,
      skip_serializing_if None) with a doc comment. Do NOT add it to
      `ButtonState` (D1).
- [x] Extend `validate_one()` / the action-count logic so `hold` counts as an
      action: a button may set **exactly one** of `run`/`builtin`/`macro`/
      `hold`, and `states` remains mutually exclusive with all of them. A `hold`
      button still needs a visual (image/color/label/watch) - reuse the existing
      "no image, color, label or watch" check. Validate the `hold` spec by
      calling `keyboard::parse_keys` and surfacing a config error on an unknown
      key so a typo fails at load, not silently at press time.
      Tests: valid `hold = "F"`; `hold` + `run` on the same key errors; `hold` +
      `states` errors; `hold` with no visual errors; `hold = "boguskey"` errors;
      `hold = ""` errors; TOML round-trips with `hold`.
- [x] `cargo fmt` + clippy + test. Commit.

## Phase 3 - Runtime: the hold latch + safe release (TDD)

- [x] Add `KeyAction::Hold(Vec<KeyCode>)` to `actions.rs` (store parsed codes,
      or the raw spec + parse in `action_map` - prefer parsing in `action_map`
      so the runtime holds codes). Update `runtime::action_map` to map a button
      with `hold` to `KeyAction::Hold`. Test: `action_map` yields `Hold` with
      the right codes.
- [x] Give `Session` an emitter (`Box<dyn KeyEmitter>` created lazily; store an
      `Option` and a constructor closure, or an enum `Uninit`/`Ready`) and a
      `held: HashMap<u8, Vec<KeyCode>>` (deck key -> codes currently held).
- [x] Latch logic in `poll()` for `KeyAction::Hold(codes)`:
      - if `held` does NOT contain the key: lazily init the emitter (on failure,
        log actionable error and return - press is a no-op); `key_down` each
        code in order; insert into `held`; mark the key so `render_key`/
        `effective_button` show the **persistent held highlight**.
      - if `held` DOES contain the key: `key_up` each code in reverse; remove
        from `held`; restore the normal visual.
      Log each transition (`key N hold on -> ctrl+shift+f`, `key N hold off`).
      Test the state machine with `RecordingEmitter`: press toggles down then
      up; two different keys are independent; unknown-key never reached (config
      validated). Assert modifier ordering (down forward, up reverse).
- [x] Press-feedback interaction: a latched key must stay visibly "on" and the
      release-flash restore in `poll()` must NOT clear a held key's highlight.
      Make `render_key`/`effective_button`/`render_current` held-aware so a
      re-render (page redraw, reload) keeps a held key highlighted. Add a pure
      test for the "held => highlighted" render decision if feasible.
- [x] Safety - release on shutdown: in `run_with_control`, after the loop (the
      existing `clear_all` path), call `emitter.release_all()` (if the emitter
      was created) so no key stays stuck. Test via `RecordingEmitter` that a
      held key gets a `key_up` on shutdown.
- [x] Safety - release on reload: `Session::reload()` (and `show_page` when the
      button set changes) must release any held key whose button no longer
      defines that exact hold; keep holds whose button/spec is unchanged.
      Test: reload dropping the hold button releases the key; reload keeping it
      does not double-press.
- [x] Reap/no-zombie and existing loop behaviour unaffected (holds do not spawn
      shell children, so no interaction with `reap_zombies`).
- [x] `cargo fmt` + clippy + full test. Commit.

## Phase 4 - Held-state visual polish (confirm A1 first if adding config)

- [x] Default: persistent `brighten(0.3)` while held (from Phase 3). Verify it
      reads clearly on the device against a normal press-flash. (Device reading
      confirmed in Phase 7 hardware verification.)
- [x] Optional (only if A1 says so): `active_color` / `active_label` on the
      button, rendered while held; validate + round-trip + test. If not doing
      it, ensure no half-added config remains. (A1 default shipped; no
      configurable form added, no dead config.)
- [x] Commit (skip if nothing changed beyond Phase 3). (Nothing changed.)

## Phase 5 - Web editor support

- [x] `assets/web/app.js`: add a **"Hold key"** action radio alongside run /
      builtin / macro / toggle / open-app; a text input for the spec
      (placeholder `e.g. ctrl+shift+f`); load it (`act = "hold"` when
      `b.hold`), collect it into the button on save, and show/hide the input
      with the other action inputs. Keep the mutual-exclusion (selecting Hold
      clears the other action fields on collect).
- [x] `assets/web/index.html` + `app.css`: the input element + label + any
      styling, matching the existing controls. Zero layout shift.
- [x] Editor writes `hold` into the TOML via the existing `post_state` ->
      `Config` -> validate -> write -> `Control::Reload` path; a bad spec
      returns the 400 from `validate` (already wired). Manually verify: set a
      key to Hold `f` in the editor, confirm the config file gets `hold = "f"`
      and the daemon reloads.
- [x] Verify headlessly (curl the editor, and/or a screenshot as prior phases
      did) that the Hold control renders and round-trips.
- [x] Commit.

## Phase 6 - Docs, examples, decisions

- [ ] `README.md`: document the `hold` field in the Button fields table and the
      "exactly one action" rule (add `hold` to the mutually-exclusive set);
      add a short "Hold a key (latch)" subsection with the `ctrl+shift+f`
      example and the note that it uses a virtual uinput keyboard (works on X11
      + Wayland, needs `/dev/uinput` access - give the udev rule for machines
      that lack the ACL: e.g. a `uinput` group + `KERNEL=="uinput",
      GROUP="uinput", MODE="0660"` rule, or `uaccess`).
- [ ] `examples/`: add a small example config (or extend `showcase.toml`) with a
      hold key, so `streamdeck run examples/<file>` demonstrates it.
- [ ] `DECISIONS.md`: record the uinput-over-xdotool decision (safety +
      portability + fit) and the native-autostart-over-PM2 decision.
- [ ] Update `README.md` Development section if not already done in Phase 0.
- [ ] Commit.

## Phase 7 - Verify on hardware + fold-back

- [ ] Full quality gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D
      warnings`, `cargo test` all green.
- [ ] Hardware verification on the MK.2 (device present on `/dev/hidraw0`):
      - Configure a key with `hold = "f"`. Press it: focus a text field / editor
        and confirm `f` autorepeats (held) until the second press releases it.
      - Configure `hold = "ctrl+shift+f"`; confirm the combo latches and
        releases.
      - Confirm the key shows the persistent held highlight while latched.
      - Kill the daemon while a key is held (`kill <pid>`) and confirm the key
        is released (no stuck key) - the uinput fd close must free it.
      - Confirm `autostart` starts the tray on login and the dev loop always
        runs the latest build.
- [ ] Record before/after value in `~/PR_RESULTS.md` (what the feature adds, any
      metrics/observations), max 8 lines.
- [ ] Fold-back: ensure module/file headers state what they ARE now (e.g.
      `keyboard.rs` documents the uinput virtual keyboard), delete any migration
      shims, and rewrite any comment citing "Phase N" into a domain statement.
      Tick every box only when the code is named for what it is.
- [ ] Open a PR (verify locally first; confirm the committed diff matches
      intent). Concise, bulleted description. No PM2/wiki cross-links.

---

## Definition of done

- Native autostart runs `streamdeck tray` on login; `scripts/dev.sh` always runs
  the latest build during development; PM2 is not involved.
- A `hold = "<key|combo>"` button latches the key(s) down on first press and
  releases on the second, via a pure-Rust uinput virtual keyboard, working on
  X11 and Wayland, with a persistent held-state highlight.
- A crash or shutdown never leaves a key stuck.
- Config validates the spec at load; the web editor can set a Hold key.
- Full `fmt`/`clippy -D warnings`/`test` green; hardware-verified on the MK.2;
  README/DECISIONS/examples updated; every checkbox ticked as its task landed,
  with a commit per task.
