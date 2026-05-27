# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Rust CLI/TUI application for reading EPUB and TXT books in the terminal. Single-crate, ~650 lines across 7 source files.

## Commands

```sh
cargo build                    # build
cargo test                     # run all tests
cargo test -- <name>           # run a single test
cargo run -- book.epub         # open a file directly
cargo run -- add <file>        # add book to library
cargo run -- list              # list library
cargo run -- open <id|title>   # open from library (fuzzy match)
cargo run -- resume            # resume last read book
cargo run -- search <book> <query>
cargo run -- bookmark add/list/goto/delete <book> ...
```

Sample EPUBs live in `Liberal/` (gitignored).

## Architecture

Six source files with clear separation:

- **`cli.rs`** — clap derive definitions (`Cli`, `Command`, `BookmarkCommand`)
- **`book.rs`** — Document model and parsing. `BookDocument` (id = SHA-256 truncated to 12 hex chars) contains `Section`s. `load_book()` dispatches to `load_epub()` (zip + OPF spine traversal via roxmltree) or `load_txt()` (split on double-newline). `html_to_text()` strips tags and decodes entities. Full-text `search()` across all sections.
- **`storage.rs`** — Persistence to `~/.config/reader/library.json` via serde_json. `Store` wraps `Library` (books + last_book_id). Supports upsert-by-id, fuzzy resolve (ID prefix or title substring), bookmarks, progress tracking.
- **`pagination.rs`** — `paginate()` takes sections + terminal dimensions, wraps text via `wrap_text()`, and splits into `Page`s. Content area is `(width-2) x (height-4)`.
- **`tui.rs`** — ratatui event loop. `ReaderApp` tracks current page, search state. Progress saved on every keypress. vim-style bindings (j/k, g/G, /, n/N).
- **`main.rs`** — Entry point, CLI dispatch, opens `Store::default()`.

Data flow: file → `book::load_book()` → `BookDocument` → `pagination::paginate()` → `Page`s → `tui::run_reader()` loop. Progress/bookmarks round-trip through `storage::Store`.

## Key libraries

ratatui 0.30 (crossterm_0_29 backend) · clap 4 derive · roxmltree 0.21 · zip 8.6 · serde_json · sha2 · anyhow
