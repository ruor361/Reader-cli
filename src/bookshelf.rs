use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::storage::{decode_book_value, LibraryBook, Store, LIBRARY_SCHEMA_VERSION};

#[derive(serde::Serialize, serde::Deserialize)]
struct Sidecar {
    schema_version: u32,
    book: LibraryBook,
}

/// Return the sidecar `.json` path for a book file: `foo.epub` -> `foo.epub.json`.
pub fn sidecar_path(book_path: &Path) -> PathBuf {
    let mut name = book_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".json");
    book_path.with_file_name(name)
}

pub fn write_sidecar(book: &LibraryBook) -> Result<()> {
    let path = sidecar_path(&book.path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&Sidecar {
        schema_version: LIBRARY_SCHEMA_VERSION,
        book: book.clone(),
    })
    .with_context(|| format!("Failed to serialize sidecar for {}", book.path.display()))?;
    fs::write(&path, text)
        .with_context(|| format!("Failed to write sidecar {}", path.display()))?;
    Ok(())
}

pub fn read_sidecar(book_path: &Path) -> Result<Option<LibraryBook>> {
    let path = sidecar_path(book_path);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read sidecar {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("Invalid sidecar JSON: {}", path.display()))?;
    let book = if let Some(book) = value.get("book") {
        decode_book_value(book.clone())
    } else {
        decode_book_value(value)
    }
    .with_context(|| format!("Invalid sidecar record: {}", path.display()))?;
    Ok(Some(book))
}

pub fn remove_sidecar(book_path: &Path) -> Result<()> {
    let path = sidecar_path(book_path);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove sidecar {}", path.display()))?;
    }
    Ok(())
}

pub fn remove_sidecar_for_book(book: &LibraryBook) -> Result<()> {
    remove_sidecar(&book.path)
}

pub fn sync_sidecar_for_book(store: &Store, book_id: &str) -> Result<()> {
    if let Some(book) = store.book_by_id(book_id) {
        write_sidecar(book)?;
    }
    Ok(())
}

#[derive(Debug, Default, Clone)]
pub struct RefreshReport {
    pub sidecars_written: usize,
    pub sidecars_removed: usize,
    pub books_added: usize,
    pub books_updated: usize,
    pub books_missing: usize,
    pub books_restored: usize,
    pub books_removed: usize,
    pub errors: Vec<(PathBuf, String)>,
}

pub fn refresh(store: &mut Store, bookshelf_dir: &Path) -> Result<RefreshReport> {
    let mut report = RefreshReport::default();
    if !bookshelf_dir.exists() {
        return Ok(report);
    }

    let mut book_files = Vec::new();
    let mut sidecars = Vec::new();
    collect_bookshelf_files(bookshelf_dir, &mut book_files, &mut sidecars, &mut report);

    for book_path in &book_files {
        let previous = store.book_by_path(book_path).cloned();
        let restored = if previous.is_none() {
            match read_sidecar(book_path) {
                Ok(book) => book,
                Err(err) => {
                    report
                        .errors
                        .push((sidecar_path(book_path), err.to_string()));
                    None
                }
            }
        } else {
            None
        };
        let document = match crate::book::load_book(book_path) {
            Ok(document) => document,
            Err(err) => {
                report.errors.push((book_path.clone(), err.to_string()));
                continue;
            }
        };

        match store.add_book(&document, book_path) {
            Ok(book) => {
                let book_id = book.id.clone();
                if let Some(restored) = restored {
                    store.restore_reading_state(&book_id, restored);
                }
                match previous {
                    None => report.books_added += 1,
                    Some(previous) if previous.missing => report.books_restored += 1,
                    Some(_) => report.books_updated += 1,
                }
                let book = store
                    .book_by_id(&book_id)
                    .expect("book was added immediately above");
                match write_sidecar(book) {
                    Ok(()) => report.sidecars_written += 1,
                    Err(err) => report.errors.push((book_path.clone(), err.to_string())),
                }
            }
            Err(err) => report.errors.push((book_path.clone(), err.to_string())),
        }
    }

    let missing_ids: Vec<String> = store
        .library()
        .books()
        .iter()
        .filter(|book| path_is_in_dir(&book.path, bookshelf_dir))
        .filter(|book| !book.path.exists())
        .filter(|book| !book.missing)
        .map(|book| book.id.clone())
        .collect();

    for id in missing_ids {
        if store.mark_missing(&id, true) {
            report.books_missing += 1;
            if let Some(book) = store.book_by_id(&id) {
                match write_sidecar(book) {
                    Ok(()) => report.sidecars_written += 1,
                    Err(err) => report.errors.push((book.path.clone(), err.to_string())),
                }
            }
        }
    }

    for sidecar in &sidecars {
        let Some(stem) = sidecar.file_stem() else {
            continue;
        };
        let book_path = sidecar.with_file_name(stem);
        if book_path.exists() {
            continue;
        }
        if store
            .library()
            .books()
            .iter()
            .any(|book| paths_match(&sidecar_path(&book.path), sidecar))
        {
            continue;
        }
        match fs::remove_file(sidecar) {
            Ok(()) => report.sidecars_removed += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => report.errors.push((sidecar.clone(), err.to_string())),
        }
    }

    Ok(report)
}

fn collect_bookshelf_files(
    dir: &Path,
    book_files: &mut Vec<PathBuf>,
    sidecars: &mut Vec<PathBuf>,
    report: &mut RefreshReport,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            report.errors.push((dir.to_path_buf(), err.to_string()));
            return;
        }
    };

    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_bookshelf_files(&path, book_files, sidecars, report);
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if ext == "epub" || ext == "txt" {
            book_files.push(path);
        } else if ext == "json" {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.ends_with(".epub") || stem.ends_with(".txt") {
                    sidecars.push(path);
                }
            }
        }
    }
}

fn path_is_in_dir(path: &Path, dir: &Path) -> bool {
    if path.starts_with(dir) {
        return true;
    }
    if let Ok(dir) = dir.canonicalize() {
        if path.starts_with(dir) {
            return true;
        }
    }
    if let Ok(path) = path.canonicalize() {
        if path.starts_with(dir) {
            return true;
        }
        if let Ok(dir) = dir.canonicalize() {
            return path.starts_with(dir);
        }
    }
    false
}

fn paths_match(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookDocument, BookFormat, BookMetadata, Section};

    fn make_book(id: &str, title: &str, path: PathBuf) -> LibraryBook {
        LibraryBook {
            id: id.into(),
            title: title.into(),
            author: Some("Tester".into()),
            path,
            format: BookFormat::Txt,
            content_metrics: crate::book::ContentMetrics {
                section_char_counts: vec![10],
            },
            progress: crate::storage::Progress::default(),
            last_opened: None,
            missing: false,
            bookmarks: Vec::new(),
        }
    }

    fn make_document(id: &str, title: &str, path: &Path) -> BookDocument {
        BookDocument {
            id: id.into(),
            title: title.into(),
            path: path.to_path_buf(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: title.into(),
                author: Some("Tester".into()),
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path: path.to_path_buf(),
                section_count: 1,
            },
            sections: vec![Section {
                title: "A".into(),
                text: "hello".into(),
            }],
        }
    }

    #[test]
    fn sidecar_path_appends_json_extension() {
        assert_eq!(
            sidecar_path(Path::new("/shelf/foo.epub")),
            PathBuf::from("/shelf/foo.epub.json")
        );
        assert_eq!(
            sidecar_path(Path::new("/shelf/foo.txt")),
            PathBuf::from("/shelf/foo.txt.json")
        );
    }

    #[test]
    fn write_and_read_sidecar_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let book = make_book("abc", "Roundtrip", dir.path().join("foo.txt"));

        write_sidecar(&book).unwrap();
        let read = read_sidecar(&book.path).unwrap().unwrap();

        assert_eq!(read.id, book.id);
        assert_eq!(read.title, book.title);
        assert_eq!(read.missing, book.missing);
    }

    #[test]
    fn refresh_migrates_legacy_sidecar_and_restores_reading_state() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let book_path = shelf.join("legacy.txt");
        fs::write(&book_path, "abcdefghij").unwrap();
        let legacy = serde_json::json!({
            "id": "legacy-id",
            "title": "Legacy",
            "path": book_path,
            "format": "Txt",
            "progress": {"page": 5, "section_index": 0, "start_line": 3},
            "page_count": 11,
            "bookmarks": [{
                "id": "bm-1", "label": "kept", "page": 5,
                "section_index": 0, "start_line": 3
            }]
        });
        fs::write(
            sidecar_path(&book_path),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let report = refresh(&mut store, &shelf).unwrap();

        assert_eq!(report.books_added, 1);
        let restored = &store.library().books()[0];
        assert_eq!(restored.progress.position.char_offset, 5);
        assert_eq!(restored.bookmarks[0].position.char_offset, 5);
        assert_eq!(restored.bookmarks[0].label, "kept");
        let rewritten: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(sidecar_path(&book_path)).unwrap()).unwrap();
        assert_eq!(rewritten["schema_version"], LIBRARY_SCHEMA_VERSION);
        assert!(rewritten["book"]["progress"]["position"].is_object());
    }

    #[test]
    fn read_sidecar_returns_none_for_missing_file() {
        let result = read_sidecar(Path::new("/nonexistent/shelf/foo.epub")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn refresh_recursively_adds_book_with_no_library_entry() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        let nested = shelf.join("authors").join("tester");
        fs::create_dir_all(&nested).unwrap();
        let book_path = nested.join("foo.txt");
        fs::write(&book_path, b"hello world").unwrap();

        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let report = refresh(&mut store, &shelf).unwrap();

        assert_eq!(report.books_added, 1);
        assert_eq!(report.sidecars_written, 1);
        assert_eq!(store.library().books().len(), 1);
        assert!(sidecar_path(&book_path).exists());
    }

    #[test]
    fn refresh_is_idempotent_for_existing_book() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let book_path = shelf.join("foo.txt");
        fs::write(&book_path, b"hello world").unwrap();

        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let first = refresh(&mut store, &shelf).unwrap();
        let second = refresh(&mut store, &shelf).unwrap();

        assert_eq!(first.books_added, 1);
        assert_eq!(second.books_added, 0);
        assert_eq!(second.books_updated, 1);
        assert_eq!(store.library().books().len(), 1);
    }

    #[test]
    fn refresh_marks_missing_instead_of_removing_library_entry() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let book_path = shelf.join("foo.txt");
        fs::write(&book_path, b"hello world").unwrap();

        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let document = make_document("foo-id", "Foo", &book_path);
        let book = store.add_book(&document, &book_path).unwrap().clone();
        store.set_progress(
            &book.id,
            crate::storage::Progress::new(crate::book::TextPosition::new(0, 4)),
        );
        write_sidecar(&book).unwrap();
        fs::remove_file(&book_path).unwrap();

        let report = refresh(&mut store, &shelf).unwrap();

        let book = store.resolve_book("Foo").unwrap();
        assert_eq!(report.books_missing, 1);
        assert!(book.missing);
        assert_eq!(book.progress.position.char_offset, 4);
        assert!(sidecar_path(&book_path).exists());
    }

    #[test]
    fn refresh_restores_missing_book_when_file_reappears() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let book_path = shelf.join("foo.txt");
        fs::write(&book_path, b"hello world").unwrap();

        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        refresh(&mut store, &shelf).unwrap();
        let id = store.library().books()[0].id.clone();
        fs::remove_file(&book_path).unwrap();
        refresh(&mut store, &shelf).unwrap();
        assert!(store.book_by_id(&id).unwrap().missing);

        fs::write(&book_path, b"hello world").unwrap();
        let report = refresh(&mut store, &shelf).unwrap();

        assert_eq!(report.books_restored, 1);
        assert!(!store.book_by_id(&id).unwrap().missing);
    }

    #[test]
    fn refresh_removes_orphan_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let orphan = shelf.join("ghost.epub.json");
        fs::write(&orphan, "{}").unwrap();

        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let report = refresh(&mut store, &shelf).unwrap();

        assert_eq!(report.sidecars_removed, 1);
        assert!(!orphan.exists());
    }

    #[test]
    fn refresh_keeps_unrelated_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let unrelated = shelf.join("notes.json");
        fs::write(&unrelated, "{\"x\":1}").unwrap();

        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let _ = refresh(&mut store, &shelf).unwrap();

        assert!(unrelated.exists());
    }
}
