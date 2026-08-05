use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "reader",
    version,
    about = "Read EPUB, TXT, and text-based PDF books in your terminal",
    subcommand_precedence_over_arg = true
)]
pub struct Cli {
    #[arg(value_name = "FILE", num_args = 1.., trailing_var_arg = true)]
    pub file: Vec<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    pub fn file_path(&self) -> Option<PathBuf> {
        (!self.file.is_empty()).then(|| PathBuf::from(self.file.join(" ")))
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Import one book into the managed bookshelf.
    #[command(name = "import", visible_alias = "add")]
    Import {
        file: PathBuf,
        /// Open the book immediately after importing it.
        #[arg(long)]
        open: bool,
    },
    List,
    Open {
        book: String,
    },
    Resume,
    Remove {
        book: String,
        #[arg(long)]
        delete_file: bool,
    },
    Bookmark {
        #[command(subcommand)]
        command: BookmarkCommand,
    },
    Search {
        book: String,
        query: String,
    },
    /// Export raw reading events.
    Export {
        #[arg(value_name = "BOOK")]
        book: Option<String>,
        #[arg(long, value_enum, default_value_t = ExportFormat::Json)]
        format: ExportFormat,
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Reconcile the managed bookshelf. This is the short form of
    /// `reader shelf refresh`.
    Refresh {
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    Shelf {
        #[command(subcommand)]
        command: Option<ShelfCommand>,
    },
}

#[derive(Debug, Subcommand)]
pub enum BookmarkCommand {
    Add { book: String, label: String },
    List { book: String },
    Goto { book: String, bookmark: String },
    Delete { book: String, bookmark: String },
}

#[derive(Debug, Subcommand)]
pub enum ShelfCommand {
    /// Reconcile the bookshelf directory, per-book sidecar JSONs, and
    /// `library.json`. Passing DIR also saves it as the managed bookshelf.
    Refresh {
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_direct_file_open() {
        let cli = Cli::parse_from(["reader", "book.txt"]);
        assert_eq!(cli.file_path(), Some(PathBuf::from("book.txt")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_unquoted_direct_file_open_with_spaces() {
        let cli = Cli::parse_from([
            "reader",
            "Liberal\\A",
            "Grammar",
            "of",
            "the",
            "English",
            "Language",
            "Barnes",
            "-",
            "William",
            "Cobbett.epub",
        ]);
        assert_eq!(
            cli.file_path(),
            Some(PathBuf::from(
                "Liberal\\A Grammar of the English Language Barnes - William Cobbett.epub"
            ))
        );
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_bookmark_add() {
        let cli = Cli::parse_from(["reader", "bookmark", "add", "abc123", "good part"]);
        let Some(Command::Bookmark { command }) = cli.command else {
            panic!("expected bookmark command");
        };
        let BookmarkCommand::Add { book, label } = command else {
            panic!("expected bookmark add");
        };
        assert_eq!(book, "abc123");
        assert_eq!(label, "good part");
    }

    #[test]
    fn parses_import_command() {
        let cli = Cli::parse_from(["reader", "import", "book.epub", "--open"]);
        assert!(matches!(
            cli.command,
            Some(Command::Import {
                ref file,
                open: true
            }) if file == &PathBuf::from("book.epub")
        ));
    }

    #[test]
    fn add_is_a_visible_import_alias() {
        let cli = Cli::parse_from(["reader", "add", "book.txt"]);
        assert!(matches!(
            cli.command,
            Some(Command::Import {
                ref file,
                open: false
            }) if file == &PathBuf::from("book.txt")
        ));
    }

    #[test]
    fn parses_search_command() {
        let cli = Cli::parse_from(["reader", "search", "abc123", "needle"]);
        let Some(Command::Search { book, query }) = cli.command else {
            panic!("expected search command");
        };

        assert_eq!(book, "abc123");
        assert_eq!(query, "needle");
    }

    #[test]
    fn parses_csv_export_to_file() {
        let cli = Cli::parse_from([
            "reader",
            "export",
            "abc123",
            "--format",
            "csv",
            "--output",
            "events.csv",
        ]);
        assert!(matches!(
            cli.command,
            Some(Command::Export {
                book: Some(ref book),
                format: ExportFormat::Csv,
                output: Some(ref output),
            }) if book == "abc123" && output == &PathBuf::from("events.csv")
        ));
    }

    #[test]
    fn parses_shelf_command() {
        let cli = Cli::parse_from(["reader", "shelf"]);
        assert!(matches!(
            cli.command,
            Some(Command::Shelf { command: None })
        ));
    }

    #[test]
    fn parses_top_level_refresh_directory() {
        let cli = Cli::parse_from(["reader", "refresh", ".\\Liberal"]);
        assert!(matches!(
            cli.command,
            Some(Command::Refresh { dir: Some(ref dir) })
                if dir == &PathBuf::from(".\\Liberal")
        ));
        assert!(cli.file.is_empty());
    }

    #[test]
    fn parses_shelf_refresh_command() {
        let cli = Cli::parse_from(["reader", "shelf", "refresh"]);
        assert!(matches!(
            cli.command,
            Some(Command::Shelf {
                command: Some(ShelfCommand::Refresh { dir: None })
            })
        ));
    }

    #[test]
    fn parses_shelf_refresh_directory() {
        let cli = Cli::parse_from(["reader", "shelf", "refresh", "Liberal"]);
        assert!(matches!(
            cli.command,
            Some(Command::Shelf {
                command: Some(ShelfCommand::Refresh {
                    dir: Some(ref dir)
                })
            }) if dir == &PathBuf::from("Liberal")
        ));
    }
}
