# Repository instructions

## Scope and orientation

These instructions apply to this repository. Reader is a single Rust crate
with a CLI and Ratatui reader/bookshelf for EPUB, TXT, and text-based PDF books.
Run Cargo commands from the directory containing `Cargo.toml`.

Read only the documentation needed for the task:

- [README.md](README.md): user-facing overview and first-run setup.
- [doc/usage.md](doc/usage.md): commands, controls, configuration, and data.
- [doc/architecture.md](doc/architecture.md): module map and design invariants.
- [doc/development.md](doc/development.md): checks and test organization.
- [doc/Windows构建环境.md](doc/Windows构建环境.md): Windows toolchains.

Treat the source as authoritative. Verify CLI behavior in both `src/cli.rs`
and `src/main.rs`; a declaration alone does not establish a working feature.
Dependency versions belong in `Cargo.toml`/`Cargo.lock`, not duplicated here.

## Working conventions

- Inspect `git status` and the relevant diff before editing. Preserve existing
  user changes and keep unrelated cleanup outside the task.
- Keep parsing, reader state, rendering, and filesystem effects in their
  existing modules. Use the module map before choosing where to make a change.
- Return contextual `anyhow::Result` errors for fallible work; avoid introducing
  panics on user input or file content.
- Keep local books, credentials, generated sidecars, exports, and build output
  out of commits. `Liberal/` is an optional gitignored sample directory, not a
  required test fixture. Automated fixtures should use temporary directories.

## Design constraints

- Book identity is the full SHA-256 of source bytes. Preserve content-based
  deduplication and migration of legacy IDs.
- Persist `TextPosition { section_index, char_offset }` using Unicode character
  offsets. Screen/page indices are runtime-only; resizing and reflow must
  preserve the stable reading position.
- Route default paths through `src/paths.rs`. Reuse `src/atomic_file.rs` for JSON
  and export writes, and preserve the managed import's validated atomic copy.
- Keep machine-local paths and missing-file state out of portable sidecars.
  Preserve library/sidecar migrations and reading-journal reconciliation.
- Keep network passwords out of persisted configuration. Preserve HTTPS
  credential handling, same-origin link/redirect checks, bounded downloads,
  and validation before a download enters the local bookshelf.
- `record_open` records recency only. Progress changes use the existing progress
  and reading-session paths.
- Add reader/shelf shortcuts through their binding registries so input, HUD,
  and help stay consistent. Inherit terminal colors through `src/ui.rs`.
- Discreet/hidden diagnostics must not expose real book metadata or search
  queries. `src/privacy.rs` is deprecated compatibility code.
- Preserve compatibility aliases: `add`, `shelf refresh`, and legacy
  `privacy_mode` configuration input.

## Verification and handoff

For Rust changes, run the applicable checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Run focused tests while iterating (`cargo test <test-name>`). For Windows GNU,
insert `+stable-x86_64-pc-windows-gnu` immediately after `cargo`; do not change
the repository target or global default toolchain to solve a local setup issue.

For documentation-only changes, check local links, fenced examples, and claims
against source. Compilation is only needed when it helps validate the change.
Update the relevant documentation when commands, keys, configuration, or data
behavior changes. Report what changed, what was verified, and any existing
blocker; do not present unimplemented declarations as completed functionality.
