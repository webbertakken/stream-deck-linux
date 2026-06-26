# Stream Deck: more built-ins, fuzzy app search, + 5 features

Expand the custom Stream Deck software. Each task: implement idiomatically with
tests, verify, tick, move on. Verify on the real MK.2 at the end of each phase.

Chosen 5 features (most useful, my pick): multi-page layouts, macro keys,
live (auto-refreshing) keys, toggle/multi-state keys, press visual feedback.

## Phase A - More built-in actions (self-contained, TDD)
- [x] `system` module: pure builders for open / media / volume commands with
      runtime tool detection (xdg-open, playerctl, wpctl/pactl/amixer)
- [x] `Builtin` enum + parser: `brightness_max`, `brightness_min`,
      `open:<target>`, `clear`, `media_play_pause`, `media_next`, `media_prev`,
      `volume_up`, `volume_down`, `volume_mute` (+ tests)
- [x] Runtime dispatch for the new built-ins (deck vs spawn-command)
- [x] Editor: built-in dropdown lists all of them (+ open target input)

## Phase B - Fuzzy app search (editor)
- [x] Replace app `<select>` with a searchable combobox: text filter + fuzzy
      match + keyboard nav; auto-fills label/icon on pick
- [x] Verify via headless-Chrome screenshot

## Phase C - Multi-page layouts
- [x] Config: `[[pages]]` (name + buttons); back-compat with top-level
      `buttons`; normalisation + validation (+ tests)
- [x] Page-nav built-ins: `page_next`, `page_prev`, `page:<name|index>`
- [x] Runtime: page-aware model (current page, render page, switch + re-render)
- [x] Editor: page tabs (switch / add / rename / delete)

## Phase D - Macro keys (run a sequence)
- [x] Config: `macro = ["cmd1", "cmd2"]`; mutually exclusive w/ run/builtin
- [x] Runtime: run commands sequentially on press (+ tests)
- [x] Editor: macro field (multi-line)

## Phase E - Live (auto-refreshing) keys
- [x] Config: `watch = "<command>"` + `interval = <secs>`; stdout -> label
- [x] Runtime: periodic refresh re-renders the key label (+ tests on the
      output->label transform)
- [x] Editor: watch + interval fields

## Phase F - Toggle / multi-state keys
- [ ] Config: `states = [{ label, color, image, text_color, run|builtin }]`
- [ ] Runtime: per-key state index, cycle on press, render+run current state
      (+ tests)
- [ ] Editor: states editor (add / remove)

## Phase G - Press visual feedback
- [ ] Runtime: briefly highlight the pressed key, restore on release (pure
      transform tested); config opt-out `press_feedback = false`

## Phase H - Verify + docs
- [ ] fmt + clippy -D warnings + full test suite green
- [ ] Hardware verification on the MK.2 (each feature)
- [ ] README/docs updated
- [ ] Commit per phase
