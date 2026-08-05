use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::book::{load_book, BookFormat, ContentMetrics};
use crate::config::ReaderConfig;
use crate::storage::{decode_book_value, LibraryBook, Store, LIBRARY_SCHEMA_VERSION};
use crate::storage::{Bookmark, Progress};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDisposition {
    Copied,
    Reused,
}

#[derive(Debug, Clone)]
pub struct ImportOutcome {
    pub book: LibraryBook,
    pub disposition: ImportDisposition,
    pub warnings: Vec<String>,
}

/// Import one supported book into the configured managed bookshelf.
///
/// The source is always preserved. The copy is validated before the store is
/// changed, and identical content already managed by the store is reused.
pub fn import_managed_book(
    store: &mut Store,
    config: &ReaderConfig,
    source: &Path,
) -> Result<ImportOutcome> {
    let bookshelf_dir = config.bookshelf_dir.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "bookshelf_dir is not configured. Set it once with `reader refresh <DIR>`, then retry the import."
        )
    })?;
    validate_bookshelf_dir(bookshelf_dir)?;

    let source = resolve_input_path(source.to_path_buf())?;
    let metadata = fs::metadata(&source)
        .with_context(|| format!("Book file not found: {}", source.display()))?;
    if !metadata.is_file() {
        bail!("Book path is not a file: {}", source.display());
    }
    if !crate::book::is_supported_book_path(&source) {
        bail!(
            "Unsupported book format: {}. Expected EPUB, TXT, or PDF.",
            source.display()
        );
    }

    // Parsing before copying ensures corrupt or unsupported content cannot
    // leave an orphan in the managed directory.
    let source_document = load_book(&source)?;
    let source_id = source_document.id.clone();
    let (destination, disposition, created_copy) =
        choose_import_destination(store, &source, bookshelf_dir, &source_id)?;

    let document = if paths_match(&source, &destination) {
        source_document
    } else {
        match load_book(&destination) {
            Ok(document) if document.id == source_id => document,
            Ok(document) => {
                if created_copy {
                    let _ = fs::remove_file(&destination);
                }
                bail!(
                    "Imported copy failed content verification: expected {}, got {}",
                    source_id,
                    document.id
                );
            }
            Err(err) => {
                if created_copy {
                    let _ = fs::remove_file(&destination);
                }
                return Err(err).with_context(|| {
                    format!("Failed to validate imported copy {}", destination.display())
                });
            }
        }
    };

    let mut staged = store.clone();
    let book = staged.add_book(&document, &destination)?.clone();
    if let Err(err) = staged.save() {
        if created_copy {
            let _ = fs::remove_file(&destination);
        }
        return Err(err).context("Failed to save the imported book to the library");
    }

    let mut warnings = Vec::new();
    if let Err(err) = sync_sidecar_for_book(&staged, &book.id, bookshelf_dir) {
        warnings.push(format!(
            "Book was imported, but its sidecar could not be updated: {err}"
        ));
    }
    *store = staged;

    Ok(ImportOutcome {
        book,
        disposition,
        warnings,
    })
}

fn choose_import_destination(
    store: &Store,
    source: &Path,
    bookshelf_dir: &Path,
    source_id: &str,
) -> Result<(PathBuf, ImportDisposition, bool)> {
    if let Some(existing) = store.book_by_id(source_id) {
        if !existing.missing
            && path_is_in_dir(&existing.path, bookshelf_dir)
            && existing.path.is_file()
            && file_sha256(&existing.path).ok().as_deref() == Some(source_id)
        {
            return Ok((existing.path.clone(), ImportDisposition::Reused, false));
        }
    }

    if path_is_in_dir(source, bookshelf_dir) {
        return Ok((source.to_path_buf(), ImportDisposition::Reused, false));
    }

    let file_name = source
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Source path has no filename: {}", source.display()))?;
    let mut target = bookshelf_dir.join(file_name);
    let mut counter = 1_u32;
    while target.exists() {
        if target.is_file() && file_sha256(&target).ok().as_deref() == Some(source_id) {
            return Ok((target, ImportDisposition::Reused, false));
        }
        target = suffixed_import_path(source, bookshelf_dir, counter);
        counter += 1;
    }

    copy_book_atomically(source, &target, source_id)?;
    Ok((target, ImportDisposition::Copied, true))
}

fn suffixed_import_path(source: &Path, bookshelf_dir: &Path, counter: u32) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("book");
    let name = match source.extension().and_then(|value| value.to_str()) {
        Some(extension) if !extension.is_empty() => {
            format!("{stem} ({counter}).{extension}")
        }
        _ => format!("{stem} ({counter})"),
    };
    bookshelf_dir.join(name)
}

fn copy_book_atomically(source: &Path, target: &Path, expected_id: &str) -> Result<()> {
    let directory = target
        .parent()
        .with_context(|| format!("Import target has no parent: {}", target.display()))?;
    let mut input =
        fs::File::open(source).with_context(|| format!("Failed to open {}", source.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).with_context(|| {
        format!(
            "Failed to create a temporary file in {}",
            directory.display()
        )
    })?;
    std::io::copy(&mut input, &mut temporary).with_context(|| {
        format!(
            "Failed to copy {} into {}",
            source.display(),
            directory.display()
        )
    })?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("Failed to flush imported copy for {}", source.display()))?;

    let copied_id = file_sha256(temporary.path())?;
    if copied_id != expected_id {
        bail!("Imported copy failed content verification: expected {expected_id}, got {copied_id}");
    }
    temporary.persist_noclobber(target).map_err(|err| {
        anyhow::anyhow!(
            "Failed to finalize imported copy {}: {}",
            target.display(),
            err.error
        )
    })?;
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to hash {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut id = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(id)
}

pub fn path_from_user_input(input: &str) -> Result<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("Enter a path");
    }
    let unquoted = if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    resolve_input_path(PathBuf::from(unquoted))
}

pub fn resolve_input_path(path: PathBuf) -> Result<PathBuf> {
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
    let mut matches = fs::read_dir(parent)
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

pub fn resolve_bookshelf_dir(path: &Path) -> Result<PathBuf> {
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
    validate_bookshelf_dir(&absolute)?;
    Ok(user_facing_path(absolute))
}

fn validate_bookshelf_dir(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("Bookshelf directory is unavailable: {}", path.display()))?;
    if !metadata.is_dir() {
        bail!("Bookshelf path is not a directory: {}", path.display());
    }
    fs::read_dir(path)
        .with_context(|| format!("Bookshelf directory is not readable: {}", path.display()))?;
    Ok(())
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

#[derive(serde::Serialize, serde::Deserialize)]
struct Sidecar {
    schema_version: u32,
    book: PortableBookState,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PortableBookState {
    id: String,
    title: String,
    #[serde(default)]
    author: Option<String>,
    format: BookFormat,
    #[serde(default)]
    content_metrics: ContentMetrics,
    #[serde(default)]
    progress: Progress,
    #[serde(default)]
    last_opened: Option<std::time::SystemTime>,
    #[serde(default)]
    bookmarks: Vec<Bookmark>,
}

impl From<&LibraryBook> for PortableBookState {
    fn from(book: &LibraryBook) -> Self {
        Self {
            id: book.id.clone(),
            title: book.title.clone(),
            author: book.author.clone(),
            format: book.format,
            content_metrics: book.content_metrics.clone(),
            progress: book.progress,
            last_opened: book.last_opened,
            bookmarks: book.bookmarks.clone(),
        }
    }
}

impl PortableBookState {
    fn into_library_book(self, book_path: &Path) -> LibraryBook {
        LibraryBook {
            id: self.id,
            title: self.title,
            author: self.author,
            path: book_path
                .canonicalize()
                .unwrap_or_else(|_| book_path.to_path_buf()),
            format: self.format,
            content_metrics: self.content_metrics,
            progress: self.progress,
            last_opened: self.last_opened,
            missing: false,
            bookmarks: self.bookmarks,
        }
    }
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

fn write_sidecar(book: &LibraryBook) -> Result<()> {
    let path = sidecar_path(&book.path);
    let text = serde_json::to_string_pretty(&Sidecar {
        schema_version: LIBRARY_SCHEMA_VERSION,
        book: PortableBookState::from(book),
    })
    .with_context(|| format!("Failed to serialize sidecar for {}", book.path.display()))?;
    crate::atomic_file::write(&path, text.as_bytes())
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
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let book = if version >= 3 {
        let portable: PortableBookState = serde_json::from_value(
            value
                .get("book")
                .cloned()
                .with_context(|| format!("Missing sidecar book record: {}", path.display()))?,
        )
        .with_context(|| format!("Invalid portable sidecar record: {}", path.display()))?;
        portable.into_library_book(book_path)
    } else if let Some(book) = value.get("book") {
        decode_book_value(book.clone())
            .with_context(|| format!("Invalid sidecar record: {}", path.display()))?
    } else {
        decode_book_value(value)
            .with_context(|| format!("Invalid sidecar record: {}", path.display()))?
    };
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

pub fn sync_sidecar_for_book(store: &Store, book_id: &str, bookshelf_dir: &Path) -> Result<()> {
    if let Some(book) = store.book_by_id(book_id) {
        ensure_book_is_managed(book, bookshelf_dir)?;
        write_sidecar(book)?;
    }
    Ok(())
}

fn ensure_book_is_managed(book: &LibraryBook, bookshelf_dir: &Path) -> Result<()> {
    let directory = bookshelf_dir.canonicalize().with_context(|| {
        format!(
            "Managed bookshelf directory is unavailable: {}",
            bookshelf_dir.display()
        )
    })?;
    let book_path = book
        .path
        .canonicalize()
        .with_context(|| format!("Book file is unavailable: {}", book.path.display()))?;
    if !book_path.starts_with(&directory) {
        bail!(
            "Refusing to write sidecar outside bookshelf_dir: {}",
            book.path.display()
        );
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
    let metadata = fs::metadata(bookshelf_dir).with_context(|| {
        format!(
            "Bookshelf directory is unavailable: {}",
            bookshelf_dir.display()
        )
    })?;
    if !metadata.is_dir() {
        bail!(
            "Bookshelf path is not a directory: {}",
            bookshelf_dir.display()
        );
    }
    fs::read_dir(bookshelf_dir).with_context(|| {
        format!(
            "Bookshelf directory is not readable: {}",
            bookshelf_dir.display()
        )
    })?;

    let mut report = RefreshReport::default();

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
                    let restored_id = restored.id.clone();
                    store.restore_reading_state(&book_id, restored);
                    store.merge_legacy_record(&restored_id, &book_id);
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
        if crate::book::is_supported_book_path(&path) {
            book_files.push(path);
        } else if ext == "json" {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let stem = stem.to_ascii_lowercase();
                if stem.ends_with(".epub") || stem.ends_with(".txt") || stem.ends_with(".pdf") {
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
            contents: vec![crate::book::ContentsEntry {
                title: "A".into(),
                depth: 0,
                target: crate::book::TextPosition::new(0, 0),
            }],
            contents_source: crate::book::ContentsSource::Generated,
        }
    }

    fn managed_config(path: &Path) -> ReaderConfig {
        let mut config = ReaderConfig::default();
        config.bookshelf_dir = Some(path.to_path_buf());
        config
    }

    #[test]
    fn managed_import_requires_a_configured_bookshelf_without_mutating_state() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("book.txt");
        fs::write(&source, "hello").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let error = import_managed_book(&mut store, &ReaderConfig::default(), &source)
            .unwrap_err()
            .to_string();

        assert!(error.contains("reader refresh <DIR>"));
        assert!(store.library().books().is_empty());
        assert_eq!(fs::read_to_string(&source).unwrap(), "hello");
    }

    #[test]
    fn managed_import_copies_saves_and_writes_sidecar_while_preserving_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source").join("一地鸡毛.txt");
        let shelf = dir.path().join("shelf");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir(&shelf).unwrap();
        fs::write(&source, "刘震云").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let outcome = import_managed_book(&mut store, &managed_config(&shelf), &source).unwrap();

        assert_eq!(outcome.disposition, ImportDisposition::Copied);
        assert_eq!(
            outcome.book.path,
            shelf.join("一地鸡毛.txt").canonicalize().unwrap()
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), "刘震云");
        assert_eq!(fs::read_to_string(&outcome.book.path).unwrap(), "刘震云");
        assert!(sidecar_path(&outcome.book.path).exists());
        let reopened = Store::open(dir.path().join("library.json")).unwrap();
        assert!(reopened.book_by_id(&outcome.book.id).is_some());
    }

    #[test]
    fn managed_import_reuses_content_already_registered_in_bookshelf() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        let shelf = dir.path().join("shelf");
        fs::create_dir(&sources).unwrap();
        fs::create_dir(&shelf).unwrap();
        let first = sources.join("first.txt");
        let duplicate = sources.join("duplicate.txt");
        fs::write(&first, "same bytes").unwrap();
        fs::write(&duplicate, "same bytes").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let config = managed_config(&shelf);
        let original = import_managed_book(&mut store, &config, &first).unwrap();

        let outcome = import_managed_book(&mut store, &config, &duplicate).unwrap();

        assert_eq!(outcome.disposition, ImportDisposition::Reused);
        assert_eq!(outcome.book.path, original.book.path);
        assert!(!shelf.join("duplicate.txt").exists());
        assert_eq!(store.library().books().len(), 1);
    }

    #[test]
    fn managed_import_reuses_unregistered_same_name_with_identical_content() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("source");
        let shelf = dir.path().join("shelf");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&shelf).unwrap();
        let source = source_dir.join("book.txt");
        let existing = shelf.join("book.txt");
        fs::write(&source, "same bytes").unwrap();
        fs::write(&existing, "same bytes").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let outcome = import_managed_book(&mut store, &managed_config(&shelf), &source).unwrap();

        assert_eq!(outcome.disposition, ImportDisposition::Reused);
        assert_eq!(outcome.book.path, existing.canonicalize().unwrap());
        assert!(!shelf.join("book (1).txt").exists());
    }

    #[test]
    fn managed_import_suffixes_same_name_with_different_content() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("source");
        let shelf = dir.path().join("shelf");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&shelf).unwrap();
        let source = source_dir.join("book.txt");
        fs::write(&source, "new content").unwrap();
        fs::write(shelf.join("book.txt"), "old content").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let outcome = import_managed_book(&mut store, &managed_config(&shelf), &source).unwrap();

        assert_eq!(outcome.disposition, ImportDisposition::Copied);
        assert_eq!(
            outcome.book.path,
            shelf.join("book (1).txt").canonicalize().unwrap()
        );
        assert_eq!(
            fs::read_to_string(shelf.join("book.txt")).unwrap(),
            "old content"
        );
    }

    #[test]
    fn managed_import_reuses_a_source_already_inside_bookshelf() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let source = shelf.join("book.txt");
        fs::write(&source, "already managed").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let outcome = import_managed_book(&mut store, &managed_config(&shelf), &source).unwrap();

        assert_eq!(outcome.disposition, ImportDisposition::Reused);
        assert_eq!(outcome.book.path, source.canonicalize().unwrap());
    }

    #[test]
    fn invalid_book_is_rejected_without_copy_or_library_record() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let source = dir.path().join("broken.epub");
        fs::write(&source, "not an epub").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        assert!(import_managed_book(&mut store, &managed_config(&shelf), &source).is_err());
        assert!(!shelf.join("broken.epub").exists());
        assert!(store.library().books().is_empty());
    }

    #[test]
    fn pasted_quoted_unicode_path_is_unwrapped() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("一地鸡毛.txt");
        fs::write(&source, "text").unwrap();

        let input = format!("  \"{}\"  ", source.display());

        assert_eq!(path_from_user_input(&input).unwrap(), source);
    }

    #[test]
    fn resolves_file_name_with_collapsed_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("A Grammar of the English Language  Barnes - William Cobbett.epub");
        fs::write(&file, []).unwrap();
        let collapsed = dir
            .path()
            .join("A Grammar of the English Language Barnes - William Cobbett.epub");

        assert_eq!(resolve_input_path(collapsed).unwrap(), file);
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

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(sidecar_path(&book.path)).unwrap())
                .unwrap();
        assert_eq!(value["schema_version"], 3);
        assert!(value["book"].get("path").is_none());
        assert!(value["book"].get("missing").is_none());
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

    #[test]
    fn refresh_preserves_the_most_recently_opened_book() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        fs::write(shelf.join("first.txt"), b"first book").unwrap();
        fs::write(shelf.join("second.txt"), b"second book").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        refresh(&mut store, &shelf).unwrap();
        let first_id = store.resolve_book("first").unwrap().id.clone();
        store.record_open(&first_id);

        refresh(&mut store, &shelf).unwrap();

        assert_eq!(
            store.library().last_book_id.as_deref(),
            Some(first_id.as_str())
        );
    }

    #[test]
    fn refresh_errors_when_bookshelf_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-mounted");
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let error = refresh(&mut store, &missing).unwrap_err().to_string();

        assert!(error.contains("Bookshelf directory is unavailable"));
        assert!(error.contains("not-mounted"));
    }

    #[test]
    fn refresh_errors_when_bookshelf_path_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        fs::write(&file, b"x").unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();

        let error = refresh(&mut store, &file).unwrap_err().to_string();

        assert!(error.contains("Bookshelf path is not a directory"));
    }

    #[test]
    fn sidecar_sync_refuses_a_book_outside_the_managed_directory() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join("shelf");
        fs::create_dir(&shelf).unwrap();
        let external = dir.path().join("external.txt");
        fs::write(&external, b"outside").unwrap();
        let document = make_document("external", "External", &external);
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        store.add_book(&document, &external).unwrap();

        let error = sync_sidecar_for_book(&store, "external", &shelf)
            .unwrap_err()
            .to_string();

        assert!(error.contains("outside bookshelf_dir"));
        assert!(!sidecar_path(&external).exists());
    }
}
