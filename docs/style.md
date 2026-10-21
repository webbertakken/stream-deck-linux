# Style

How a key on this deck looks, so that every key, whoever made it, reads as part of one deck. Programs
read the same rules as data: `GET /api/v1/style` or `streamdeck style` (see
[`control-api.md`](control-api.md)). The data lives in `src/style.rs`; a test fails when this page and
that data disagree.

A key is a dark palette colour with a white [Lucide](https://lucide.dev) icon and, under it, a short
white label. The icon carries the meaning at a glance; the label confirms it.

## Palette

Pick the background by what the key is for. Each colour carries white icons and labels above WCAG 2.2
AA contrast for text (4.5:1); a test proves it for every colour.

| Name | Hex | For | Contrast with white |
| ---- | --- | --- | ------------------- |
| `apps` | `#2E5A88` | launching programs | 7.16:1 |
| `media` | `#6B3FA0` | playback and volume | 7.38:1 |
| `system` | `#3A3F4B` | deck, screens, power, brightness | 10.54:1 |
| `home` | `#2F7D4F` | lights, devices, the house | 5.04:1 |
| `runa` | `#7A4E0A` | anything that asks Runa or her plugins | 7.19:1 |
| `alert` | `#A33A3A` | stop, mute, anything to press with care | 6.51:1 |

## Labels

- One or two words, in title case.
- Say what pressing the key does.
- At most 10 characters a line. A program setting a key through the control API (`streamdeck key set`) is refused as `invalid` for a longer line; the editor and hand edits are not.
- White on the key's palette colour: leave text_color unset.

## Icons

- Every key gets an icon when one fits.
- One icon per key.
- The icon and the label say the same thing.
- White, like the label.

Find an icon by what it shows: `streamdeck icons <words>` (or `GET /api/v1/icons?q=<words>`) ranks
Lucide's icons by name, then by Lucide's own tags. With a label, the icon sits above it and the label
is drawn smaller; without one, the icon fills more of the key.

## Layout

- Related keys sit together in a row.
- Page navigation keys sit on the bottom row's ends.

## Example

A launcher and a mute key, in this style:

```toml
[[buttons]]
key = 0
icon = "mail"
label = "Mail"
color = "#2E5A88"     # apps
run = "thunderbird"

[[buttons]]
key = 4
icon = "volume-x"
label = "Mute"
color = "#A33A3A"     # alert
builtin = "volume_mute"
```
