# Architecture

[Back to README](../README.md) · [Development](development.md)

Reader is a single Rust crate with a binary entry point in `src/main.rs` and
library exports in `src/lib.rs`. The reading path separates parsing, state
transitions, rendering, and persistence.

## Module map

All paths in this table are relative to `src/`.

| Module | Responsibility |
| --- | --- |
| `main.rs` | CLI dispatch, shared book-opening flow, refresh reports, search output, and JSON/CSV export |
| `lib.rs` | Module declarations and public re-exports |
| `cli.rs` | Clap commands, arguments, and export-format definitions |
| `book.rs` | EPUB/TXT/PDF loading, metadata/contents, content hashing, Unicode positions, metrics, and search |
| `bookshelf.rs` | Managed import, deduplication, portable sidecars, and recursive refresh |
| `storage.rs` | Versioned local library, stable progress/bookmarks, selection, recency, and migrations |
| `paths.rs` | Default configuration, library, and local reading-data paths |
| `config.rs` | Configuration defaults, loading/saving, display toggles, and legacy handling |
| `atomic_file.rs` | Temporary-file writes, flush/sync, and atomic replacement |
| `reading_data.rs` | UTC session events, checkpoints, bookmark operations, mirror reconciliation, and materialization |
| `pagination.rs` | Unicode-width wrapping and runtime pages anchored to source positions |
| `reader_state.rs` | Pure state transitions, navigation, overlays, notifications, and effects |
| `reader_input.rs` | Reader key registries and key-to-action conversion |
| `reader_view.rs` | Pure Ratatui body, HUD, notification, overlay, and Discreet Mode rendering |
| `tui.rs` | Reader event loop, effect execution, resize handling, sessions, and persistence |
| `shelf.rs` | Local shelf, import/search/sort/removal, network switching, and `ShelfOutcome` |
| `network.rs` | OPDS requests/parsing, authentication, URL/certificate checks, and validated download/import |
| `network_shelf.rs` | Network shelf UI, catalog search/paging, and background request handling |
| `ui.rs` | Shared semantic colors, key registry helpers, responsive HUD, help, and popup layout |
| `discreet.rs` | Redacted diagnostics and adaptive decoration |
| `privacy.rs` | Deprecated compatibility exports for `discreet.rs` |

Dependency versions belong in `Cargo.toml` and `Cargo.lock`.

## Reading flow

```text
file -> managed import -> book::load_book -> BookDocument + ContentMetrics
     -> anchored pagination -> ReaderState -> reader_view::draw
keyboard -> reader_input -> ReaderAction -> state update -> ReaderEffect
ReaderEffect -> tui -> Store / ReaderConfig / ReadingDataStore / sidecars
resize -> reflow pages -> restore page from stable TextPosition
OPDS catalog -> network download -> managed import -> shared local reader
```

`main::open_book_in_reader` is shared by `open`, `resume`, `bookmark goto`, and
books opened from the shelf. The shelf returns `ShelfOutcome::Open(book)` to
`main`; quitting the launched reader returns to the shelf loop.

## Stable identity and positions

A book ID is the full SHA-256 of its source bytes. Relocating identical content
does not change its ID. Managed imports validate before copying, verify the
copy, reuse identical content, and avoid overwriting different content with
the same filename.

`TextPosition { section_index, char_offset }` uses Unicode character offsets,
not byte offsets or terminal columns. PDF physical pages become stable sections.
`ContentMetrics` computes progress and clamps positions to available content.
Runtime `Page` values carry `source_start`/`source_end` anchors;
`page_for_position()` maps a stable position back to the screen after reflow.
Screen indices never become persisted progress or bookmark coordinates.

## Persistence boundaries

- `storage.rs` uses library schema v3. Legacy page-based records migrate to
  stable positions; path-derived IDs reconcile with content-derived IDs while
  retaining progress, bookmarks, and resume references.
- `bookshelf.rs` writes v3 sidecars. Portable state excludes absolute paths and
  local missing status. Refresh seeds absent local records from sidecars,
  tracks missing/restored files, and removes orphan sidecars.
- `reading_data.rs` uses journal schema v1. Events and bookmark operations are
  stored locally and mirrored under the bookshelf's `.reader/` directory.
  Reconciliation materializes progress/bookmarks into `Store`.
- Active reading events are updated by checkpoints. Bookmark operations have
  independent UUID records; deletion takes precedence for the same bookmark.
- `Store::record_open()` records recency only. Reading state is saved through
  the explicit progress/session paths.
- `atomic_file.rs` writes completed data to a temporary file beside the target
  before replacing it. Managed copying separately validates and atomically
  publishes book files in `bookshelf.rs`.

See [Usage](usage.md#configuration-and-data-paths) for filesystem locations and
[folder synchronization](usage.md#folder-synchronization) for the external sync
workflow. OPDS downloads enter the local bookshelf; they do not synchronize
server reading progress. Passwords stay outside persisted configuration.

## UI boundaries

Reader state and rendering remain independent of filesystem effects. `tui.rs`
applies effect values and uses a guard to restore the terminal on exit. Shared
colors inherit the terminal palette. Reader and local shelf binding registries
also generate HUD hints and help, so shortcut changes belong in those registries.

Discreet Mode uses redacted diagnostics without real metadata or search queries.
`privacy.rs` keeps old exports available; new implementation belongs in
`discreet.rs`.
