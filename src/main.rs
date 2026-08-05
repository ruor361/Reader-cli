use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;
use reader::book::load_book;
use reader::bookshelf;
use reader::cli::{BookmarkCommand, Cli, Command, ExportFormat, ShelfCommand};
use reader::config::ReaderConfig;
use reader::reading_data::{ReadingDataStore, ReadingEvent};
use reader::shelf::{run_shelf, ShelfOutcome};
use reader::storage::{LibraryBook, Store};
use reader::tui::run_reader;
use unicode_width::UnicodeWidthStr;

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
    synchronize_reading_data(&mut store, &config)?;

    match cli.command {
        Some(Command::Import { file, open }) => {
            let entry = import_book_from_cli(&mut store, &config, file)?;
            if open {
                open_book_in_reader(entry, &mut store, &mut config)?;
            }
        }
        Some(Command::List) => {
            print_library_table(&store);
        }
        Some(Command::Refresh { dir }) => {
            refresh_bookshelf(&mut store, &mut config, dir)?;
        }
        Some(Command::Shelf { command }) => match command {
            None => loop {
                match run_shelf(&mut store, &mut config, None)? {
                    ShelfOutcome::Exit => break,
                    ShelfOutcome::Open(book) => open_book_in_reader(book, &mut store, &mut config)?,
                }
            },
            Some(ShelfCommand::Refresh { dir }) => {
                refresh_bookshelf(&mut store, &mut config, dir)?;
            }
        },
        Some(Command::Open { book }) => {
            let entry = store.resolve_book(&book)?.clone();
            open_book_in_reader(entry, &mut store, &mut config)?;
        }
        Some(Command::Resume) => {
            let entry = store
                .last_book()?
                .with_context(|| "No recent book. Open or import a book first.")?
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
            println!("Removed: {} ({})", removed.title, short_id(&removed.id));
        }
        Some(Command::Bookmark { command }) => match command {
            BookmarkCommand::Add { book, label } => {
                let entry = store.resolve_book(&book)?.clone();
                let progress = store.progress_for(&entry.id).cloned().unwrap_or_default();
                let bookmark = store.add_bookmark(&entry.id, label, progress)?;
                print_sync_warnings(
                    ReadingDataStore::open_default(&config)?
                        .append_bookmark_add(&entry.id, &bookmark)?,
                );
                store.save()?;
                sync_sidecar_if_configured(&store, &config, &entry.id)?;
                println!(
                    "Bookmark added: {} at {} {} · {}",
                    short_id(&bookmark.id),
                    entry.format.section_label().to_ascii_lowercase(),
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
                        "{}  {} {}  {}  {}",
                        short_id(&bookmark.id),
                        entry.format.section_label().to_ascii_lowercase(),
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
                print_sync_warnings(
                    ReadingDataStore::open_default(&config)?
                        .append_bookmark_delete(&entry.id, &removed.id)?,
                );
                store.save()?;
                sync_sidecar_if_configured(&store, &config, &entry.id)?;
                println!(
                    "Deleted bookmark: {} ({})",
                    removed.label,
                    short_id(&removed.id)
                );
            }
        },
        Some(Command::Export {
            book,
            format,
            output,
        }) => {
            export_reading_events(&store, &config, book.as_deref(), format, output.as_deref())?;
        }
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
                    "{} {}  {}  {}",
                    document.format.section_label().to_ascii_lowercase(),
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
            let entry = import_book_from_cli(&mut store, &config, file)?;
            open_book_in_reader(entry, &mut store, &mut config)?;
        }
    }

    Ok(())
}

fn refresh_bookshelf(
    store: &mut Store,
    config: &mut ReaderConfig,
    dir: Option<PathBuf>,
) -> Result<()> {
    let supplied_dir = dir.is_some();
    let dir = match dir {
        Some(dir) => bookshelf::resolve_bookshelf_dir(&dir)?,
        None => config.bookshelf_dir.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "bookshelf_dir is not configured. Pass it once with \
                 `reader refresh <DIR>` (or `reader shelf refresh <DIR>`)."
            )
        })?,
    };
    let report = bookshelf::refresh(store, &dir)?;
    if supplied_dir {
        config.bookshelf_dir = Some(dir.clone());
        config.save()?;
    }
    synchronize_reading_data(store, config)?;
    store.save()?;
    print_refresh_report(&report);
    Ok(())
}

fn open_book_in_reader(
    mut entry: reader::storage::LibraryBook,
    store: &mut Store,
    config: &mut ReaderConfig,
) -> Result<()> {
    if entry.missing {
        bail!(
            "Book file is missing: {}. Put the file back under bookshelf_dir and run `reader shelf refresh`.",
            entry.path.display()
        );
    }
    if config
        .bookshelf_dir
        .as_deref()
        .is_some_and(|directory| !path_is_in_dir(&entry.path, directory))
    {
        let source = entry.path.clone();
        let outcome = bookshelf::import_managed_book(store, config, &source)?;
        print_sync_warnings(outcome.warnings);
        entry = outcome.book;
    }
    let document = load_book(&entry.path)?;
    let book_id = store.add_book(&document, &entry.path)?.id.clone();
    synchronize_reading_data(store, config)?;
    store.save()?;
    run_reader(document, book_id.clone(), store, config)?;
    sync_sidecar_if_configured(store, config, &book_id)?;
    Ok(())
}

fn import_book_from_cli(
    store: &mut Store,
    config: &ReaderConfig,
    source: PathBuf,
) -> Result<LibraryBook> {
    let source = bookshelf::resolve_input_path(source)?;
    let outcome = bookshelf::import_managed_book(store, config, &source)?;
    print_sync_warnings(outcome.warnings);
    synchronize_reading_data(store, config)?;
    store.save()?;
    sync_sidecar_if_configured(store, config, &outcome.book.id)?;

    let verb = match outcome.disposition {
        bookshelf::ImportDisposition::Copied => "Imported",
        bookshelf::ImportDisposition::Reused => "Already imported",
    };
    println!(
        "{}: {} ({}) -> {}",
        verb,
        outcome.book.title,
        short_id(&outcome.book.id),
        outcome.book.path.display()
    );
    Ok(outcome.book)
}

fn sync_sidecar_if_configured(store: &Store, config: &ReaderConfig, book_id: &str) -> Result<()> {
    if let Some(directory) = config.bookshelf_dir.as_deref() {
        if store
            .book_by_id(book_id)
            .is_some_and(|book| !path_is_in_dir(&book.path, directory))
        {
            return Ok(());
        }
        if let Err(err) = bookshelf::sync_sidecar_for_book(store, book_id, directory) {
            eprintln!(
                "warning: Book state was saved locally but its sidecar could not be updated: {err}"
            );
        }
    }
    Ok(())
}

fn synchronize_reading_data(store: &mut Store, config: &ReaderConfig) -> Result<()> {
    let data = ReadingDataStore::open_default(config)?;
    let report = data.synchronize_store(store)?;
    print_sync_warnings(report.warnings);
    if report.state_changed {
        store.save()?;
    }
    Ok(())
}

fn print_sync_warnings(warnings: Vec<String>) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
}

fn export_reading_events(
    store: &Store,
    config: &ReaderConfig,
    book: Option<&str>,
    format: ExportFormat,
    output: Option<&Path>,
) -> Result<()> {
    let selected_book_id = book
        .map(|query| store.resolve_book(query).map(|book| book.id.clone()))
        .transpose()?;
    let data = ReadingDataStore::open_default(config)?;
    let sync_report = data.reconcile()?;
    print_sync_warnings(sync_report.warnings);
    let stored = data.events_strict()?;
    let events = stored
        .into_iter()
        .filter(|stored| {
            selected_book_id
                .as_deref()
                .is_none_or(|book_id| stored.event.book_id == book_id)
        })
        .map(|stored| stored.event)
        .collect::<Vec<_>>();

    let mut rendered = match format {
        ExportFormat::Json => serde_json::to_string_pretty(&events)?,
        ExportFormat::Csv => render_events_csv(&events)?,
    };
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    match output {
        Some(path) => ReadingDataStore::write_export(path, rendered.as_bytes())?,
        None => {
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            stdout.write_all(rendered.as_bytes())?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn render_events_csv(events: &[ReadingEvent]) -> Result<String> {
    use time::format_description::well_known::Rfc3339;

    let mut output = String::from(
        "book_id,started_at,ended_at,start_section_index,start_char_offset,end_section_index,end_char_offset\n",
    );
    for event in events {
        let started = event.started_at.format(&Rfc3339)?;
        let ended = event.ended_at.format(&Rfc3339)?;
        output.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv_escape(&event.book_id),
            csv_escape(&started),
            csv_escape(&ended),
            event.start_position.section_index,
            event.start_position.char_offset,
            event.end_position.section_index,
            event.end_position.char_offset,
        ));
    }
    Ok(output)
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
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
        println!("No books in library. Import one with `reader import <file>`.");
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
    let title = fit_column(&book.title, 30);
    let author = book
        .author
        .as_deref()
        .map(|value| fit_column(value, 16))
        .unwrap_or_else(|| fit_column("-", 16));
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
    let format = book.format.label();
    let status = if book.missing { "missing" } else { "ok" };
    format!(
        "{:<2} {}  {}  {:>4}  {:<10}  {:>4}  {:<5}  {:<7}",
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
    if text.width() <= max {
        text.to_string()
    } else if max == 0 {
        String::new()
    } else if max == 1 {
        "\u{2026}".to_string()
    } else {
        let mut out = String::new();
        for ch in text.chars() {
            let mut candidate = out.clone();
            candidate.push(ch);
            candidate.push('\u{2026}');
            if candidate.width() > max {
                break;
            }
            out.push(ch);
        }
        out.push('\u{2026}');
        out
    }
}

fn fit_column(text: &str, width: usize) -> String {
    let mut fitted = truncate_str(text, width);
    fitted.push_str(&" ".repeat(width.saturating_sub(fitted.width())));
    fitted
}

// Discreet mode only affects what the TUI renders. CLI errors and panics
// are always printed in full so the user can debug regardless of mode.
fn render_error(err: &anyhow::Error) -> String {
    format!("Error: {err:?}")
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
    fn error_output_always_keeps_real_error_details() {
        // Discreet mode used to swap real errors for a fake JS stack; that
        // suppressed useful diagnostics. The new contract is: render_error
        // always returns the real error, regardless of any discreet setting.
        let err = anyhow::anyhow!("Book not found: secret-title");

        let output = render_error(&err);

        assert!(output.contains("Book not found: secret-title"));
        assert!(!output.contains("Uncaught TypeError"));
        assert!(!output.contains("DiscreetOverlay.render"));
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
    fn csv_export_flattens_positions_and_escapes_values() {
        let event = ReadingEvent {
            book_id: "book,one".into(),
            started_at: time::OffsetDateTime::UNIX_EPOCH,
            ended_at: time::OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(2),
            start_position: reader::book::TextPosition::new(1, 2),
            end_position: reader::book::TextPosition::new(3, 4),
        };

        let csv = render_events_csv(&[event]).unwrap();

        assert!(csv.starts_with("book_id,started_at,ended_at"));
        assert!(csv.contains("\"book,one\",1970-01-01T00:00:00Z"));
        assert!(csv.ends_with(",1,2,3,4\n"));
    }
}
