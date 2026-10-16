# Development

[Back to README](../README.md) · [Architecture](architecture.md)

## Toolchain and build

Run commands from the directory containing `Cargo.toml`. `rust-toolchain.toml`
selects stable Rust with `rustfmt` and `clippy`, using the native host. It does
not force a Windows host or cross-compilation target. Dependency versions are
recorded in `Cargo.toml` and `Cargo.lock`.

```sh
cargo build
cargo build --release
cargo run -- --help
```

For Windows linker prerequisites and explicit GNU commands, see
[Windows build setup](Windows构建环境.md).

## Try the application

Create or choose an existing book folder before importing a supported file:

```sh
mkdir Books
cargo run -- refresh ./Books
cargo run -- import "path/to/book.epub"
cargo run -- shelf
cargo run -- "path/to/book.epub"
```

These commands write real configuration/library state in the default user data
location and copy books into the selected folder. For automated verification,
use temporary fixtures with `Store::open(path)`, `ReaderConfig::open(path)`, and
`ReadingDataStore::new(local_root, cloud_root)`.

`Liberal/` is an optional gitignored sample folder; builds and automated tests
must not require personal book files.

## Verification

For Rust changes:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Filter tests by name before running the full suite:

```sh
cargo test <test-name>
cargo test --test library_workflow
cargo test --test contents_workflow
cargo test --test pdf_workflow
```

`cargo test <test-name>` applies the name filter. Arguments after `--` are passed
to the test harness (for example, `cargo test <test-name> -- --nocapture`).
For documentation-only edits, verify relative links, code-fence syntax, JSON
examples, and behavior against the relevant implementation.

## Test layout

| Location | Coverage |
| --- | --- |
| Inline `#[cfg(test)]` modules in `src/` | Parsing, migration, deduplication, persistence, pagination, state, input, views, and reconciliation |
| `tests/library_workflow.rs` | TXT search and persisted progress/bookmarks across reloads |
| `tests/contents_workflow.rs` | EPUB 2/3 contents and navigation |
| `tests/pdf_workflow.rs` | PDF metadata/contents, extracted sections, search, and persisted reading state |

Check the existing working tree before edits and distinguish pre-existing
failures from task regressions. CLI variants must have dispatch implementations,
and modules declared in `lib.rs` must have corresponding source files before
that checkout can compile. Do not advertise a feature based only on a declaration.

## Documentation maintenance

See [documentation index](README.md) for ownership and current-vs-target
authority. Keep setup in [root README](../README.md), **implemented**
commands and controls in [usage](usage.md), technical invariants in
[architecture](architecture.md), build/check instructions here, and agent
work constraints in [AGENTS.md](../AGENTS.md).

The [Agent Terminal design contract](design/agent-terminal-reading.md)
defines **future UX acceptance**. Historical experiments and implementation
records live under [notes](notes/阅读体验优化讨论.md) and must not become
requirements by accident. Update tests when the contract deliberately
supersedes old layouts; a passing old snapshot may be the wrong target.

For new features, verify actual code before describing them as available.
Maintain relative links and avoid duplicating the same rules across
README, usage, architecture, and design documents.
