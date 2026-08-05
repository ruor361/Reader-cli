# Reader

Reader is a local-first terminal application for reading EPUB, TXT, and
text-based PDF books. It combines a focused full-screen reader with a searchable
bookshelf, stable reading progress, bookmarks, and portable reading data.

## Features

- Read EPUB, TXT, and text-based PDF files directly in the terminal
- Browse books from an interactive TUI shelf or use script-friendly CLI commands
- Navigate by screen, percentage, table of contents, or full-text search
- Save progress with stable text positions that survive terminal resizing and reflow
- Create bookmarks and resume the most recently opened book
- Keep book files, metadata sidecars, progress, and bookmarks in a managed folder
- Export reading events as JSON or CSV
- Inherit the terminal's own colors and optionally constrain the reading width

Reader also includes an optional low-exposure display mode, available from the
in-app help when needed.

## Supported formats

| Format | Support |
| --- | --- |
| EPUB | Metadata, EPUB 2/3 contents, chapters, search, and reflowed text |
| TXT | Unicode plain text, with title and author inferred from common filename patterns |
| PDF | Text extraction by physical page, metadata, outlines, search, and reflowed text |

PDF support is intended for text-based documents. Scanned PDFs and
layout-accurate page rendering are not supported.

## Installation

Reader requires the stable Rust toolchain. From the project directory:

```sh
cargo install --path .
```

Alternatively, build without installing:

```sh
cargo build --release
cargo run -- book.epub
```

The compiled executable is written to `target/release/reader` (or
`target\release\reader.exe` on Windows).

Windows users who do not have Visual Studio Build Tools can follow the
[Windows GNU toolchain setup](doc/Windows构建环境.md).

## Quick start

Open a book directly:

```sh
reader book.epub
```

Build a local library and open the interactive shelf:

```sh
reader refresh ./Books
reader import book.epub
reader import notes.txt
reader shelf
```

Common commands:

```text
reader <file>
reader import <file> [--open]
reader add <file> [--open]     # compatibility alias
reader list
reader shelf
reader open <book-id-or-title>
reader resume
reader remove <book-id-or-title> [--delete-file]
reader search <book-id-or-title> <query>
reader bookmark add <book-id-or-title> <label>
reader bookmark list <book-id-or-title>
reader bookmark goto <book-id-or-title> <bookmark-id>
reader bookmark delete <book-id-or-title> <bookmark-id>
reader refresh [directory]
reader export [book-id-or-title] [--format json|csv] [--output <file>]
```

Book IDs and bookmark IDs accept any unique prefix. `reader list` shows the
first 12 characters of each book ID.

## Reader controls

| Key | Action |
| --- | --- |
| `j` / `Down` / `Space` / `PageDown` | Next screen |
| `k` / `Up` / `PageUp` | Previous screen |
| `g` | Go to a percentage |
| `Home` / `End` / `G` | Go to the beginning / end |
| `t` | Browse the table of contents |
| `/` | Search within the book |
| `n` / `N` | Next / previous search result |
| `b` | Add a bookmark at the current position |
| `M` | Show book information |
| `s` | Show or hide reading progress |
| `?` / `F1` | Show all available shortcuts |
| `q` / `Ctrl+C` | Save and quit |

A “screen” is the content visible at the current terminal size, not a stored
page number. Progress and bookmarks use section-relative Unicode character
positions, so changing the terminal size or text width does not move the saved
reading location.

## Shelf controls

`reader shelf` shows title, author, progress, last-opened time, bookmark count,
and missing-file status for every library entry.

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
| `d` | Remove the selected library entry after confirmation |
| `?` / `F1` | Show shelf help |
| `q` / `Esc` / `Ctrl+C` | Quit |

## Managed bookshelf and synchronization

Before importing the first book, create a managed bookshelf by passing an
existing directory to `reader refresh`:

```sh
reader refresh ./Books
```

The directory is saved as `bookshelf_dir`. Future imports and direct file
opens are copied into it, while the original source file is left untouched.
`reader import <file>` imports without opening; add `--open` to start reading.
The older `reader add` spelling remains available as an alias. Inside
`reader shelf`, press `i` and paste a path to perform the same managed import.
If no bookshelf is configured, the shelf guides you through setting one first.
Each managed book receives a sibling `.json` sidecar containing portable
metadata, progress, and bookmarks.

Reader also mirrors its append-only reading and bookmark journals into
`<bookshelf_dir>/.reader/`. Synchronizing the bookshelf with OneDrive, Dropbox,
iCloud, Syncthing, or another folder-sync tool lets Reader reconcile reading
state across machines. Reader itself does not connect to a cloud provider.

Run `reader refresh` after files have been added, removed, restored, or synced.
The older `reader shelf refresh [directory]` spelling remains available.

## Configuration and data

Reader creates `configuration.json` and `library.json` in:

- Linux and macOS: `~/.config/reader/`
- Windows and other platforms without `~/.config`: the platform configuration
  directory, such as `%APPDATA%\reader`

Common configuration values include:

```json
{
  "show_progress": true,
  "mouse_support": false,
  "text_width": null,
  "bookshelf_dir": null
}
```

Set `text_width` to a terminal column width to center a narrower reading area,
or leave it as `null` to use the available width. The interface inherits the
terminal's foreground, background, and ANSI color palette.

Library records and configuration files are written atomically. Book identity
is based on the full SHA-256 hash of the source file, so moving the same file to
another path does not change its ID.

## Current limitations

- PDF reading extracts and reflows text; it does not preserve the original page layout
- Scanned or image-only PDFs require OCR before Reader can open them
- Password-protected PDFs are not supported
- FB2, MOBI, AZW3, web URLs, dictionary lookup, and text-to-speech are not supported

## Development

```sh
cargo run -- --help
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The project is a single Rust crate. The parsing, persistence, pagination,
reader state, rendering, and shelf components are kept in focused modules under
`src/`.

## License

MIT
