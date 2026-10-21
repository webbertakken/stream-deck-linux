# Control API and CLI client

The running daemon (`streamdeck tray` or `streamdeck ui`) owns the device and the config file. Programs
read what the deck shows and change the layout through its **control API**, never by writing the file: the daemon validates each
change against the model, writes the file atomically, and reloads the device at once. The `streamdeck`
CLI's layout verbs are a client of that API, so the same binary edits the deck from a terminal, a script
or another program.

The API is versioned (`/api/v1`). The CLI and the daemon are built from the same source, so a given build
of the CLI always speaks its daemon's version.

## Where it listens

On the editor's server: `http://127.0.0.1:57617`, or an ephemeral loopback port when that one is taken
(the tray's log names it). Every request passes the editor guard first (loopback `Host`, no foreign
`Origin`, writes as `application/json`; see the README).

## Tokens

Every `/api/v1` request carries `Authorization: Bearer <token>`, except the two **open reads**,
`GET /api/v1/icons` and `GET /api/v1/style`: they answer this build's catalogue, the same for every
caller, change nothing, and so need no token (the web editor, which holds none, calls them). The
editor guard still stands before them.

- A token belongs to a named **caller** (`cli`, `runa`, ...): lowercase letters, digits and hyphens.
- Tokens live one per file in `$XDG_CONFIG_HOME/streamdeck/tokens/<caller>` (directory `0700`, files
  `0600`). The daemon reads the directory on every request, so revoking takes effect at once.
- A token is `sdk_<caller>_<32 hex digits>` from the kernel's random source.
- No file is ever printed: `streamdeck token issue <caller>` writes the caller's file, and with
  `--out <file>` writes the same token to that file too, so another program can be handed its own copy.

| Command                                         | Does                                                      |
| ----------------------------------------------- | --------------------------------------------------------- |
| `streamdeck token issue <caller> [--out <file>]` | Mints (or replaces) the caller's token.                   |
| `streamdeck token revoke <caller>`              | Deletes it; that caller is refused from the next request. |
| `streamdeck token list`                         | Names the callers that hold a token.                      |

Each authorised request is logged with its caller's name and how long it took; each refusal with its name.

## Revisions

A layout's **revision** is the FNV-1a 64-bit hash of the config file's bytes, as 16 hex digits. Every
answer carries the revision it saw or wrote. A write may send `If-Match: <revision>`: when the file
has changed since (the web editor, a hand edit, another caller), the write is refused as
`stale-revision` and nothing changes. A write without `If-Match` applies to the file as it stands.

## Routes

`{page}` is a page's 0-based index or its name; `{key}` is a hardware key index.

| Route                                | Body                         | Answers                                                    |
| ------------------------------------ | ---------------------------- | ---------------------------------------------------------- |
| `GET /api/v1/layout`                 |                              | `{ revision, model, brightness, press_feedback, pages }`   |
| `GET /api/v1/schema`                 |                              | `{ api, fields, rules, builtins, hold_keys }`              |
| `GET /api/v1/view`                   |                              | `{ page, pages, model, brightness, standby, keys }`: what the device shows now |
| `GET /api/v1/icons?q=<words>`        |                              | `{ total, icons }`: Lucide icons a key may show, best first; no token |
| `GET /api/v1/style`                  |                              | `{ palette, labels, icons, layout }`: the deck's design language; no token |
| `PUT /api/v1/pages/{page}/keys/{key}` | a button, without `key`      | `{ revision }`: the key is replaced whole                  |
| `DELETE /api/v1/pages/{page}/keys/{key}` |                          | `{ revision }`: the key is blank; already blank is fine    |
| `POST /api/v1/pages`                 | `{ "name": "…" }`            | `{ revision, page }`: appended, `page` its index           |
| `DELETE /api/v1/pages/{page}`        |                              | `{ revision }`                                             |
| `PUT /api/v1/brightness`             | `{ "value": 0-100 }`         | `{ revision }`: stored in the layout and applied           |

- `pages` in a layout is always the normalised list: a single-page config reads as one unnamed page.
  Adding a page to it turns it into a two-page config whose first page is unnamed.
- A button is the same object as one `[[buttons]]` entry in the TOML (README, *Config reference*).
  Unknown fields are refused, so a caller learns of a typo instead of losing it.
- `schema` describes the build that answers: `fields` names every button field with its meaning,
  `rules` what every key must meet, `builtins` every built-in action (patterns such as `page:<name|index>` included), `hold_keys` every key
  name a `hold` accepts. A caller that reads it needs no change when the deck gains a field. Tests
  fail when a button field or a built-in is added without being described there (`src/schema.rs`).
- `view` is described under *The live view* below; `icons` and `style` under *Icons* and *Style*.
- A key's `icon` names a Lucide icon. A key with both `image` and `icon` is refused as `invalid`
  (`key 4 sets both image and icon; a key shows one picture`); so is an unknown name, with up to
  three close names (`unknown icon 'mial'; did you mean mail?`).
- A write re-serialises the whole config, so comments in a hand-written file do not survive it (the
  web editor's saves behave the same).

## The live view

`GET /api/v1/view` answers what the device shows at this moment, as the daemon last drew it: the
current page, toggle states, live (`watch`) labels and press or hold highlights included.

```json
{
  "page": { "index": 0, "name": null },
  "pages": 1,
  "model": { "name": "Stream Deck MK.2", "columns": 5, "rows": 3, "keyCount": 15 },
  "brightness": 40,
  "standby": false,
  "keys": [{ "key": 0, "png": "<base64 PNG>", "held": false }]
}
```

- `page` is the page on show, `name` `null` when it has none; `pages` counts the layout's pages.
- `brightness` is the percentage the daemon last set; `null` while it has set none (a layout without
  `brightness` leaves the device at its own).
- `keys` holds every key of the model, in index order. `png` is the key image exactly as sent to the
  device, upright (before the device's rotation), at the model's key size (72x72 on the MK.2).
  A blank key is a black image.
- `held` is `true` while the key's `hold` latch is on (its keyboard keys are held down).
- After a reset the device shows its standby logo: every key reads as black and `standby` is `true`
  until the daemon next redraws the whole page (a reload or a page switch). A key drawn in between
  shows its image while the rest stay on the logo.
- Before the daemon has drawn the deck once, the route answers `503 not-ready`.
- The view is read, never stored: polling it changes nothing. An answer takes well under a
  millisecond to encode on a release build (`cargo run --release --example view_timing`).

## Icons

`GET /api/v1/icons?q=<words>` searches the [Lucide](https://lucide.dev) icons embedded in this build
(the version is in `assets/lucide/VERSION`):

```json
{
  "total": 23,
  "icons": [
    { "name": "mail", "tags": ["email", "message", "letter", "unread"] },
    { "name": "mail-x", "tags": ["email", "message", "letter", "remove", "delete"] }
  ]
}
```

- `total` counts every icon that matches; `icons` holds at most 40 of them, best first.
- An icon matches when every word is in its name or one of Lucide's tags for it (any case). Name
  matches rank first: the name itself (`arrow up` finds `arrow-up` first), then names made of the
  words, then names holding them, then tag matches; shorter names first within each.
- Without words (`q` absent or empty) it lists the first 40 icons by name, and `total` counts them all.
- `q` is a query value: words separated by spaces, sent as `+` or `%20`.

## Style

`GET /api/v1/style` answers the deck's design language, the same rules [`style.md`](style.md) writes out:

```json
{
  "palette": [{ "name": "apps", "hex": "#2E5A88", "for": "launching programs" }],
  "labels": ["One or two words, in title case."],
  "icons": ["Every key gets an icon when one fits."],
  "layout": ["Related keys sit together in a row."]
}
```

- `palette` names each background colour, its `#RRGGBB` and what it is for; every one carries the
  white icon and label at WCAG 2.2 AA contrast.
- `labels`, `icons` and `layout` are rules in words, for a program that makes keys to follow.

## Refusals

A refusal answers `{ "error": "<name>", "detail": "<why>" }`.

| Name             | Status | When                                                              |
| ---------------- | ------ | ----------------------------------------------------------------- |
| `foreign-host`   | 403    | `Host` is not this server on loopback                             |
| `foreign-origin` | 403    | An `Origin` other than the editor's own                           |
| `not-json`       | 415    | A write not sent as `application/json`                            |
| `no-token`       | 401    | No `Authorization: Bearer` header                                 |
| `bad-token`      | 401    | A token no caller holds                                           |
| `bad-json`       | 400    | A body that does not parse as the route's object                  |
| `invalid`        | 422    | The layout the change would make breaks a rule (an image with an icon, an unknown icon, ...); `detail` says which |
| `no-such-page`   | 404    | `{page}` names no page                                            |
| `last-page`      | 409    | Removing the only page                                            |
| `stale-revision` | 409    | `If-Match` names a revision the file no longer has                |
| `unreadable`     | 500    | The config on disk does not parse; fix it by hand or in the editor |
| `write-failed`   | 500    | The config could not be written; the old file is untouched        |
| `not-ready`      | 503    | The daemon has not drawn the deck yet; ask again shortly          |
| `not-found`      | 404    | No such route                                                     |

## The CLI client

```bash
streamdeck layout                            # the layout, as JSON
streamdeck schema                            # what a button may hold, as JSON
streamdeck view                              # what the deck shows now, as JSON
streamdeck icons [words]                     # Lucide icons a key may show, best first, as JSON
streamdeck style                             # the palette and the label, icon and layout rules, as JSON
streamdeck key set <page> <key> '<json>'     # replace a key ("-" reads the JSON from stdin)
streamdeck key clear <page> <key>            # blank a key
streamdeck page add <name>                   # append a page
streamdeck page remove <page>                # remove a page
streamdeck layout brightness <0-100>         # store and apply brightness
```

- Every write takes `--revision <rev>` (sent as `If-Match`).
- The daemon is found at `--url`, else `$STREAMDECK_URL`, else `http://127.0.0.1:57617`.
- The token is `$STREAMDECK_TOKEN`, else the file `$STREAMDECK_TOKEN_FILE`, else the `cli` caller's file.
  Issue that one once with `streamdeck token issue cli`. `icons` and `style` work without one.
- Success prints the daemon's JSON answer on stdout and exits `0`.
- A refusal prints the daemon's `{ error, detail }` on stderr and exits `2`.
- A failure before the daemon answered prints `{ error, detail }` on stderr and exits `1`, with
  `unreachable` (nothing answers at the URL), `no-token` (no token found locally) or `usage`.
