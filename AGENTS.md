# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project

A Rust CLI/TUI application for reading EPUB, TXT, and text-based PDF books in the terminal. Single crate with 16 focused source modules.

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

Source modules have clear separation:

- **`cli.rs`** — clap derive definitions (`Cli`, `Command`, `BookmarkCommand`, `ShelfCommand`)
- **`book.rs`** — Document model and parsing. `BookDocument` uses the full SHA-256 of the source file bytes as a path-independent id and contains `Section`s. `TextPosition { section_index, char_offset }` and `ContentMetrics` are the stable, Unicode-character-based coordinate model used by navigation and persistence. `load_book()` dispatches to EPUB, TXT, or text-based PDF parsing; each PDF page becomes a stable section, and full-text `search()` returns stable positions.
- **`storage.rs`** — Versioned v3 machine-local persistence for `Library`, stable progress, content metrics, and materialized bookmarks. v1 page records and v2 path-derived ids migrate through the same implementation. Positions are clamped when content metrics are available. `record_open(book_id)` only records recency.
- **`paths.rs`** — Single source of truth for the default config/library location. `config_dir()` prefers `$HOME/.config/reader/` (XDG-style, consistent across macOS and Linux) and falls back to `dirs::config_dir()/reader/`. `library_path()` and `configuration_path()` are thin wrappers that return the JSON filenames. Both `Store::open_default` and `ReaderConfig::open_default` route through these.
- **`config.rs`** — `ReaderConfig` persisted to `configuration.json`. Includes display/Discreet Mode toggles, optional `text_width`, `library_dirs`, and managed `bookshelf_dir`; legacy `privacy_mode` input migrates to `discreet_mode`, while the removed `theme` key is ignored.
- **`bookshelf.rs`** — Portable v3 sidecar `.json` I/O and refresh reconciliation for EPUB, TXT, and PDF files. Sidecars omit absolute paths and machine-local missing state. Refresh restores seed state when the central library entry is absent, updates content-derived metrics, tracks missing/restored files, and removes orphan sidecars.
- **`reading_data.rs`** — Five-field UTC reading events, 15-minute idle session tracking, UUID bookmark operation logs, local/cloud journal reconciliation, and materialization of the latest cross-device progress/bookmarks into `Store`. Local data lives under the config directory and mirrors to `<bookshelf_dir>/.reader/`.
- **`pagination.rs`** — Runtime-only `Page`s with `source_start`/`source_end` positions. The anchored wrapper handles Unicode display width, CJK text, long words, whitespace, and blank lines. `page_for_position()` maps a stable position back to the containing screen after reflow.
- **`reader_state.rs`** — Pure reader state machine: current stable position, runtime pages, overlays, notifications, percentage/contents/search navigation, and effect values for persistence/config changes.
- **`reader_input.rs`** — Key-to-`ReaderAction` parsing. Reading commands are declared once in `READING_BINDINGS`, which also drives the HUD and help view.
- **`reader_view.rs`** — Pure Ratatui reader rendering: terminal-inheriting body, two-row HUD, responsive action hints, notifications, all overlays, and Discreet Mode rendering.
- **`ui.rs`** — Shared terminal-inheriting semantic colors, key-binding registry helpers, responsive HUD generation, popup layout, and help generation used by both reader and shelf.
- **`tui.rs`** — Thin reader event loop. It applies state-machine effects, checkpoints reading sessions, persists progress/bookmarks/config, updates versioned sidecars, and handles resize without changing the stable position.
- **`shelf.rs`** — Interactive bookshelf using terminal colors, a two-row HUD, binding-generated help, stable percentages, search/sort/resume/delete flows, and `ShelfOutcome::{Exit, Open(book)}`.
- **`discreet.rs`** — Fixed redacted diagnostic content plus stable per-page log decoration and adaptive prefix/body layout for Discreet Mode; it never exposes the reader's real metadata/query. `privacy.rs` contains deprecated compatibility exports only.
- **`main.rs`** — Entry point, CLI dispatch, opens `Store::default()`. `open_book_in_reader(entry, store, config)` is the shared helper used by `Open`, `Resume`, `Bookmark Goto`, and the `Shelf` `Open` outcome. `print_library_table` formats the same row data as plain text for the `list` command. `print_refresh_report` summarizes the four counts + errors from `bookshelf::refresh`. `Command::Shelf { command: Option<ShelfCommand> }` dispatches bare `reader shelf` to the TUI and `reader shelf refresh` to `bookshelf::refresh`.

Data flow: file → `book::load_book()` → `BookDocument`/`ContentMetrics` → anchored runtime pagination → `ReaderState` → pure reader view. The event loop maps stable positions to screens and applies persistence effects through `Store`; page indices never leave runtime memory. `reader shelf` is the interactive home and returns after a launched reader quits.

## Key libraries

ratatui 0.30 (crossterm_0_29 backend) · clap 4 derive · pdf-extract 0.12 · roxmltree 0.21 · zip 8.6 · serde_json · sha2 · anyhow
