use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::book::{BookDocument, BookFormat};

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    library: Library,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Library {
    pub books: Vec<LibraryBook>,
    pub last_book_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryBook {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub format: BookFormat,
    pub progress: Progress,
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    pub page: usize,
    pub section_index: usize,
    pub start_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub label: String,
    pub page: usize,
    pub section_index: usize,
    pub start_line: usize,
}

impl Bookmark {
    pub fn progress(&self) -> Progress {
        Progress {
            page: self.page,
            section_index: self.section_index,
            start_line: self.start_line,
        }
    }
}

impl Library {
    pub fn books(&self) -> &[LibraryBook] {
        &self.books
    }
}

impl Store {
    pub fn open_default() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .with_context(|| "Could not locate a user config directory")?
            .join("reader");
        Self::open(config_dir.join("library.json"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let library = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("Invalid {}", path.display()))?
        } else {
            Library::default()
        };
        Ok(Self { path, library })
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.library)?;
        std::fs::write(&self.path, text)?;
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

        if let Some(index) = self
            .library
            .books
            .iter()
            .position(|book| book.id == document.id)
        {
            self.library.books[index].title = document.title.clone();
            self.library.books[index].path = canonical;
            self.library.books[index].format = document.format;
            self.library.last_book_id = Some(document.id.clone());
            return Ok(&self.library.books[index]);
        }

        self.library.books.push(LibraryBook {
            id: document.id.clone(),
            title: document.title.clone(),
            path: canonical,
            format: document.format,
            progress: Progress::default(),
            bookmarks: Vec::new(),
        });
        self.library.last_book_id = Some(document.id.clone());
        let index = self.library.books.len() - 1;
        Ok(&self.library.books[index])
    }

    pub fn resolve_book(&self, query: &str) -> Result<&LibraryBook> {
        let needle = query.to_lowercase();
        let matches: Vec<&LibraryBook> = self
            .library
            .books
            .iter()
            .filter(|book| book.id == query || book.title.to_lowercase().contains(&needle))
            .collect();

        match matches.as_slice() {
            [book] => Ok(book),
            [] => bail!("Book not found: {query}"),
            _ => bail!("Book query is ambiguous: {query}"),
        }
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
        self.library
            .books
            .iter()
            .find(|book| book.id == book_id)
            .map(|book| &book.progress)
    }

    pub fn set_progress(&mut self, book_id: &str, progress: Progress) {
        if let Some(book) = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
        {
            book.progress = progress;
            self.library.last_book_id = Some(book_id.to_string());
        }
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
            id: format!("bm-{}", book.bookmarks.len() + 1),
            label,
            page: progress.page,
            section_index: progress.section_index,
            start_line: progress.start_line,
        };
        book.bookmarks.push(bookmark.clone());
        Ok(bookmark)
    }

    pub fn bookmarks_for(&self, book_id: &str) -> &[Bookmark] {
        self.library
            .books
            .iter()
            .find(|book| book.id == book_id)
            .map(|book| book.bookmarks.as_slice())
            .unwrap_or(&[])
    }

    pub fn bookmark_for(&self, book_id: &str, bookmark_id: &str) -> Option<&Bookmark> {
        self.bookmarks_for(book_id)
            .iter()
            .find(|bookmark| bookmark.id == bookmark_id)
    }

    pub fn delete_bookmark(&mut self, book_id: &str, bookmark_id: &str) -> Result<Bookmark> {
        let book = self
            .library
            .books
            .iter_mut()
            .find(|book| book.id == book_id)
            .with_context(|| format!("Book not found: {book_id}"))?;
        let index = book
            .bookmarks
            .iter()
            .position(|bookmark| bookmark.id == bookmark_id)
            .with_context(|| format!("Bookmark not found: {bookmark_id}"))?;
        Ok(book.bookmarks.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookDocument, BookMetadata, Section};

    fn document(id: &str, title: &str, path: PathBuf) -> BookDocument {
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
                section_count: 1,
            },
            sections: vec![Section {
                title: "A".into(),
                text: "Hello".into(),
            }],
        }
    }

    #[test]
    fn saves_and_loads_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let document = document("abc", "Book", dir.path().join("book.txt"));

        let mut store = Store::open(path.clone()).unwrap();
        store.add_book(&document, &document.path).unwrap();
        store.set_progress(
            "abc",
            Progress {
                page: 2,
                section_index: 0,
                start_line: 4,
            },
        );
        store.save().unwrap();

        let loaded = Store::open(path).unwrap();
        assert_eq!(loaded.resolve_book("Book").unwrap().progress.page, 2);
    }

    #[test]
    fn resolves_by_id_or_unique_title_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let rust = document("rust-id", "Rust Book", dir.path().join("rust.txt"));
        let poetry = document("poem-id", "Poetry", dir.path().join("poetry.txt"));

        store.add_book(&rust, &rust.path).unwrap();
        store.add_book(&poetry, &poetry.path).unwrap();

        assert_eq!(store.resolve_book("rust-id").unwrap().title, "Rust Book");
        assert_eq!(store.resolve_book("poet").unwrap().id, "poem-id");
    }

    #[test]
    fn rejects_ambiguous_title_fragments() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let first = document("one", "Rust Notes", dir.path().join("one.txt"));
        let second = document("two", "Rust Guide", dir.path().join("two.txt"));

        store.add_book(&first, &first.path).unwrap();
        store.add_book(&second, &second.path).unwrap();

        let err = store.resolve_book("rust").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn removing_last_book_moves_recent_to_remaining_book() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let first = document("one", "One", dir.path().join("one.txt"));
        let second = document("two", "Two", dir.path().join("two.txt"));

        store.add_book(&first, &first.path).unwrap();
        store.add_book(&second, &second.path).unwrap();
        let removed = store.remove_book("two").unwrap();

        assert_eq!(removed.id, "two");
        assert_eq!(store.library().last_book_id.as_deref(), Some("one"));
    }

    #[test]
    fn adds_finds_and_deletes_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        let book = document("book", "Book", dir.path().join("book.txt"));
        let progress = Progress {
            page: 4,
            section_index: 1,
            start_line: 20,
        };

        store.add_book(&book, &book.path).unwrap();
        let bookmark = store
            .add_bookmark("book", "Important".to_string(), progress)
            .unwrap();

        assert_eq!(bookmark.id, "bm-1");
        assert_eq!(
            store.bookmark_for("book", "bm-1").unwrap().progress(),
            progress
        );

        let removed = store.delete_bookmark("book", "bm-1").unwrap();
        assert_eq!(removed.label, "Important");
        assert!(store.bookmark_for("book", "bm-1").is_none());
    }
}
