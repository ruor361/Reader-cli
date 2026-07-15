# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Rust CLI/TUI application for reading EPUB and TXT books in the terminal. Single-crate, ~1300 lines across 9 source files.

## Commands

```sh
cargo build                    # build
cargo test                     # run all tests
cargo test -- <name>           # run a single test
cargo run -- book.epub         # open a file directly
cargo run -- add <file>        # add book to library
cargo run -- list              # list library as table
cargo run -- shelf             # open interactive TUI shelf
cargo run -- open <id|title>   # open from library (fuzzy match)
cargo run -- resume            # resume last read book
cargo run -- search <book> <query>
cargo run -- bookmark add/list/goto/delete <book> ...
```

Sample EPUBs live in `Liberal/` (gitignored).

## Architecture

Eleven source files with clear separation:

- **`cli.rs`** — clap derive definitions (`Cli`, `Command`, `BookmarkCommand`, `ShelfCommand`)
- **`book.rs`** — Document model and parsing. `BookDocument` (id = SHA-256 truncated to 12 hex chars) contains `Section`s. `load_book()` dispatches to `load_epub()` (zip + OPF spine traversal via roxmltree) or `load_txt()` (split on double-newline). `html_to_text()` strips tags and decodes entities. `author_from_txt_path()` infers author from `Title - Author.txt` or `Title (Author).txt` filename patterns. Full-text `search()` across all sections.
- **`storage.rs`** — Persistence to `~/.config/reader/library.json` and `~/.config/reader/configuration.json` via serde_json. `Store` wraps `Library` (books + last_book_id). `LibraryBook` carries `author`, `page_count`, `last_opened` (all `#[serde(default)]` for backward compatibility). `add_book()` internally derives the author from EPUB metadata or TXT filename heuristic. `record_open(book_id, page_count)` records `last_opened = now()` and updates `page_count`. Supports upsert-by-id, fuzzy resolve (ID prefix or title substring), bookmarks, progress tracking. `scan_lib_dir(dir)` walks a directory, imports any `.epub`/`.txt` not already in the library, and merges the result via `ScanDirResult`.
- **`paths.rs`** — Single source of truth for the default config/library location. `config_dir()` prefers `$HOME/.config/reader/` (XDG-style, consistent across macOS and Linux) and falls back to `dirs::config_dir()/reader/`. `library_path()` and `configuration_path()` are thin wrappers that return the JSON filenames. Both `Store::open_default` and `ReaderConfig::open_default` route through these.
- **`config.rs`** — `ReaderConfig` persisted to `~/.config/reader/configuration.json`. Toggles (`show_progress`, `privacy_mode`, `mouse_support`), optional `text_width`, scan-time `library_dirs`, and `bookshelf_dir` (target dir for `add`-copied books; see `bookshelf.rs`). XDG-first resolution; see `paths.rs`.
- **`bookshelf.rs`** — Sidecar `.json` per book and `refresh` reconciliation. `sidecar_path(book)` derives `foo.epub` → `foo.epub.json`. `write_sidecar`/`read_sidecar`/`remove_sidecar` round-trip a full `LibraryBook`. `refresh(store, dir)` reconciles on-disk book files, sidecars, and `library.json` in four passes (enumerate → ensure library entry + sidecar → prune dead entries → prune orphan sidecars) and returns a `RefreshReport` of counts and errors. Invoked by `reader add` (write sidecar) and `reader shelf refresh` (full reconciliation).
- **`pagination.rs`** — `paginate()` takes sections + terminal dimensions, wraps text via `wrap_text()`, and splits into `Page`s. Content area is `(width-2) x (height-4)`.
- **`tui.rs`** — ratatui event loop. `ReaderApp` tracks current page, search state. Calls `store.record_open(book_id, pages.len())` after pagination so the shelf can show accurate progress. Progress saved on every keypress. vim-style bindings (j/k, g/G, /, n/N).
- **`shelf.rs`** — Interactive TUI bookshelf. `ShelfApp` filters/sorts `LibraryBook`s in memory. Renders a list with title, author, progress bar + percent, last-opened relative time, bookmark count. Modes: Browse, Search, ConfirmDelete. Outcomes bubble up as `ShelfOutcome::{Exit, Open(book)}` for the caller to dispatch. `format_relative_time` is also reused by `main::format_library_row`.
- **`main.rs`** — Entry point, CLI dispatch, opens `Store::default()`. `open_book_in_reader(entry, store, config)` is the shared helper used by `Open`, `Resume`, `Bookmark Goto`, and the `Shelf` `Open` outcome. `print_library_table` formats the same row data as plain text for the `list` command. `print_refresh_report` summarizes the four counts + errors from `bookshelf::refresh`. `Command::Shelf { command: Option<ShelfCommand> }` dispatches bare `reader shelf` to the TUI and `reader shelf refresh` to `bookshelf::refresh`.

Data flow: file → `book::load_book()` → `BookDocument` → `pagination::paginate()` → `Page`s → `tui::run_reader()` loop. Progress/bookmarks round-trip through `storage::Store`. `reader shelf` is the interactive home: it can launch `run_reader` and return on quit.

## Key libraries

ratatui 0.30 (crossterm_0_29 backend) · clap 4 derive · roxmltree 0.21 · zip 8.6 · serde_json · sha2 · anyhow
