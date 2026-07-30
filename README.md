# Reader

`reader` is a local-first command line reading tool for EPUB, TXT, and
text-based PDF books.

## What works

- Open a local book directly: `reader book.epub`
- Add/list/open/remove books in a local bookshelf
- Interactive TUI bookshelf: `reader shelf` for browsing, searching, sorting, opening
- Resume the most recently read book
- Full-screen terminal reading UI with reflow-stable progress saving
- EPUB/TXT/PDF parsing (EPUB pulls author from OPF metadata; TXT infers from `Title - Author.txt` or `Title (Author).txt` filename patterns; PDF uses document metadata and extracts text page by page)
- Percentage jump and searchable hierarchical contents navigation (EPUB 3 nav, EPUB 2 NCX, and PDF outlines, with clearly labeled generated fallbacks)
- Metadata view
- Display configuration
- Discreet Mode / 低暴露模式
- Bookmarks
- Book search
- Automatic reading-session history with 15-minute idle splitting
- Folder-based cross-device progress and bookmark synchronization
- JSON/CSV reading-event export

Reader stores its state in the user's config directory. When a
`bookshelf_dir` is configured, each book in the bookshelf gets a
sibling `.json` sidecar — see [Bookshelf](#bookshelf) below.

## Commands

```powershell
reader <file>
reader add <file>
reader list
reader shelf
reader refresh [directory]
reader shelf refresh [directory]
reader open <book-id-or-title>
reader resume
reader remove <book-id-or-title> [--delete-file]
reader bookmark add <book-id-or-title> <label>
reader bookmark list <book-id-or-title>
reader bookmark goto <book-id-or-title> <bookmark-id>
reader bookmark delete <book-id-or-title> <bookmark-id>
reader search <book-id-or-title> <query>
reader export [book-id-or-title] [--format json|csv] [--output <file>]
```

Book and UUID bookmark identifiers accept a unique prefix. Reader displays the
first 12 characters while retaining the complete identifier internally.

## Development

`rust-toolchain.toml` selects the host-native stable Rust toolchain, so Linux,
macOS, Windows MSVC, and Windows GNU builds are not forced to cross-compile.
On Windows without Visual Studio Build Tools, install the GNU toolchain plus a
MinGW-w64 `gcc`/`ar` pair and run Cargo with
`+stable-x86_64-pc-windows-gnu`. Exact setup commands and the verified recovery
procedure for interrupted rustup downloads are documented in
[Windows build setup](doc/Windows构建环境.md).

```powershell
cargo run -- --help
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Shelf

`reader shelf` opens an interactive bookshelf that lists every book in the
library with title, author, reading progress (with bar and percent), last opened
time, and bookmark count. Pressing `Enter` on a row opens that book; quitting
the reader returns to the shelf. The `*` marker flags the most recently opened
book.

| Key | Action |
| --- | --- |
| `j` / `k` / `Up` / `Down` | Move selection (wraps) |
| `g` / `G` | First / last row |
| `PageDown` / `PageUp` | Jump 10 rows |
| `Enter` | Open the selected book in the reader |
| `r` | Resume the most recently opened book |
| `/` | Filter by title or author (substring, case-insensitive) |
| `s` | Cycle sort: recent / title / progress |
| `d` | Delete the selected book (confirms with `y` / `n`) |
| `?` / `F1` | Open complete shelf help |
| `q` / `Esc` / `Ctrl+C` | Quit |

`reader list` prints the same data as a plain-text table for scripts and
non-interactive shells, including whether a record is `ok` or `missing`.

## Reader Keys

The reader always keeps a status row and a context-sensitive action row visible.
The action row and the full `?` help panel are generated from the same binding
registry. `Esc` closes or cancels a child panel and does nothing at the reading
root. `q` or `Ctrl+C` quits and saves.

| Mode | Key | Action |
| --- | --- | --- |
| Reading | `q` / `Ctrl+C` | Quit and save progress |
| Reading | `j` / `Down` / `PageDown` / `Space` | Next screen |
| Reading | `k` / `Up` / `PageUp` | Previous screen |
| Reading | `g` | Jump to a whole-book percentage (`0`–`100`, one decimal) |
| Reading | `Home` / `End` / `G` | Jump to 0% / 100% |
| Reading | `t` | Open the contents navigator |
| Reading | `M` | Open book info/metadata |
| Reading | `/` | Open search prompt |
| Reading | `s` | Show or hide progress |
| Reading | `p` | Toggle Discreet Mode |
| Any reader screen | `F12` | Show/restore the full-screen simulated JS diagnostic |
| Reading | `b` | Add a bookmark at the current text position |
| Reading | `n` / `N` | Next / previous search result |
| Reading | `?` / `F1` | Open complete contextual help |
| Contents | `j/k`, arrows, PageUp/PageDown | Move selection |
| Contents | digits + `Enter` | Jump directly to a numbered contents entry |
| Contents | `/`, text, arrows, `Enter` | Filter content titles and jump |
| Contents | `Esc` | Clear an active filter, then close |
| Metadata | `Esc` / `Enter` / `M` | Close and return |
| Search | text | Type query |
| Search | `Backspace` | Delete character |
| Search | `Enter` | Run search and return |
| Search | `Esc` | Cancel and return |

"Page" in the movement rows means one current terminal screen, not a stored
book page number. Persistent progress, bookmarks, search results, and shelf
percentages use a Unicode character position within a content section, so changing the
window size, text width, or Discreet Mode layout does not move the saved place.

## Configuration

Reader stores display settings in `configuration.json` and the book
library in `library.json`, both under the same directory.

- Linux and macOS: `~/.config/reader/`
- Windows / systems without `~/.config`: the platform's
  `dirs::config_dir()` (e.g. `%APPDATA%\reader` on Windows)

Supported settings:

```json
{
  "show_progress": true,
  "discreet_mode": false,
  "mouse_support": false,
  "text_width": null
}
```

The TUI inherits the terminal's default foreground and background instead of
painting a dark or light theme. Accents and status colors use the terminal's
ANSI palette, so they follow the terminal profile as well. Existing `theme`
keys are accepted for compatibility, ignored, and omitted on the next save.

The TUI can choose colors and layout but cannot change the terminal emulator's
font size. For sustained reading in Windows Terminal, use Cascadia Mono at
14pt and normal weight; 16pt is a reasonable comfort option on high-DPI screens
or for longer sessions. Windows Terminal defaults to 12pt, while a controlled
continuous-screen-reading study comparing 10/12/14pt recommended 14pt. Example
profile setting:

```json
"font": {
  "face": "Cascadia Mono",
  "size": 14,
  "weight": "normal"
}
```

References: [Windows Terminal appearance settings](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/profile-appearance),
[screen-reading font-size study](https://pubmed.ncbi.nlm.nih.gov/25665207/).

### Bookshelf

By default, `reader add <file>` only registers the book in the
library. Set `bookshelf_dir` in `configuration.json` to make that
directory the actively managed bookshelf. Added books and direct
opens (`reader <file>`) are copied there first, then opened or
registered from the bookshelf copy:

```json
{
  "bookshelf_dir": "/Users/me/Books/reader"
}
```

If a file with the same name already exists in the bookshelf and is
already registered, Reader reuses that existing record instead of
creating another copy. If the name collides with an unregistered file,
the copy is renamed with a ` (1)`, ` (2)`, ... suffix before the
extension.

#### Sidecar `.json`

Every book in the bookshelf gets a versioned sibling `.json` sidecar containing
portable metadata and a recovery snapshot (id, title, author, stable text
progress, content metrics, last opened, and bookmarks). Machine-local absolute
paths and `missing` state stay in `library.json`. For `Pride and Prejudice.epub`
the sidecar is `Pride and Prejudice.epub.json`. Sidecars are written
on `reader add`, `reader shelf refresh`, and normal state changes such
as reading progress and bookmark updates.

#### Reading data synchronization

Reader always journals reading data under the local config directory. When
`bookshelf_dir` is configured, the journal is bidirectionally mirrored into
`<bookshelf_dir>/.reader/`, which a normal OneDrive, Dropbox, iCloud, Syncthing,
or similar desktop client can synchronize:

- `reading-events/<book-id>/<session-uuid>.json` stores five-field reading
  events with UTC RFC 3339 timestamps and stable text positions.
- `bookmark-ops/<book-id>/<operation-uuid>.json` stores additive bookmark
  changes and deletion tombstones.

Each platform configures its own local cloud mount path. Reader does not log in
to a cloud provider or upload data over a provider API. Book IDs are full
SHA-256 fingerprints of file bytes, so the same synchronized file keeps the
same identity even when its absolute path differs.

A reading session ends on quit, on entering the F12 hidden screen, or after 15
minutes without input. Active sessions are atomically checkpointed every minute
and whenever progress changes. A cloud-mirror failure leaves the local journal
intact and is retried on a later startup, state change, or refresh.

Raw events can be sent to another program or saved atomically:

```powershell
reader export
reader export stoner --format csv --output reading.csv
```

JSON preserves the exact five-field event model. CSV expands each text position
into section-index and character-offset columns.

#### `reader refresh` / `reader shelf refresh`

`reader refresh [directory]` recursively reconciles three sources. The older
`reader shelf refresh [directory]` spelling remains supported:

1. the on-disk bookshelf directory (`.epub`/`.txt`/`.pdf` files),
2. the per-book `.json` sidecars, and
3. the central `library.json`.

Rules:

- A book file on disk + no library entry -> added to the library, sidecar written.
- A book file on disk + library entry present -> metadata/path refreshed, reading state kept, sidecar written.
- A library entry under the bookshelf whose file is gone -> kept in the library as `missing: true`.
- A missing book whose file reappears -> restored to `missing: false`.
- A sidecar with no matching book file and no library record -> removed as an orphan.

Pass a directory once to select and persist the managed bookshelf. Later
refreshes can omit it and reuse `bookshelf_dir` from `configuration.json`:

```powershell
reader refresh .\Liberal
reader refresh
```

If no directory has ever been selected, `reader shelf refresh` reports the
exact command needed instead of requiring a manual JSON edit.
If the configured directory has disappeared, is not a directory, or cannot be
read, refresh fails with the affected path instead of reporting a zero-count
success.
Missing books remain visible in `reader list` and `reader shelf`, but cannot be
opened until the file is restored and `reader shelf refresh` is run again.
`reader remove <book>` removes only the library record and sidecar. Use
`reader remove <book> --delete-file` to also delete the actual bookshelf file.

`text_width` may be set to a terminal column width, or left as `null` to use
the available terminal width. Reader centers the configured width and renders
it with the terminal's configured colors.

### Storage migration

`library.json` and bookshelf sidecars use schema version 3. Library,
configuration, and sidecar updates are written to a same-directory temporary
file and atomically replaced only after the complete file has been flushed, so
an interrupted write preserves the previous valid state. On first load of an
older record, Reader converts its page-based position into a stable text
position. If the record contains a page count, the old page ratio is mapped
across the book's Unicode character count. Otherwise Reader keeps the old
content section and starts at that section's beginning. Bookmark labels and library
metadata are preserved. Version 2 path-derived IDs are replaced by content
fingerprints when a book is next opened or refreshed, while progress, bookmarks,
and the resume reference are retained; the next normal save writes the version
3 shape.

### Discreet Mode / 低暴露模式

When `discreet_mode` is enabled with `p`, Reader shows a neutral simulated
JavaScript diagnostic above an adaptive 4–8 line reading area
(when the terminal has at least four rows). For Chinese long-form reading, the
layout treats 48 terminal columns—about 24 full-width characters—as the
engineering floor and defaults to the 64-column upper bound—about 32
characters. Configured widths are clamped to 48–64 columns. Narrower terminals
preserve a minimal level prefix when possible and give the remaining width to
the body. The reading block is left-aligned with one terminal column of padding
rather than centered.

Every non-empty physical body line is formatted as a stable terminal log; blank
paragraph rows remain blank. Levels use `INFO/WARN/ERROR = 80%/15%/5%`. Full
timestamps are used when the terminal can preserve the requested body width,
then prefixes degrade to compact and level-only forms. Log timestamps and
levels are generated when a page is entered and remain fixed while that page is
redrawn.

Discreet Mode also keeps the terminal's default foreground and background.
Timestamps, the row status, and context-free notifications use the terminal's
muted ANSI color; only the log-level keyword uses the terminal's green, yellow,
or red ANSI color. The bottom row contains only a muted physical-row range such
as `rows 128-133/980`.

Only direct navigation, start/end, bookmark, quit, `p`, and `F12` are
accepted; commands that require metadata or input overlays are rejected with a
context-free notice. Book titles, metadata, search text, progress, real paths,
and contextual status messages stay hidden.

Discreet Mode reduces casual visual exposure; it is not encryption, access
control, screen-capture prevention, or an operating-system privacy boundary.
Existing configurations containing the legacy `privacy_mode` key are migrated
on read. If both keys exist, `discreet_mode` wins, and the next save writes only
the new key.

`F12` is a separate transient hide control available from every reader screen,
including open prompts. It replaces the entire terminal with a fixed neutral JS
stack and accepts only `F12` to restore or `Ctrl+C` to save and quit. It never
changes progress, configuration, overlays, or pending input. Discreet and hide
rendering only affect the TUI—real CLI errors and panics are always printed in
full so they remain diagnosable.

## Not Yet Supported

PDF support is text-only and reflows extracted content in the terminal.
PDF outline destinations jump to the beginning of their target physical page.
Scanned/image-only PDFs, OCR, password prompts, original page rendering, and
layout preservation are not implemented. FB2, MOBI, AZW3, URL reading,
dictionary lookup, text-to-speech, double spread, and seamless between-section
reading are not implemented yet.
