use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;
use reader::book::load_book;
use reader::bookshelf;
use reader::cli::{BookmarkCommand, Cli, Command, ShelfCommand};
use reader::config::ReaderConfig;
use reader::shelf::{run_shelf, ShelfOutcome};
use reader::storage::{LibraryBook, Store};
use reader::tui::run_reader;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}", render_error(&err));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut store = Store::open_default()?;
    let mut config = ReaderConfig::open_default()?;

    match cli.command {
        Some(Command::Add { file }) => {
            let file = resolve_input_path(file)?;
            let book = import_book(&mut store, &config, &file)?;
            let title = book.title.clone();
            let id = book.id.clone();
            store.save()?;
            sync_sidecar_if_configured(&store, &config, &id)?;
            println!("Added: {} ({})", title, id);
        }
        Some(Command::List) => {
            print_library_table(&store);
        }
        Some(Command::Shelf { command }) => match command {
            None => loop {
                match run_shelf(&mut store, &mut config, None)? {
                    ShelfOutcome::Exit => break,
                    ShelfOutcome::Open(book) => open_book_in_reader(book, &mut store, &mut config)?,
                }
            },
            Some(ShelfCommand::Refresh { dir }) => {
                let supplied_dir = dir.is_some();
                let dir = match dir {
                    Some(dir) => resolve_bookshelf_dir(&dir)?,
                    None => config.bookshelf_dir.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "bookshelf_dir is not configured. Pass it once with \
                             `reader shelf refresh <DIR>`."
                        )
                    })?,
                };
                let report = bookshelf::refresh(&mut store, &dir)?;
                store.save()?;
                if supplied_dir {
                    config.bookshelf_dir = Some(dir);
                    config.save()?;
                }
                print_refresh_report(&report);
            }
        },
        Some(Command::Open { book }) => {
            let entry = store.resolve_book(&book)?.clone();
            open_book_in_reader(entry, &mut store, &mut config)?;
        }
        Some(Command::Resume) => {
            let entry = store
                .last_book()?
                .with_context(|| "No recent book. Open or add a book first.")?
                .clone();
            open_book_in_reader(entry, &mut store, &mut config)?;
        }
        Some(Command::Remove { book, delete_file }) => {
            let removed = store.remove_book(&book)?;
            bookshelf::remove_sidecar_for_book(&removed)?;
            if delete_file {
                delete_bookshelf_file(&removed, &config)?;
            }
            store.save()?;
            println!("Removed: {} ({})", removed.title, removed.id);
        }
        Some(Command::Bookmark { command }) => match command {
            BookmarkCommand::Add { book, label } => {
                let entry = store.resolve_book(&book)?.clone();
                let progress = store.progress_for(&entry.id).cloned().unwrap_or_default();
                let bookmark = store.add_bookmark(&entry.id, label, progress)?;
                store.save()?;
                sync_sidecar_if_configured(&store, &config, &entry.id)?;
                println!(
                    "Bookmark added: {} at chapter {} · {}",
                    bookmark.id,
                    bookmark.position.section_index + 1,
                    format_progress_tenths(
                        entry.content_metrics.progress_tenths(bookmark.position)
                    )
                );
            }
            BookmarkCommand::List { book } => {
                let entry = store.resolve_book(&book)?;
                for bookmark in store.bookmarks_for(&entry.id) {
                    println!(
                        "{}  chapter {}  {}  {}",
                        bookmark.id,
                        bookmark.position.section_index + 1,
                        format_progress_tenths(
                            entry.content_metrics.progress_tenths(bookmark.position)
                        ),
                        bookmark.label
                    );
                }
            }
            BookmarkCommand::Goto { book, bookmark } => {
                let entry = store.resolve_book(&book)?.clone();
                let target = store
                    .bookmark_for(&entry.id, &bookmark)
                    .with_context(|| format!("Bookmark not found: {bookmark}"))?
                    .clone();
                store.set_progress(&entry.id, target.progress());
                store.save()?;
                sync_sidecar_if_configured(&store, &config, &entry.id)?;
                open_book_in_reader(entry, &mut store, &mut config)?;
            }
            BookmarkCommand::Delete { book, bookmark } => {
                let entry = store.resolve_book(&book)?.clone();
                let removed = store.delete_bookmark(&entry.id, &bookmark)?;
                store.save()?;
                sync_sidecar_if_configured(&store, &config, &entry.id)?;
                println!("Deleted bookmark: {} ({})", removed.label, removed.id);
            }
        },
        Some(Command::Search { book, query }) => {
            let entry = store.resolve_book(&book)?.clone();
            let document = load_book(&entry.path)?;
            let results = document.search(&query);
            if results.is_empty() {
                bail!("No matches for {query:?}");
            }
            let metrics = document.content_metrics();
            for result in results.into_iter().take(25) {
                println!(
                    "chapter {}  {}  {}",
                    result.position.section_index + 1,
                    format_progress_tenths(metrics.progress_tenths(result.position)),
                    result.preview
                );
            }
        }
        None => {
            let file = cli
                .file_path()
                .with_context(|| "Pass a book path or use a subcommand. Try `reader --help`.")?;
            let file = resolve_input_path(file)?;
            let book = import_book(&mut store, &config, &file)?;
            let id = book.id.clone();
            store.save()?;
            sync_sidecar_if_configured(&store, &config, &id)?;
            let entry = store
                .book_by_id(&id)
                .with_context(|| format!("Imported book disappeared: {id}"))?
                .clone();
            open_book_in_reader(entry, &mut store, &mut config)?;
        }
    }

    Ok(())
}

fn resolve_bookshelf_dir(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Failed to resolve the current directory")?
            .join(path)
    };
    let absolute = absolute
        .canonicalize()
        .with_context(|| format!("Bookshelf directory not found: {}", absolute.display()))?;
    if !absolute.is_dir() {
        bail!("Bookshelf path is not a directory: {}", absolute.display());
    }
    Ok(user_facing_path(absolute))
}

#[cfg(windows)]
fn user_facing_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    if let Some(local) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(local);
    }
    path
}

#[cfg(not(windows))]
fn user_facing_path(path: PathBuf) -> PathBuf {
    path
}

fn open_book_in_reader(
    entry: reader::storage::LibraryBook,
    store: &mut Store,
    config: &mut ReaderConfig,
) -> Result<()> {
    if entry.missing {
        bail!(
            "Book file is missing: {}. Put the file back under bookshelf_dir and run `reader shelf refresh`.",
            entry.path.display()
        );
    }
    let document = load_book(&entry.path)?;
    let book_id = entry.id.clone();
    run_reader(document, book_id.clone(), store, config)?;
    sync_sidecar_if_configured(store, config, &book_id)?;
    Ok(())
}

fn import_book(store: &mut Store, config: &ReaderConfig, source: &Path) -> Result<LibraryBook> {
    if let Some(existing) = store.book_by_path(source) {
        return Ok(existing.clone());
    }

    let stored = match config.bookshelf_dir.as_deref() {
        Some(dir) => choose_bookshelf_path(store, source, dir)?,
        None => source.to_path_buf(),
    };
    let document = load_book(&stored)?;
    Ok(store.add_book(&document, &stored)?.clone())
}

fn choose_bookshelf_path(store: &Store, source: &Path, bookshelf_dir: &Path) -> Result<PathBuf> {
    if path_is_in_dir(source, bookshelf_dir) {
        return Ok(source.to_path_buf());
    }

    let file_name = source
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("source path has no filename: {}", source.display()))?;
    let target = bookshelf_dir.join(file_name);
    if target.exists() {
        if store.book_by_path(&target).is_some() {
            return Ok(target);
        }
        return copy_to_library_unique(source, bookshelf_dir);
    }
    copy_to_library_unique(source, bookshelf_dir)
}

fn sync_sidecar_if_configured(store: &Store, config: &ReaderConfig, book_id: &str) -> Result<()> {
    if config.bookshelf_dir.is_some() {
        bookshelf::sync_sidecar_for_book(store, book_id)?;
    }
    Ok(())
}

fn delete_bookshelf_file(book: &LibraryBook, config: &ReaderConfig) -> Result<()> {
    let Some(dir) = config.bookshelf_dir.as_deref() else {
        bail!("--delete-file requires bookshelf_dir to be configured");
    };
    if !path_is_in_dir(&book.path, dir) {
        bail!(
            "Refusing to delete file outside bookshelf_dir: {}",
            book.path.display()
        );
    }
    if book.path.exists() {
        std::fs::remove_file(&book.path)
            .with_context(|| format!("Failed to delete {}", book.path.display()))?;
    }
    Ok(())
}

fn print_library_table(store: &Store) {
    let books = store.library().books();
    if books.is_empty() {
        println!("No books in library. Add a book with `reader add <file>`.");
        return;
    }
    println!(
        "{:<2} {:<30}  {:<16}  {:>4}  {:<10}  {:>4}  {:<5}  {:<7}",
        "", "Title", "Author", "Done", "Last Opened", "Bkmk", "Fmt", "Status"
    );
    for book in books {
        println!(
            "{}",
            format_library_row(book, store.library().last_book_id.as_deref())
        );
    }
}

fn format_library_row(book: &LibraryBook, last_book_id: Option<&str>) -> String {
    let marker = if last_book_id == Some(book.id.as_str()) {
        "*"
    } else {
        " "
    };
    let title = truncate_str(&book.title, 30);
    let author = book
        .author
        .as_deref()
        .map(|value| truncate_str(value, 16))
        .unwrap_or_else(|| "-".to_string());
    let percent = format!("{}%", progress_percent(book));
    let last = book
        .last_opened
        .map(reader::shelf::format_relative_time)
        .unwrap_or_else(|| "-".to_string());
    let bookmarks = if book.bookmarks.is_empty() {
        "-".to_string()
    } else {
        book.bookmarks.len().to_string()
    };
    let format = match book.format {
        reader::book::BookFormat::Epub => "EPUB",
        reader::book::BookFormat::Txt => "TXT",
    };
    let status = if book.missing { "missing" } else { "ok" };
    format!(
        "{:<2} {:<30}  {:<16}  {:>4}  {:<10}  {:>4}  {:<5}  {:<7}",
        marker, title, author, percent, last, bookmarks, format, status
    )
}

fn progress_percent(book: &LibraryBook) -> u16 {
    book.progress_percent()
}

fn format_progress_tenths(tenths: u16) -> String {
    format!("{}.{:01}%", tenths / 10, tenths % 10)
}

fn print_refresh_report(report: &bookshelf::RefreshReport) {
    println!("Shelf refresh complete:");
    println!("  books_added:       {}", report.books_added);
    println!("  books_updated:     {}", report.books_updated);
    println!("  books_missing:     {}", report.books_missing);
    println!("  books_restored:    {}", report.books_restored);
    println!("  books_removed:     {}", report.books_removed);
    println!("  sidecars_written:  {}", report.sidecars_written);
    println!("  sidecars_removed:  {}", report.sidecars_removed);
    if !report.errors.is_empty() {
        println!("  errors:            {}", report.errors.len());
        for (path, err) in &report.errors {
            println!("    {} — {}", path.display(), err);
        }
    }
}

fn truncate_str(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else if max <= 1 {
        "\u{2026}".to_string()
    } else {
        let mut out: String = text.chars().take(max - 1).collect();
        out.push('\u{2026}');
        out
    }
}

// Privacy mode only affects what the TUI renders. CLI errors and panics
// are always printed in full so the user can debug regardless of mode.
fn render_error(err: &anyhow::Error) -> String {
    format!("Error: {err:?}")
}

fn resolve_input_path(path: PathBuf) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path);
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(path);
    };
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(path);
    };

    let normalized_name = normalize_whitespace(file_name);
    let mut matches = std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| normalize_whitespace(name) == normalized_name)
        });

    let Some(first) = matches.next() else {
        return Ok(path);
    };

    if let Some(second) = matches.next() {
        bail!(
            "Multiple files match {} after normalizing spaces: {} and {}",
            path.display(),
            first.display(),
            second.display()
        );
    }

    Ok(first)
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Copy `src` into `storage_dir` (creating it if needed) and return the
/// destination path. If `src` is already inside `storage_dir`, returns it
/// unchanged. If the destination file already exists, picks a unique
/// name by appending " (1)", " (2)", ... before the extension.
fn copy_to_library_unique(src: &Path, storage_dir: &Path) -> Result<PathBuf> {
    if path_is_in_dir(src, storage_dir) {
        return Ok(src.to_path_buf());
    }
    if !storage_dir.exists() {
        std::fs::create_dir_all(storage_dir)
            .with_context(|| format!("Failed to create {}", storage_dir.display()))?;
    }
    let file_name = src
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("source path has no filename: {}", src.display()))?;
    let mut target = storage_dir.join(file_name);
    let mut counter: u32 = 1;
    while target.exists() {
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("book");
        let ext = src.extension().and_then(|e| e.to_str());
        let new_name = match ext {
            Some(ext) if !ext.is_empty() => format!("{} ({}).{}", stem, counter, ext),
            _ => format!("{} ({})", stem, counter),
        };
        target = storage_dir.join(new_name);
        counter += 1;
    }
    std::fs::copy(src, &target)
        .with_context(|| format!("Failed to copy {} to {}", src.display(), target.display()))?;
    Ok(target)
}

fn path_is_in_dir(path: &Path, dir: &Path) -> bool {
    if let (Ok(path), Ok(dir)) = (path.canonicalize(), dir.canonicalize()) {
        return path.starts_with(dir);
    }
    path.starts_with(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader::storage::Progress;

    #[test]
    fn resolves_file_name_with_collapsed_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("A Grammar of the English Language  Barnes - William Cobbett.epub");
        std::fs::write(&file, []).unwrap();

        let collapsed = dir
            .path()
            .join("A Grammar of the English Language Barnes - William Cobbett.epub");

        assert_eq!(resolve_input_path(collapsed).unwrap(), file);
    }

    #[test]
    fn leaves_unknown_file_path_unchanged() {
        let path = std::path::Path::new("missing").join("book.epub");

        assert_eq!(resolve_input_path(path.clone()).unwrap(), path);
    }

    #[cfg(windows)]
    #[test]
    fn strips_windows_verbatim_prefix_from_persisted_bookshelf_path() {
        assert_eq!(
            user_facing_path(PathBuf::from(r"\\?\G:\Document\reader\Liberal")),
            PathBuf::from(r"G:\Document\reader\Liberal")
        );
        assert_eq!(
            user_facing_path(PathBuf::from(r"\\?\UNC\server\books\Liberal")),
            PathBuf::from(r"\\server\books\Liberal")
        );
    }

    #[test]
    fn error_output_always_keeps_real_error_details() {
        // Privacy mode used to swap real errors for a fake JS stack; that
        // suppressed useful diagnostics. The new contract is: render_error
        // always returns the real error, regardless of any privacy setting.
        let err = anyhow::anyhow!("Book not found: secret-title");

        let output = render_error(&err);

        assert!(output.contains("Book not found: secret-title"));
        assert!(!output.contains("Uncaught TypeError"));
        assert!(!output.contains("PrivacyOverlay.render"));
    }

    #[test]
    fn format_library_row_includes_marker_and_progress() {
        let book = LibraryBook {
            id: "abc".into(),
            title: "Pride and Prejudice".into(),
            author: Some("Jane Austen".into()),
            path: PathBuf::from("/tmp/p.txt"),
            format: reader::book::BookFormat::Txt,
            content_metrics: reader::book::ContentMetrics {
                section_char_counts: vec![100],
            },
            progress: Progress::new(reader::book::TextPosition::new(0, 42)),
            last_opened: None,
            missing: false,
            bookmarks: Vec::new(),
        };
        let row = format_library_row(&book, Some("abc"));
        assert!(row.starts_with("* "));
        assert!(row.contains("Pride and Prejudice"));
        assert!(row.contains("Jane Austen"));
        assert!(row.contains("42%"));
        assert!(row.contains("TXT"));
    }

    #[test]
    fn progress_percent_handles_missing_content_metrics() {
        let book = LibraryBook {
            id: "x".into(),
            title: "x".into(),
            author: None,
            path: PathBuf::from("/x"),
            format: reader::book::BookFormat::Txt,
            content_metrics: reader::book::ContentMetrics::default(),
            progress: Progress::default(),
            last_opened: None,
            missing: false,
            bookmarks: Vec::new(),
        };
        assert_eq!(progress_percent(&book), 0);
    }

    #[test]
    fn progress_percent_caps_at_hundred() {
        let book = LibraryBook {
            id: "x".into(),
            title: "x".into(),
            author: None,
            path: PathBuf::from("/x"),
            format: reader::book::BookFormat::Txt,
            content_metrics: reader::book::ContentMetrics {
                section_char_counts: vec![10],
            },
            progress: Progress::new(reader::book::TextPosition::new(0, 99)),
            last_opened: None,
            missing: false,
            bookmarks: Vec::new(),
        };
        assert_eq!(progress_percent(&book), 100);
    }

    #[test]
    fn copy_to_library_creates_copy_in_target() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.txt");
        let storage = dir.path().join("library");
        std::fs::create_dir(&storage).unwrap();
        std::fs::write(&src, b"hello world").unwrap();

        let copied = copy_to_library_unique(&src, &storage).unwrap();

        assert_eq!(copied, storage.join("source.txt"));
        assert!(copied.exists());
        assert_eq!(std::fs::read(&copied).unwrap(), b"hello world");
        // Source must still exist — this is a copy, not a move.
        assert!(src.exists());
    }

    #[test]
    fn copy_to_library_creates_target_dir_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("book.epub");
        let storage = dir.path().join("nested").join("library");
        std::fs::write(&src, b"epub bytes").unwrap();
        assert!(!storage.exists());

        let copied = copy_to_library_unique(&src, &storage).unwrap();

        assert!(storage.exists());
        assert_eq!(copied, storage.join("book.epub"));
    }

    #[test]
    fn copy_to_library_appends_unique_suffix_on_collision() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().join("library");
        std::fs::create_dir(&storage).unwrap();

        // Pre-populate `book.epub` so the next copy must pick a different name.
        std::fs::write(storage.join("book.epub"), b"original").unwrap();
        let src1 = dir.path().join("book.epub");
        std::fs::write(&src1, b"second").unwrap();

        let first = copy_to_library_unique(&src1, &storage).unwrap();
        assert_eq!(first, storage.join("book (1).epub"));

        // Pre-populate `book (1).epub` so the next collision must try ` (2)`.
        std::fs::write(storage.join("book (1).epub"), b"second placeholder").unwrap();
        let src2 = dir.path().join("book (1).epub");
        std::fs::write(&src2, b"third").unwrap();
        // For src2, parent is the tempdir (not the storage dir), so the
        // no-op check doesn't kick in. The collision logic still applies.
        let second = copy_to_library_unique(&src2, &storage).unwrap();
        assert_eq!(second, storage.join("book (1) (1).epub"));
    }

    #[test]
    fn copy_to_library_is_noop_when_src_already_inside_target() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().join("library");
        std::fs::create_dir(&storage).unwrap();
        let src = storage.join("already-here.txt");
        std::fs::write(&src, b"preexisting").unwrap();

        let result = copy_to_library_unique(&src, &storage).unwrap();

        assert_eq!(result, src);
        // No extra file should have appeared.
        let entries: Vec<_> = std::fs::read_dir(&storage)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);
    }
}
