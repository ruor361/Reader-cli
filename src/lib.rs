mod atomic_file;
pub mod book;
pub mod bookshelf;
pub mod cli;
pub mod config;
pub mod discreet;
pub mod pagination;
pub mod paths;
#[deprecated(note = "use reader::discreet")]
pub mod privacy;
mod reader_input;
mod reader_state;
mod reader_view;
pub mod shelf;
pub mod storage;
pub mod tui;
pub mod ui;

pub use book::{
    BookDocument, BookFormat, ContentMetrics, ContentsEntry, ContentsSource, Section, TextPosition,
};
pub use config::ReaderConfig;
pub use discreet::discreet_stack;
pub use pagination::{paginate, Page};
pub use paths::{configuration_path, library_path};
#[allow(deprecated)]
pub use privacy::privacy_stack;
pub use storage::{Bookmark, Library, LibraryBook, Progress, ScanDirResult, Store};
