# Reader

`reader` is a local-first command line reading tool for EPUB and TXT books.

## What works in v2

- Open a local book directly: `reader book.epub`
- Add/list/open/remove books in a local bookshelf
- Interactive TUI bookshelf: `reader shelf` for browsing, searching, sorting, opening
- Resume the most recently read book
- Full-screen terminal reading UI with reflow-stable progress saving
- EPUB/TXT parsing (EPUB pulls author from OPF metadata; TXT infers from `Title - Author.txt` or `Title (Author).txt` filename patterns)
- Percentage jump and searchable chapter navigation
- Metadata view
- Display configuration
- Privacy mode
- Bookmarks
- Book search

Reader stores its state in the user's config directory. When a
`bookshelf_dir` is configured, each book in the bookshelf gets a
sibling `.json` sidecar — see [Bookshelf](#bookshelf) below.

## Commands

```powershell
reader <file>
reader add <file>
reader list
reader shelf
reader shelf refresh [directory]
reader open <book-id-or-title>
reader resume
reader remove <book-id-or-title> [--delete-file]
reader bookmark add <book-id-or-title> <label>
reader bookmark list <book-id-or-title>
reader bookmark goto <book-id-or-title> <bookmark-id>
reader bookmark delete <book-id-or-title> <bookmark-id>
reader search <book-id-or-title> <query>
```

## Development

The repository selects the Windows GNU Rust host through
`rust-toolchain.toml`; Visual Studio and `link.exe` are not required. On
Windows, install the GNU toolchain plus a MinGW-w64 `gcc`/`ar` pair on `PATH`,
then run the normal Cargo commands below. The exact setup commands and the
verified recovery procedure for interrupted rustup downloads are documented in
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
| `c` | Switch the high-contrast dark/light theme |
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
| Reading | `t` | Open the chapter navigator |
| Reading | `M` | Open book info/metadata |
| Reading | `/` | Open search prompt |
| Reading | `s` | Show or hide progress |
| Reading | `p` | Toggle privacy stack overlay mode |
| Reading | `c` | Switch dark/light theme |
| Reading | `b` | Add a bookmark at the current text position |
| Reading | `n` / `N` | Next / previous search result |
| Reading | `?` / `F1` | Open complete contextual help |
| Chapters | `j/k`, arrows, PageUp/PageDown | Move selection |
| Chapters | digits + `Enter` | Jump directly to a chapter number |
| Chapters | `/`, text, arrows, `Enter` | Filter chapter titles and jump |
| Chapters | `Esc` | Clear an active filter, then close |
| Metadata | `Esc` / `Enter` / `M` | Close and return |
| Search | text | Type query |
| Search | `Backspace` | Delete character |
| Search | `Enter` | Run search and return |
| Search | `Esc` | Cancel and return |

"Page" in the movement rows means one current terminal screen, not a stored
book page number. Persistent progress, bookmarks, search results, and shelf
percentages use a Unicode character position within a chapter, so changing the
window size, text width, theme, or privacy layout does not move the saved place.

## Configuration

Reader stores display settings in `configuration.json` and the book
library in `library.json`, both under the same directory.

- Linux and macOS: `~/.config/reader/`
- Windows / systems without `~/.config`: the platform's
  `dirs::config_dir()` (e.g. `%APPDATA%\reader` on Windows)

Supported settings:

```json
{
  "theme": "dark",
  "show_progress": true,
  "privacy_mode": false,
  "mouse_support": false,
  "text_width": null
}
```

`theme` accepts `"dark"` or `"light"`. Both palettes explicitly set the
background, high-contrast body text, muted text, accents, borders, selections,
and error colors instead of relying on a terminal's dim default foreground.

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
the full record (id, title, author, stable text progress, content metrics, last
opened, missing status, bookmarks). For `Pride and Prejudice.epub`
the sidecar is `Pride and Prejudice.epub.json`. Sidecars are written
on `reader add`, `reader shelf refresh`, and normal state changes such
as reading progress and bookmark updates.

#### `reader shelf refresh`

`reader shelf refresh [directory]` recursively reconciles three sources:

1. the on-disk bookshelf directory (`.epub`/`.txt` files),
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
reader shelf refresh .\Liberal
reader shelf refresh
```

If no directory has ever been selected, `reader shelf refresh` reports the
exact command needed instead of requiring a manual JSON edit.
Missing books remain visible in `reader list` and `reader shelf`, but cannot be
opened until the file is restored and `reader shelf refresh` is run again.
`reader remove <book>` removes only the library record and sidecar. Use
`reader remove <book> --delete-file` to also delete the actual bookshelf file.

`text_width` may be set to a terminal column width, or left as `null` to use
the available terminal width. Reader centers the configured width and renders
it with the selected high-contrast palette.

### Storage migration

`library.json` and bookshelf sidecars use schema version 2. On first load of an
older record, Reader converts its page-based position into a stable text
position. If the record contains a page count, the old page ratio is mapped
across the book's Unicode character count. Otherwise Reader keeps the old
chapter and starts at that chapter's beginning. Bookmark labels and library
metadata are preserved; the next normal save writes the version 2 shape.

When `privacy_mode` is enabled from the reader with `p`, the reader shows a
fixed simulated JavaScript error stack in the top 80% of the terminal and keeps
only subdued reading text in the bottom 20%. Book titles, metadata, search text,
progress, and status messages stay hidden. Privacy mode only affects the TUI
rendering — CLI errors and panics are always printed in full so they remain
diagnosable.

## Not Yet Supported

Reader intentionally keeps this stage focused on core EPUB/TXT reading. FB2,
MOBI, AZW3, URL reading, dictionary lookup, text-to-speech, double spread, and
seamless between-chapter reading are not implemented yet.
