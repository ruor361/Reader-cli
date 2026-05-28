use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "reader",
    version,
    about = "Read EPUB and TXT books in your terminal",
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
    Add {
        file: PathBuf,
    },
    List,
    Open {
        book: String,
    },
    Resume,
    Remove {
        book: String,
    },
    Bookmark {
        #[command(subcommand)]
        command: BookmarkCommand,
    },
    Search {
        book: String,
        query: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum BookmarkCommand {
    Add { book: String, label: String },
    List { book: String },
    Goto { book: String, bookmark: String },
    Delete { book: String, bookmark: String },
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
    fn parses_search_command() {
        let cli = Cli::parse_from(["reader", "search", "abc123", "needle"]);
        let Some(Command::Search { book, query }) = cli.command else {
            panic!("expected search command");
        };

        assert_eq!(book, "abc123");
        assert_eq!(query, "needle");
    }
}
