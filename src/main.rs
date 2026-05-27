use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use reader::book::load_book;
use reader::cli::{BookmarkCommand, Cli, Command};
use reader::storage::Store;
use reader::tui::run_reader;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut store = Store::open_default()?;

    match cli.command {
        Some(Command::Add { file }) => {
            let file = resolve_input_path(file)?;
            let document = load_book(&file)?;
            let book = store.add_book(&document, file)?;
            let title = book.title.clone();
            let id = book.id.clone();
            store.save()?;
            println!("Added: {} ({})", title, id);
        }
        Some(Command::List) => {
            for book in store.library().books() {
                let marker = if store.library().last_book_id.as_deref() == Some(book.id.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{marker} {}  {}", book.id, book.title);
            }
        }
        Some(Command::Open { book }) => {
            let entry = store.resolve_book(&book)?.clone();
            let document = load_book(&entry.path)?;
            run_reader(document, entry.id, &mut store)?;
        }
        Some(Command::Resume) => {
            let entry = store
                .last_book()?
                .with_context(|| "No recent book. Open or add a book first.")?
                .clone();
            let document = load_book(&entry.path)?;
            run_reader(document, entry.id, &mut store)?;
        }
        Some(Command::Remove { book }) => {
            let removed = store.remove_book(&book)?;
            store.save()?;
            println!("Removed: {} ({})", removed.title, removed.id);
        }
        Some(Command::Bookmark { command }) => match command {
            BookmarkCommand::Add { book, label } => {
                let entry = store.resolve_book(&book)?.clone();
                let progress = store.progress_for(&entry.id).cloned().unwrap_or_default();
                let bookmark = store.add_bookmark(&entry.id, label, progress)?;
                store.save()?;
                println!("Bookmark added: {} at page {}", bookmark.id, bookmark.page + 1);
            }
            BookmarkCommand::List { book } => {
                let entry = store.resolve_book(&book)?;
                for bookmark in store.bookmarks_for(&entry.id) {
                    println!(
                        "{}  page {}  {}",
                        bookmark.id,
                        bookmark.page + 1,
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
                let document = load_book(&entry.path)?;
                run_reader(document, entry.id, &mut store)?;
            }
            BookmarkCommand::Delete { book, bookmark } => {
                let entry = store.resolve_book(&book)?.clone();
                let removed = store.delete_bookmark(&entry.id, &bookmark)?;
                store.save()?;
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
            for result in results.into_iter().take(25) {
                println!(
                    "section {}:{}  {}",
                    result.section_index + 1,
                    result.byte_offset,
                    result.preview
                );
            }
        }
        None => {
            let file = cli
                .file_path()
                .with_context(|| "Pass a book path or use a subcommand. Try `reader --help`.")?;
            let file = resolve_input_path(file)?;
            let document = load_book(&file)?;
            let book = store.add_book(&document, file)?;
            let id = book.id.clone();
            store.save()?;
            run_reader(document, id, &mut store)?;
        }
    }

    Ok(())
}

fn resolve_input_path(path: PathBuf) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path);
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(path);
    };
    let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
