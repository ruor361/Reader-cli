# Reader

Reader is a local-first Rust CLI/TUI for reading EPUB, TXT, and text-based PDF
books in the terminal. It provides a managed bookshelf, full-text search,
bookmarks, stable reading progress, and portable reading data.

## Documentation

| Document | Contents |
| --- | --- |
| [Usage](doc/usage.md) | Commands, keyboard controls, configuration, synchronization, and export |
| [Architecture](doc/architecture.md) | Module responsibilities, data flow, and persistence invariants |
| [Development](doc/development.md) | Build commands, verification, and test layout |
| [Windows build setup](doc/Windows构建环境.md) | MSVC/GNU setup and troubleshooting (中文) |
| [AGENTS.md](AGENTS.md) | Repository instructions for coding agents |

## Features

- Interactive bookshelf and script-friendly CLI commands
- Calibre-Web OPDS browsing and downloads into the local bookshelf
- Navigation by screen, percentage, table of contents, and search results
- Progress and bookmarks that survive terminal resizing and text reflow
- Managed imports that preserve source files and deduplicate identical content
- Portable metadata sidecars and reading journals for folder synchronization
- Reading-event export as JSON or CSV
- Terminal-inherited colors, optional reading width, and Discreet Mode

## Supported formats

| Format | Support |
| --- | --- |
| EPUB | Metadata, EPUB 2/3 contents, chapters, search, and reflowed text |
| TXT | Unicode plain text; title and author inferred from common filename patterns |
| PDF | Text extraction by physical page, metadata, outlines, search, and reflowed text |

PDF support requires extractable text. Original page layout, scanned/image-only
PDFs without OCR, and password-protected PDFs are unsupported. FB2, MOBI, AZW3,
arbitrary web URL reading, dictionary lookup, and text-to-speech are also unsupported.

## Installation

Install the stable Rust toolchain and run from the project directory:

```sh
cargo install --path .
```

To build without installing:

```sh
cargo build --release
cargo run -- --help
```

The executable is `target/release/reader` on Unix or
`target\release\reader.exe` on Windows. See the
[Windows build setup](doc/Windows构建环境.md) for MSVC and GNU prerequisites.

## Quick start

Create a book folder first, then configure it as the managed bookshelf:

```sh
mkdir Books
reader refresh ./Books
reader import "book.epub"
reader shelf
```

Use an existing directory instead of `Books` if you already have a book folder.
`refresh` scans supported files recursively and saves the folder as
`bookshelf_dir`.

After configuring the bookshelf, open a file directly or resume your last book:

```sh
reader "book.epub"
reader resume
```

Opening a file directly performs a managed import before launching the reader.
The original file is preserved. Imports and direct file opens both require a
configured bookshelf; `reader shelf` can guide you through setup interactively.

In the reader, use `j`/`k` to move, `/` to search, `b` to add a bookmark, `?` for
help, and `q` to save and quit. See [Usage](doc/usage.md) for the full reference.

For a Calibre-Web server, see [Network bookshelf](doc/usage.md#network-bookshelf-calibre-web--calibre-web-automated).

## Development

See [Development](doc/development.md) for build and verification commands and
[Architecture](doc/architecture.md) for the module map. Coding agents should
start with [AGENTS.md](AGENTS.md).

## License

MIT, as declared in `Cargo.toml`.
