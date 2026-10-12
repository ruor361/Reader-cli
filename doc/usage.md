# Usage reference

[Back to README](../README.md)

Complete the [first-run bookshelf setup](../README.md#quick-start) before
importing or opening a file. Quote paths, titles, labels, and queries containing
spaces. Commands below use an installed `reader`; from source, replace it with
`cargo run --` (see [Development](development.md)).

## CLI commands

```text
reader --help
reader <file>
reader import <file> [--open]
reader add <file> [--open]
reader list
reader shelf
reader network
reader network configure <url> [--username <name>] [--ca-certificate <file>]
reader network off
reader network list
reader open <book-id-or-title>
reader resume
reader remove <book-id-or-title> [--delete-file]
reader search <book-id-or-title> <query>
reader bookmark add <book-id-or-title> <label>
reader bookmark list <book-id-or-title>
reader bookmark goto <book-id-or-title> <bookmark-id>
reader bookmark delete <book-id-or-title> <bookmark-id>
reader refresh [directory]
reader shelf refresh [directory]
reader export [book-id-or-title] [--format json|csv] [--output <file>]
```

`add` aliases `import`; `shelf refresh` is the older spelling of `refresh`.
`import` opens the book only with `--open`; a direct file open imports and then
starts reading. `resume` opens the most recently recorded book. Bare `reader`
requires a file or subcommand; use `reader shelf` for the interactive home.

Book selection accepts an exact ID, a unique ID prefix, or a case-insensitive
title substring. Ambiguous selections fail. Bookmark IDs accept unique prefixes.
`reader list` prints the first 12 characters of each book ID.

CLI search prints up to 25 matches with section, percentage, and preview.
`bookmark add` uses the book's saved position. `bookmark goto` updates the
position and opens the reader.

`remove` removes the local library entry and its sidecar. `--delete-file` also
requests removal of the book file, subject to the managed-folder checks in
`src/main.rs`. A later refresh can re-add a book file left in the bookshelf.

## Reader controls

| Key | Action |
| --- | --- |
| `j` / `Down` / `Space` / `PageDown` | Next screen |
| `k` / `Up` / `PageUp` | Previous screen |
| `g` | Go to a percentage |
| `Home` | Go to the beginning |
| `End` / `G` | Go to the end |
| `t` | Browse the table of contents |
| `/` | Search within the book |
| `n` / `N` | Next / previous search result |
| `b` | Add a bookmark at the current position |
| `M` | Show book information |
| `s` | Show or hide reading progress |
| `p` | Toggle Discreet Mode |
| `F12` | Hide behind diagnostics / return |
| `?` / `F1` | Show available shortcuts |
| `q` / `Ctrl+C` | Save and quit |

A screen is the text visible at the current terminal size. Saved progress and
bookmarks use section-relative Unicode character positions, so resizing the
terminal or changing the text width does not change the saved location.

Discreet Mode displays redacted diagnostic decoration and provides a reduced
set of controls. Hidden mode displays diagnostics; press `F12` to return. Use
in-app help to see the controls available in the current mode.

## Shelf controls

The shelf shows title, author, progress, last-opened time, bookmark count, and
missing-file status. After a launched reader quits, you return to the shelf.

| Key | Action |
| --- | --- |
| `j` / `k` / arrows | Move the selection |
| `g` / `G` | Go to the first / last book |
| `PageDown` / `PageUp` | Move by 10 books |
| `Enter` | Open the selected book |
| `r` | Resume the most recently opened book |
| `i` | Import a book by pasting or typing its path |
| `/` | Filter by title or author |
| `s` | Sort by recent activity, title, or progress |
| `w` | Switch to the network bookshelf |
| `d` | Start removal of the selected book with confirmation |
| `?` / `F1` | Show shelf help |
| `q` / `Esc` / `Ctrl+C` | Quit |

## Managed bookshelf

Pass an existing folder to `reader refresh <directory>` once to save
`bookshelf_dir`. Later `reader refresh` uses that folder. In the shelf, `i`
guides you through setup if no folder is configured.

Imports validate the source, deduplicate by content, and copy into the managed
folder while preserving the original. A sibling sidecar uses the full filename
plus `.json`: `book.epub` has `book.epub.json`. It contains portable metadata,
progress, content metrics, and bookmarks, without machine-local absolute paths
or missing-file state.

Refresh recursively reconciles supported files, sidecars, and the local
library. It restores state from sidecars when a local entry is absent, marks
missing files, recognizes restored files, and removes orphan book sidecars
that have neither a book file nor a retained library entry.

## Configuration and data paths

Default paths are resolved in `src/paths.rs`:

1. If the `HOME` environment variable is set and `HOME/.config` already exists,
   Reader uses `HOME/.config/reader/` on any platform.
2. Otherwise it uses the platform directory from `dirs::config_dir()` plus
   `reader`, typically `~/.config/reader` on Linux,
   `~/Library/Application Support/reader` on macOS, or `%APPDATA%\reader` on Windows.

| Location relative to the config directory | Purpose |
| --- | --- |
| `configuration.json` | Display settings and bookshelf configuration |
| `library.json` | Machine-local library, paths, progress, and bookmarks |
| `sync/manifest.json` | Reading-journal schema version |
| `sync/reading-events/<book-id>/<event-id>.json` | Reading-session records |
| `sync/bookmark-ops/<book-id>/<operation-id>.json` | Bookmark add/delete operations |

Common settings (a partial example; omitted fields use defaults):

```json
{
  "show_progress": true,
  "discreet_mode": false,
  "mouse_support": false,
  "text_width": null,
  "bookshelf_dir": null
}
```

Set `text_width` to a terminal column width to center a narrower reading area,
or leave it `null` to use the available width. Colors inherit the terminal's
foreground, background, and ANSI palette. Use `refresh <directory>` to configure
the bookshelf rather than leaving `bookshelf_dir` at `null`.

Legacy `privacy_mode` input is accepted as `discreet_mode`; when both are present,
`discreet_mode` takes precedence. The removed `theme` key is ignored. Configuration
and library writes use atomic replacement.

## Folder synchronization

Reader mirrors the reading and bookmark journals from the local `sync/` directory
to `<bookshelf_dir>/.reader/` using the same structure. Configure an external
folder-sync tool to synchronize the bookshelf, including its hidden `.reader`
folder, book files, and sidecars. This synchronization uses filesystem access;
the separate network bookshelf is described below.

Reading events contain `book_id`, UTC `started_at`/`ended_at`, and stable
`start_position`/`end_position`. Active sessions checkpoint every 60 seconds and
become idle after 15 minutes without activity. A session's event file is updated
as reading continues; bookmark operations are separate UUID-named records.

Reconciliation merges journals, restores the latest reading position by event
time, and materializes bookmarks into the local library. A deletion wins over an
addition for the same bookmark ID. Mirror failures are reported as warnings;
local saves can still succeed. Run `reader refresh` after book files have been
added, removed, restored, or synchronized on another machine.

## Export

```sh
reader export --output events.json
reader export "Book title" --format csv --output events.csv
```

The default format is JSON; omit `--output` to write to stdout. Exports contain
raw reading events, optionally filtered to one book. JSON contains an array of
the five event fields. CSV uses these columns:

```text
book_id,started_at,ended_at,start_section_index,start_char_offset,end_section_index,end_char_offset
```

Exports represent recorded sessions rather than an aggregated reading report.

## Network bookshelf (Calibre-Web / Calibre-Web Automated)

Configure a server once, then switch between local and network books with `w`:

```sh
reader network configure https://books.example --username YOUR_USERNAME
reader shelf
```

The address may be the website root or its `/opds` endpoint, including a reverse
proxy subpath. The server must enable OPDS and grant the account download access.
When authentication is required, Reader asks for the password with hidden input
before entering the network view, unless credentials were supplied through
environment variables or a local `.env`. It stores the configured address,
username, and view switch; it never copies the password into `configuration.json`.
The switch is remembered for the next launch. Press `w` in the network view to
return to local books, or run `reader network off`.

In the network view, `/` searches the server catalog by title or author, `n` / `p`
change catalog pages, `R` reloads the current page, and `Enter` downloads and opens
the selected book. Requests run in the background; `w` and `q` remain available
while a request is pending. Downloads are validated and imported into the existing
local `bookshelf_dir` (set with `reader refresh <DIR>`). Local reading progress and
bookmarks keep working after download, including offline reading. Reader accesses
the remote server through GET requests and does not synchronize Calibre-Web's
reading position or edit/delete its books.

EPUB is preferred, followed by TXT and PDF. Books offered only as MOBI/AZW3 remain
visible with an `Unsupported` label and need conversion in Calibre. Scanned PDFs
still require OCR. Download size is limited to 256 MiB.

For HTTPS with a private CA, provide a trusted PEM root certificate:

```sh
reader network configure https://books.example --ca-certificate /path/to/root.pem
```

Reader validates HTTPS certificates and sends login credentials only over HTTPS.
OPDS links and redirects must stay on the configured server origin.
For temporary overrides, use `READER_NETWORK_URL`, `READER_NETWORK_USERNAME`,
`READER_NETWORK_PASSWORD`, and `READER_NETWORK_CA_CERT`. Password overrides are
optional; interactive hidden input avoids placing a password in shell history.
`reader network` starts directly in the network view;
`reader network list` prints all catalog pages for scripting.

Reader loads `.env` from the current working directory, falling back to the
executable's directory. Use `READER_ENV_FILE` to select a file explicitly.
For an installed executable, save a fixed path once so it works from any directory:

```sh
reader network configure https://books.example --env-file /absolute/path/to/.env
```

The explicit `READER_ENV_FILE` override takes precedence over this saved path;
without either, Reader checks the working directory and executable directory.
The file can use standard `READER_NETWORK_*=VALUE` lines or a JSON object:

```json
{
  "username": "your-username",
  "password": "your-password"
}
```

JSON also accepts `url` and `ca_certificate`, or the equivalent
`READER_NETWORK_*` names. A final trailing comma is accepted. Process environment
variables take precedence over `.env`, which takes precedence over ordinary
connection configuration. `.env` and `.env.*` are ignored by Git; `.env.example`
contains placeholders only. Reader does not print file contents or passwords.

Libraries from older local builds may contain `Mobi` records. Those records and
their reading data are preserved; opening MOBI files still requires conversion.
