use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::reader_state::{Overlay, ReaderAction, ReaderState};
use crate::ui::{command_for, KeyBinding, KeyChord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadingCommand {
    PreviousScreen,
    NextScreen,
    GoToPercent,
    Chapters,
    Search,
    Help,
    Quit,
    Start,
    End,
    NextMatch,
    PreviousMatch,
    Bookmark,
    Metadata,
    Privacy,
    Progress,
    Theme,
}

const PREVIOUS_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('k')),
    KeyChord::plain(KeyCode::Up),
    KeyChord::plain(KeyCode::PageUp),
];
const NEXT_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('j')),
    KeyChord::plain(KeyCode::Down),
    KeyChord::plain(KeyCode::Char(' ')),
    KeyChord::plain(KeyCode::PageDown),
];
const GOTO_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('g'))];
const CHAPTER_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('t'))];
const SEARCH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('/'))];
const HELP_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('?')),
    KeyChord::plain(KeyCode::F(1)),
];
const QUIT_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('q')), KeyChord::control('c')];
const START_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Home)];
const END_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::End),
    KeyChord::plain(KeyCode::Char('G')),
];
const NEXT_MATCH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('n'))];
const PREVIOUS_MATCH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('N'))];
const BOOKMARK_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('b'))];
const METADATA_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('M'))];
const PRIVACY_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('p'))];
const PROGRESS_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('s'))];
const THEME_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('c'))];

pub(crate) const READING_BINDINGS: &[KeyBinding<ReadingCommand>] = &[
    KeyBinding {
        command: ReadingCommand::PreviousScreen,
        chords: PREVIOUS_KEYS,
        keys: "k / Up / PgUp",
        hud_keys: "k",
        label: "Previous screen",
        short_label: "Prev",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::NextScreen,
        chords: NEXT_KEYS,
        keys: "j / Down / Space / PgDn",
        hud_keys: "j",
        label: "Next screen",
        short_label: "Next",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::GoToPercent,
        chords: GOTO_KEYS,
        keys: "g",
        hud_keys: "g",
        label: "Go to percentage",
        short_label: "Go",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::Chapters,
        chords: CHAPTER_KEYS,
        keys: "t",
        hud_keys: "t",
        label: "Browse chapters",
        short_label: "Chapters",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::Search,
        chords: SEARCH_KEYS,
        keys: "/",
        hud_keys: "/",
        label: "Search book",
        short_label: "Search",
        show_in_hud: true,
        hud_priority: 1,
    },
    KeyBinding {
        command: ReadingCommand::Help,
        chords: HELP_KEYS,
        keys: "? / F1",
        hud_keys: "?",
        label: "Full help",
        short_label: "Help",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::Quit,
        chords: QUIT_KEYS,
        keys: "q / Ctrl+C",
        hud_keys: "q",
        label: "Save and quit",
        short_label: "Quit",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::Start,
        chords: START_KEYS,
        keys: "Home",
        hud_keys: "Home",
        label: "Go to start (0%)",
        short_label: "Start",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::End,
        chords: END_KEYS,
        keys: "End / G",
        hud_keys: "End",
        label: "Go to end (100%)",
        short_label: "End",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::NextMatch,
        chords: NEXT_MATCH_KEYS,
        keys: "n",
        hud_keys: "n",
        label: "Next search result",
        short_label: "Next match",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::PreviousMatch,
        chords: PREVIOUS_MATCH_KEYS,
        keys: "N",
        hud_keys: "N",
        label: "Previous search result",
        short_label: "Prev match",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Bookmark,
        chords: BOOKMARK_KEYS,
        keys: "b",
        hud_keys: "b",
        label: "Add bookmark here",
        short_label: "Bookmark",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Metadata,
        chords: METADATA_KEYS,
        keys: "M",
        hud_keys: "M",
        label: "Book information",
        short_label: "Info",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Privacy,
        chords: PRIVACY_KEYS,
        keys: "p",
        hud_keys: "p",
        label: "Toggle privacy mode",
        short_label: "Privacy",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Progress,
        chords: PROGRESS_KEYS,
        keys: "s",
        hud_keys: "s",
        label: "Show or hide progress",
        short_label: "Progress",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Theme,
        chords: THEME_KEYS,
        keys: "c",
        hud_keys: "c",
        label: "Switch dark/light theme",
        short_label: "Theme",
        show_in_hud: false,
        hud_priority: 2,
    },
];

pub(crate) fn action_for_key(key: KeyEvent, state: &ReaderState) -> ReaderAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return ReaderAction::Exit;
    }

    match &state.overlay {
        Overlay::None => command_for(READING_BINDINGS, key)
            .map(command_action)
            .unwrap_or(ReaderAction::None),
        Overlay::Percent(_) | Overlay::Search(_) => text_input_action(key),
        Overlay::Chapters(navigator) => chapter_action(key, navigator.filter_active),
        Overlay::Metadata => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('M') => ReaderAction::Confirm,
            _ => ReaderAction::None,
        },
        Overlay::Help => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::F(1) => {
                ReaderAction::Confirm
            }
            _ => ReaderAction::None,
        },
    }
}

fn command_action(command: ReadingCommand) -> ReaderAction {
    match command {
        ReadingCommand::PreviousScreen => ReaderAction::PreviousScreen,
        ReadingCommand::NextScreen => ReaderAction::NextScreen,
        ReadingCommand::GoToPercent => ReaderAction::OpenPercent,
        ReadingCommand::Chapters => ReaderAction::OpenChapters,
        ReadingCommand::Search => ReaderAction::OpenSearch,
        ReadingCommand::Help => ReaderAction::OpenHelp,
        ReadingCommand::Quit => ReaderAction::Exit,
        ReadingCommand::Start => ReaderAction::JumpStart,
        ReadingCommand::End => ReaderAction::JumpEnd,
        ReadingCommand::NextMatch => ReaderAction::NextSearch,
        ReadingCommand::PreviousMatch => ReaderAction::PreviousSearch,
        ReadingCommand::Bookmark => ReaderAction::AddBookmark,
        ReadingCommand::Metadata => ReaderAction::OpenMetadata,
        ReadingCommand::Privacy => ReaderAction::TogglePrivacy,
        ReadingCommand::Progress => ReaderAction::ToggleProgress,
        ReadingCommand::Theme => ReaderAction::ToggleTheme,
    }
}

fn text_input_action(key: KeyEvent) -> ReaderAction {
    match key.code {
        KeyCode::Esc => ReaderAction::Cancel,
        KeyCode::Enter => ReaderAction::Confirm,
        KeyCode::Backspace => ReaderAction::Backspace,
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            ReaderAction::Input(ch)
        }
        _ => ReaderAction::None,
    }
}

fn chapter_action(key: KeyEvent, filter_active: bool) -> ReaderAction {
    match key.code {
        KeyCode::Esc => ReaderAction::Cancel,
        KeyCode::Enter => ReaderAction::Confirm,
        KeyCode::Backspace => ReaderAction::Backspace,
        KeyCode::Up => ReaderAction::Move(-1),
        KeyCode::Down => ReaderAction::Move(1),
        KeyCode::PageUp => ReaderAction::Move(-10),
        KeyCode::PageDown => ReaderAction::Move(10),
        KeyCode::Home => ReaderAction::JumpStart,
        KeyCode::End => ReaderAction::JumpEnd,
        KeyCode::Char('/') if !filter_active => ReaderAction::StartFilter,
        KeyCode::Char('k') if !filter_active => ReaderAction::Move(-1),
        KeyCode::Char('j') if !filter_active => ReaderAction::Move(1),
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            ReaderAction::Input(ch)
        }
        _ => ReaderAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookDocument, BookFormat, BookMetadata, Section};
    use crate::config::ReaderConfig;
    use crate::storage::Progress;
    use crossterm::event::KeyEventKind;
    use ratatui::layout::Rect;

    fn state() -> ReaderState {
        let path = std::path::PathBuf::from("book.txt");
        let sections = vec![Section {
            title: "One".into(),
            text: "text".into(),
        }];
        ReaderState::new(
            BookDocument {
                id: "book".into(),
                title: "Book".into(),
                path: path.clone(),
                format: BookFormat::Txt,
                metadata: BookMetadata {
                    title: "Book".into(),
                    author: None,
                    language: None,
                    publisher: None,
                    format: BookFormat::Txt,
                    path,
                    section_count: 1,
                },
                sections,
            },
            "book".into(),
            Rect::new(0, 0, 80, 24),
            ReaderConfig::default(),
            Progress::default(),
        )
    }

    #[test]
    fn root_escape_is_not_exit() {
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &state()),
            ReaderAction::None
        );
    }

    #[test]
    fn bindings_drive_reader_actions() {
        let state = state();
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::OpenPercent
        );
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT),
                &state
            ),
            ReaderAction::OpenHelp
        );
    }

    #[test]
    fn release_events_can_be_filtered_by_loop() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(release.kind, KeyEventKind::Release);
    }
}
