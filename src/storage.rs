use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};

use crate::book::{
    author_from_txt_path, is_supported_book_path, load_book, BookDocument, BookFormat,
    ContentMetrics, TextPosition,
};

pub const LIBRARY_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    library: Library,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Library {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub books: Vec<LibraryBook>,
    #[serde(default)]
    pub last_book_id: Option<String>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: LIBRARY_SCHEMA_VERSION,
            books: Vec::new(),
            last_book_id: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryBook {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    pub path: PathBuf,
    pub format: BookFormat,
    #[serde(default)]
    pub content_metrics: ContentMetrics,
    #[serde(default)]
    pub progress: Progress,
    #[serde(default)]
    pub last_opened: Option<SystemTime>,
    #[serde(default)]
    pub missing: bool,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl LibraryBook {
    pub fn progress_tenths(&self) -> u16 {
        self.content_metrics.progress_tenths(self.progress.position)
    }

    pub fn progress_percent(&self) -> u16 {
        (self.progress_tenths() + 5) / 10
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    pub position: TextPosition,
}

impl Progress {
    pub const fn new(position: TextPosition) -> Self {
        Self { position }
    }
}

impl From<TextPosition> for Progress {
    fn from(position: TextPosition) -> Self {
        Self::new(position)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub label: String,
    pub position: TextPosition,
}

impl Bookmark {
    pub const fn progress(&self) -> Progress {
        Progress::new(self.position)
    }
}

impl Library {
    pub fn books(&self) -> &[LibraryBook] {
        &self.books
    }
}

impl Store {
    pub fn open_default() -> Result<Self> {
        Self::open(crate::paths::library_path()?)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let library = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            match decode_library(&text) {
                Ok(library) => library,
                Err(err) => {
                    eprintln!(
                        "warning: {} is corrupt ({err}); initializing a fresh library. \
                         The old file was preserved as {}.corrupt.",
                        path.display(),
                        path.display(),
                    );
                    let backup = path.with_extension("json.corrupt");
                    let _ = std::fs::rename(&path, &backup);
                    Library::default()
                }
            }
        } else {
            Library::default()
        };
        Ok(Self { path, library })
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(&self.library)?;
        crate::atomic_file::write(&self.path, text.as_bytes())
            .with_context(|| format!("Failed to save {}", self.path.display()))?;
        Ok(())
    }

    pub fn add_book(
        &mut self,
        document: &BookDocument,
        path: impl AsRef<Path>,
    ) -> Result<&LibraryBook> {
        let canonical = path
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| path.as_ref().to_path_buf());
        let author = document.metadata.author.clone().or_else(|| {
            (document.format == BookFormat::Txt)
                .then(|| author_from_txt_path(path.as_ref()))
                .flatten()
        });
        let content_metrics = document.content_metrics();

        if let Some(index) = self
            .library
            .books
            .iter()
            .position(|book| book.id == document.id)
        {
            let book = &mut self.library.books[index];
            book.title = document.title.clone();
            book.path = canonical;
            book.format = document.format;
            book.missing = false;
            if book.author.is_none() {
                book.author = author;
            }
            apply_metrics(book, content_metrics);
            return Ok(&self.library.books[index]);
        }

        if let Some(index) = self.library.books.iter().position(|book| {
            book.path == canonical
                || book
                    .path
                    .canonicalize()
                    .ok()
                    .is_some_and(|existing| existing == canonical)
        }) {
            let previous_id = self.library.books[index].id.clone();
            let book = &mut self.library.books[index];
            book.id = document.id.clone();
            book.title = document.title.clone();
            book.path = canonical;
            book.format = document.format;
            book.missing = false;
            if book.author.is_none() {
                book.author = author;
            }
            apply_metrics(book, content_metrics);
            if self.library.last_book_id.as_deref() == Some(previous_id.as_str()) {
                self.library.last_book_id = Some(document.id.clone());
            }
            return Ok(&self.library.books[index]);
        }

        self.library.books.push(LibraryBook {
            id: document.id.clone(),
            title: document.title.clone(),
            author,
            path: canonical,
            format: document.format,
            content_metrics,
            progress: Progress::default(),
            last_opened: None,
            missing: false,
            bookmarks: Vec::new(),
        });
        Ok(self.library.books.last().expect("book was just inserted"))
    }

    pub fn update_document_metrics(&mut self, book_id: &str, document: &BookDocument) {
        if let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        {
            apply_metrics(book, document.content_metrics());
        }
    }

    pub fn restore_reading_state(&mut self, book_id: &str, restored: LibraryBook) {
        let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        else {
            return;
        };
        if book.author.is_none() {
            book.author = restored.author;
        }
        book.progress = Progress::new(book.content_metrics.clamp(restored.progress.position));
        book.bookmarks = restored
            .bookmarks
            .into_iter()
            .map(|mut bookmark| {
                bookmark.position = book.content_metrics.clamp(bookmark.position);
                bookmark
            })
            .collect();
        book.last_opened = restored.last_opened;
    }

    pub fn merge_legacy_record(&mut self, legacy_id: &str, current_id: &str) {
        if legacy_id == current_id {
            return;
        }
        let Some(legacy_index) = self
            .library
            .books
            .iter()
            .position(|book| book.id == legacy_id)
        else {
            return;
        };
        let legacy = self.library.books.remove(legacy_index);
        let Some(current) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == current_id)
        else {
            self.library.books.insert(legacy_index, legacy);
            return;
        };

        if legacy.last_opened > current.last_opened {
            current.progress =
                Progress::new(current.content_metrics.clamp(legacy.progress.position));
            current.last_opened = legacy.last_opened;
        }
        if current.author.is_none() {
            current.author = legacy.author;
        }
        for mut bookmark in legacy.bookmarks {
            if current
                .bookmarks
                .iter()
                .any(|existing| existing.id == bookmark.id)
            {
                continue;
            }
            bookmark.position = current.content_metrics.clamp(bookmark.position);
            current.bookmarks.push(bookmark);
        }
        if self.library.last_book_id.as_deref() == Some(legacy_id) {
            self.library.last_book_id = Some(current_id.to_string());
        }
    }

    pub fn record_open(&mut self, book_id: &str) {
        if let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        {
            book.last_opened = Some(SystemTime::now());
            self.library.last_book_id = Some(book_id.to_string());
        }
    }

    pub fn resolve_book(&self, query: &str) -> Result<&LibraryBook> {
        let needle = query.to_lowercase();
        if let Some(book) = self.library.books.iter().find(|book| book.id == query) {
            return Ok(book);
        }
        let matches: Vec<&LibraryBook> = self
            .library
            .books
            .iter()
            .filter(|book| {
                book.id.starts_with(query) || book.title.to_lowercase().contains(&needle)
            })
            .collect();

        match matches.as_slice() {
            [book] => Ok(book),
            [] => bail!("Book not found: {query}"),
            _ => bail!("Book query is ambiguous: {query}"),
        }
    }

    pub fn book_by_id(&self, book_id: &str) -> Option<&LibraryBook> {
        self.library.books.iter().find(|book| book.id == book_id)
    }

    pub fn book_by_path(&self, path: &Path) -> Option<&LibraryBook> {
        let canonical = path.canonicalize().ok();
        self.library.books.iter().find(|book| {
            if let (Some(left), Some(right)) = (book.path.canonicalize().ok(), canonical.as_ref()) {
                left == *right
            } else {
                book.path == path
            }
        })
    }

    pub fn mark_missing(&mut self, book_id: &str, missing: bool) -> bool {
        let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        else {
            return false;
        };
        if book.missing == missing {
            return false;
        }
        book.missing = missing;
        true
    }

    pub fn last_book(&self) -> Result<Option<&LibraryBook>> {
        let Some(id) = self.library.last_book_id.as_deref() else {
            return Ok(None);
        };
        Ok(self.library.books.iter().find(|book| book.id == id))
    }

    pub fn remove_book(&mut self, query: &str) -> Result<LibraryBook> {
        let id = self.resolve_book(query)?.id.clone();
        let index = self
            .library
            .books
            .iter()
            .position(|book| book.id == id)
            .with_context(|| format!("Book not found: {query}"))?;
        let removed = self.library.books.remove(index);
        if self.library.last_book_id.as_deref() == Some(removed.id.as_str()) {
            self.library.last_book_id = self.library.books.first().map(|book| book.id.clone());
        }
        Ok(removed)
    }

    pub fn progress_for(&self, book_id: &str) -> Option<&Progress> {
        self.book_by_id(book_id).map(|book| &book.progress)
    }

    pub fn progress_tenths_for(&self, book_id: &str) -> Option<u16> {
        self.book_by_id(book_id).map(LibraryBook::progress_tenths)
    }

    pub fn set_progress(&mut self, book_id: &str, progress: Progress) {
        if let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        {
            book.progress = Progress::new(book.content_metrics.clamp(progress.position));
            book.missing = false;
        }
    }

    pub fn apply_synced_progress(
        &mut self,
        book_id: &str,
        progress: Progress,
        last_opened: SystemTime,
    ) -> bool {
        let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        else {
            return false;
        };
        let progress = Progress::new(book.content_metrics.clamp(progress.position));
        let changed = book.progress != progress || book.last_opened != Some(last_opened);
        book.progress = progress;
        book.last_opened = Some(last_opened);
        changed
    }

    pub fn set_last_book_id(&mut self, book_id: &str) -> bool {
        if !self.library.books.iter().any(|book| book.id == book_id)
            || self.library.last_book_id.as_deref() == Some(book_id)
        {
            return false;
        }
        self.library.last_book_id = Some(book_id.to_string());
        true
    }

    pub fn add_bookmark(
        &mut self,
        book_id: &str,
        label: String,
        progress: Progress,
    ) -> Result<Bookmark> {
        let book = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
            .with_context(|| format!("Book not found: {book_id}"))?;
        let bookmark = Bookmark {
            id: uuid::Uuid::new_v4().to_string(),
            label,
            position: book.content_metrics.clamp(progress.position),
        };
        book.bookmarks.push(bookmark.clone());
        Ok(bookmark)
    }

    pub fn bookmarks_for(&self, book_id: &str) -> &[Bookmark] {
        self.book_by_id(book_id)
            .map(|book| book.bookmarks.as_slice())
            .unwrap_or(&[])
    }

    pub fn bookmark_for(&self, book_id: &str, bookmark_id: &str) -> Option<&Bookmark> {
        let bookmarks = self.bookmarks_for(book_id);
        if let Some(bookmark) = bookmarks.iter().find(|bookmark| bookmark.id == bookmark_id) {
            return Some(bookmark);
        }
        let mut matches = bookmarks
            .iter()
            .filter(|bookmark| bookmark.id.starts_with(bookmark_id));
        let bookmark = matches.next()?;
        matches.next().is_none().then_some(bookmark)
    }

    pub fn delete_bookmark(&mut self, book_id: &str, bookmark_id: &str) -> Result<Bookmark> {
        let book = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
            .with_context(|| format!("Book not found: {book_id}"))?;
        let exact = book
            .bookmarks
            .iter()
            .position(|bookmark| bookmark.id == bookmark_id);
        let index = match exact {
            Some(index) => index,
            None => {
                let matches = book
                    .bookmarks
                    .iter()
                    .enumerate()
                    .filter(|(_, bookmark)| bookmark.id.starts_with(bookmark_id))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [index] => *index,
                    [] => bail!("Bookmark not found: {bookmark_id}"),
                    _ => bail!("Bookmark id is ambiguous: {bookmark_id}"),
                }
            }
        };
        Ok(book.bookmarks.remove(index))
    }

    pub fn merge_bookmark(&mut self, book_id: &str, bookmark: Bookmark) -> bool {
        let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        else {
            return false;
        };
        let mut bookmark = bookmark;
        bookmark.position = book.content_metrics.clamp(bookmark.position);
        if let Some(existing) = book
            .bookmarks
            .iter_mut()
            .find(|existing| existing.id == bookmark.id)
        {
            if *existing == bookmark {
                return false;
            }
            *existing = bookmark;
            return true;
        }
        book.bookmarks.push(bookmark);
        true
    }

    pub fn remove_synced_bookmark(&mut self, book_id: &str, bookmark_id: &str) -> bool {
        let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        else {
            return false;
        };
        let before = book.bookmarks.len();
        book.bookmarks.retain(|bookmark| bookmark.id != bookmark_id);
        before != book.bookmarks.len()
    }

    pub fn scan_lib_dir(&mut self, dir: &Path) -> ScanDirResult {
        let mut result = ScanDirResult::default();
        let Some(entries) = std::fs::read_dir(dir).ok() else {
            return result;
        };
        let existing_paths: HashSet<_> = self
            .library
            .books
            .iter()
            .filter_map(|book| book.path.canonicalize().ok())
            .collect();

        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if !is_supported_book_path(&path) {
                continue;
            }
            if path
                .canonicalize()
                .ok()
                .is_some_and(|canonical| existing_paths.contains(&canonical))
            {
                continue;
            }

            let document = match load_book(&path) {
                Ok(document) => document,
                Err(err) => {
                    result.errors.push((path, err.to_string()));
                    continue;
                }
            };
            let is_new = !self.library.books.iter().any(|book| book.id == document.id);
            if let Err(err) = self.add_book(&document, &path) {
                result.errors.push((path, err.to_string()));
            } else if is_new {
                result.imported += 1;
            } else {
                result.updated += 1;
            }
        }

        if result.imported > 0 || result.updated > 0 {
            let _ = self.save();
        }
        result
    }
}

fn current_schema_version() -> u32 {
    LIBRARY_SCHEMA_VERSION
}

fn apply_metrics(book: &mut LibraryBook, metrics: ContentMetrics) {
    book.progress = Progress::new(metrics.clamp(book.progress.position));
    for bookmark in &mut book.bookmarks {
        bookmark.position = metrics.clamp(bookmark.position);
    }
    book.content_metrics = metrics;
}

fn decode_library(text: &str) -> serde_json::Result<Library> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    if version >= 2 {
        let mut library: Library = serde_json::from_value(value)?;
        library.schema_version = LIBRARY_SCHEMA_VERSION;
        normalize_library_positions(&mut library);
        Ok(library)
    } else {
        let legacy: LegacyLibrary = serde_json::from_value(value)?;
        Ok(migrate_legacy_library(legacy))
    }
}

pub(crate) fn decode_book_value(value: serde_json::Value) -> serde_json::Result<LibraryBook> {
    let is_v2 = value
        .get("progress")
        .and_then(|progress| progress.get("position"))
        .is_some()
        || value.get("content_metrics").is_some();
    let mut book = if is_v2 {
        serde_json::from_value(value)
    } else {
        let legacy: LegacyLibraryBook = serde_json::from_value(value)?;
        Ok(migrate_legacy_book(legacy))
    }?;
    normalize_book_positions(&mut book);
    Ok(book)
}

fn normalize_library_positions(library: &mut Library) {
    for book in &mut library.books {
        normalize_book_positions(book);
    }
}

fn normalize_book_positions(book: &mut LibraryBook) {
    if book.content_metrics.section_char_counts.is_empty() {
        return;
    }
    book.progress.position = book.content_metrics.clamp(book.progress.position);
    for bookmark in &mut book.bookmarks {
        bookmark.position = book.content_metrics.clamp(bookmark.position);
    }
}

fn migrate_legacy_library(legacy: LegacyLibrary) -> Library {
    Library {
        schema_version: LIBRARY_SCHEMA_VERSION,
        books: legacy.books.into_iter().map(migrate_legacy_book).collect(),
        last_book_id: legacy.last_book_id,
    }
}

fn migrate_legacy_book(legacy: LegacyLibraryBook) -> LibraryBook {
    let document = load_book(&legacy.path).ok();
    let metrics = document
        .as_ref()
        .map(BookDocument::content_metrics)
        .unwrap_or_default();
    let progress = migrate_legacy_position(legacy.progress, legacy.page_count, &metrics);
    let bookmarks = legacy
        .bookmarks
        .into_iter()
        .map(|bookmark| Bookmark {
            id: bookmark.id,
            label: bookmark.label,
            position: migrate_legacy_position(
                LegacyProgress {
                    page: bookmark.page,
                    section_index: bookmark.section_index,
                    start_line: bookmark.start_line,
                },
                legacy.page_count,
                &metrics,
            )
            .position,
        })
        .collect();

    LibraryBook {
        id: legacy.id,
        title: legacy.title,
        author: legacy.author,
        path: legacy.path,
        format: legacy.format,
        content_metrics: metrics,
        progress,
        last_opened: legacy.last_opened,
        missing: legacy.missing,
        bookmarks,
    }
}

fn migrate_legacy_position(
    legacy: LegacyProgress,
    page_count: Option<usize>,
    metrics: &ContentMetrics,
) -> Progress {
    if !metrics.section_char_counts.is_empty() {
        if let Some(page_count) = page_count.filter(|count| *count > 1) {
            let page = legacy.page.min(page_count - 1);
            let tenths = ((page as u128 * 1000) / (page_count - 1) as u128) as u16;
            return Progress::new(metrics.position_at_tenths(tenths));
        }
        return Progress::new(metrics.clamp(TextPosition::new(legacy.section_index, 0)));
    }
    Progress::new(TextPosition::new(legacy.section_index, 0))
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct LegacyLibrary {
    #[serde(default)]
    books: Vec<LegacyLibraryBook>,
    #[serde(default)]
    last_book_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LegacyLibraryBook {
    id: String,
    title: String,
    #[serde(default)]
    author: Option<String>,
    path: PathBuf,
    format: BookFormat,
    #[serde(default)]
    progress: LegacyProgress,
    #[serde(default)]
    page_count: Option<usize>,
    #[serde(default)]
    last_opened: Option<SystemTime>,
    #[serde(default)]
    missing: bool,
    #[serde(default)]
    bookmarks: Vec<LegacyBookmark>,
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
struct LegacyProgress {
    #[serde(default)]
    page: usize,
    #[serde(default)]
    section_index: usize,
    #[serde(default)]
    #[allow(dead_code)]
    start_line: usize,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LegacyBookmark {
    id: String,
    label: String,
    #[serde(default)]
    page: usize,
    #[serde(default)]
    section_index: usize,
    #[serde(default)]
    start_line: usize,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScanDirResult {
    pub imported: usize,
    pub updated: usize,
    pub errors: Vec<(PathBuf, String)>,
}

impl ScanDirResult {
    pub fn merge(&mut self, other: ScanDirResult) {
        self.imported += other.imported;
        self.updated += other.updated;
        self.errors.extend(other.errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookMetadata, Section};

    fn document(id: &str, title: &str, path: PathBuf) -> BookDocument {
        let sections = vec![Section {
            title: "One".into(),
            text: "alpha beta gamma delta".into(),
        }];
        BookDocument {
            id: id.into(),
            title: title.into(),
            path: path.clone(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: title.into(),
                author: None,
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path,
                section_count: sections.len(),
            },
            sections,
            contents: vec![crate::book::ContentsEntry {
                title: "One".into(),
                depth: 0,
                target: TextPosition::new(0, 0),
            }],
            contents_source: crate::book::ContentsSource::Generated,
        }
    }

    #[test]
    fn progress_and_bookmarks_round_trip_with_stable_positions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.txt");
        std::fs::write(&path, "alpha beta gamma delta").unwrap();
        let document = document("book", "Book", path.clone());
        let store_path = dir.path().join("library.json");
        let mut store = Store::open(store_path.clone()).unwrap();
        store.add_book(&document, &path).unwrap();
        let progress = Progress::new(TextPosition::new(0, 11));
        store.set_progress("book", progress);
        store
            .add_bookmark("book", "Point".into(), progress)
            .unwrap();
        store.save().unwrap();

        let loaded = Store::open(store_path).unwrap();
        let book = loaded.resolve_book("Book").unwrap();
        assert_eq!(book.progress, progress);
        assert_eq!(book.bookmarks[0].position, progress.position);
        assert_eq!(loaded.library.schema_version, LIBRARY_SCHEMA_VERSION);
    }

    #[test]
    fn v1_page_progress_migrates_by_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let book_path = dir.path().join("legacy.txt");
        std::fs::write(&book_path, "abcdefghij").unwrap();
        let json = serde_json::json!({
            "books": [{
                "id": "legacy",
                "title": "Legacy",
                "path": book_path,
                "format": "Txt",
                "progress": {"page": 5, "section_index": 0, "start_line": 20},
                "page_count": 11,
                "bookmarks": [{
                    "id": "bm-1", "label": "Half", "page": 5,
                    "section_index": 0, "start_line": 20
                }]
            }]
        });

        let library = decode_library(&json.to_string()).unwrap();
        assert_eq!(library.books[0].progress.position, TextPosition::new(0, 5));
        assert_eq!(
            library.books[0].bookmarks[0].position,
            TextPosition::new(0, 5)
        );
    }

    #[test]
    fn v1_without_page_count_falls_back_to_chapter_start() {
        let value = serde_json::json!({
            "id": "missing", "title": "Missing", "path": "missing.txt", "format": "Txt",
            "progress": {"page": 7, "section_index": 3, "start_line": 20},
            "bookmarks": []
        });
        let book = decode_book_value(value).unwrap();
        assert_eq!(book.progress.position, TextPosition::new(3, 0));
    }

    #[test]
    fn v2_out_of_range_positions_are_clamped_on_load() {
        let json = serde_json::json!({
            "schema_version": 2,
            "books": [{
                "id": "book", "title": "Book", "path": "book.txt", "format": "Txt",
                "content_metrics": {"section_char_counts": [10]},
                "progress": {"position": {"section_index": 99, "char_offset": 99}},
                "bookmarks": [{
                    "id": "bm-1", "label": "End",
                    "position": {"section_index": 99, "char_offset": 99}
                }]
            }]
        });

        let library = decode_library(&json.to_string()).unwrap();

        assert_eq!(library.books[0].progress.position, TextPosition::new(0, 10));
        assert_eq!(
            library.books[0].bookmarks[0].position,
            TextPosition::new(0, 10)
        );
    }

    #[test]
    fn record_open_only_updates_recency() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.txt");
        std::fs::write(&path, "alpha").unwrap();
        let document = document("book", "Book", path.clone());
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        store.add_book(&document, path).unwrap();

        assert!(store.library().last_book_id.is_none());
        store.set_progress("book", Progress::new(TextPosition::new(0, 2)));
        assert!(store.library().last_book_id.is_none());

        store.record_open("book");

        assert!(store.resolve_book("Book").unwrap().last_opened.is_some());
        assert_eq!(store.library().last_book_id.as_deref(), Some("book"));
    }

    #[test]
    fn content_id_migration_preserves_state_and_resume_reference() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.txt");
        std::fs::write(&path, "alpha beta gamma delta").unwrap();
        let legacy_document = document("legacy-path-id", "Book", path.clone());
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        store.add_book(&legacy_document, &path).unwrap();
        let progress = Progress::new(TextPosition::new(0, 7));
        store.set_progress("legacy-path-id", progress);
        store
            .add_bookmark("legacy-path-id", "Point".into(), progress)
            .unwrap();
        store.record_open("legacy-path-id");

        let content_document = load_book(&path).unwrap();
        let content_id = content_document.id.clone();
        store.add_book(&content_document, &path).unwrap();

        assert_eq!(store.library().books.len(), 1);
        let migrated = store.book_by_id(&content_id).unwrap();
        assert_eq!(migrated.progress, progress);
        assert_eq!(migrated.bookmarks.len(), 1);
        assert_eq!(
            store.library().last_book_id.as_deref(),
            Some(content_id.as_str())
        );
    }

    #[test]
    fn sidecar_assisted_migration_removes_an_old_path_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("old").join("book.txt");
        let new_path = dir.path().join("new").join("book.txt");
        std::fs::create_dir_all(old_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(new_path.parent().unwrap()).unwrap();
        std::fs::write(&old_path, "alpha beta gamma delta").unwrap();
        std::fs::write(&new_path, "alpha beta gamma delta").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let legacy = document("legacy-path-id", "Book", old_path.clone());
        store.add_book(&legacy, &old_path).unwrap();
        let progress = Progress::new(TextPosition::new(0, 9));
        store.set_progress("legacy-path-id", progress);
        store.record_open("legacy-path-id");

        let current = load_book(&new_path).unwrap();
        let current_id = current.id.clone();
        store.add_book(&current, &new_path).unwrap();
        store.merge_legacy_record("legacy-path-id", &current_id);

        assert_eq!(store.library().books.len(), 1);
        assert_eq!(store.progress_for(&current_id), Some(&progress));
        assert_eq!(
            store.library().last_book_id.as_deref(),
            Some(current_id.as_str())
        );
    }
}
