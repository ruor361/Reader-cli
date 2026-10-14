use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::reader_state::{Overlay, ReaderAction, ReaderState};
use crate::ui::{command_for, KeyBinding, KeyChord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadingCommand {
    PreviousScreen,
    NextScreen,
    GoToPercent,
    Contents,
    Search,
    Help,
    Quit,
    Start,
    End,
    NextMatch,
    PreviousMatch,
    Bookmark,
    Bookmarks,
    Metadata,
    Discreet,
    Hide,
    Progress,
    SearchResults,
    DecreaseWidth,
    IncreaseWidth,
    ResetWidth,
    ToggleImmersive,
    TogglePageOverlap,
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
const CONTENTS_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('t'))];
const SEARCH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('/'))];
const SEARCH_RESULTS_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('S'))];
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
const BOOKMARKS_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('B'))];
const METADATA_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('M'))];
const DISCREET_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('p'))];
const HIDE_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::F(12))];
const PROGRESS_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('s'))];
const DECREASE_WIDTH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('['))];
const INCREASE_WIDTH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char(']'))];
const RESET_WIDTH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('\\'))];
const IMMERSIVE_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('i'))];
const OVERLAP_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('o'))];

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
        command: ReadingCommand::Contents,
        chords: CONTENTS_KEYS,
        keys: "t",
        hud_keys: "t",
        label: "Browse contents",
        short_label: "Contents",
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
        command: ReadingCommand::Bookmarks,
        chords: BOOKMARKS_KEYS,
        keys: "B",
        hud_keys: "B",
        label: "Browse bookmarks",
        short_label: "Bookmarks",
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
        command: ReadingCommand::Discreet,
        chords: DISCREET_KEYS,
        keys: "p",
        hud_keys: "p",
        label: "Toggle discreet mode",
        short_label: "Discreet",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Hide,
        chords: HIDE_KEYS,
        keys: "F12",
        hud_keys: "F12",
        label: "Hide behind diagnostics",
        short_label: "Hide",
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
        command: ReadingCommand::SearchResults,
        chords: SEARCH_RESULTS_KEYS,
        keys: "S",
        hud_keys: "S",
        label: "Browse search results",
        short_label: "Results",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::DecreaseWidth,
        chords: DECREASE_WIDTH_KEYS,
        keys: "[",
        hud_keys: "[",
        label: "Narrow reading width",
        short_label: "Narrow",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::IncreaseWidth,
        chords: INCREASE_WIDTH_KEYS,
        keys: "]",
        hud_keys: "]",
        label: "Widen reading width",
        short_label: "Widen",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::ResetWidth,
        chords: RESET_WIDTH_KEYS,
        keys: "\\",
        hud_keys: "\\",
        label: "Reset reading width to auto",
        short_label: "Auto width",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::ToggleImmersive,
        chords: IMMERSIVE_KEYS,
        keys: "i",
        hud_keys: "i",
        label: "Toggle immersive mode",
        short_label: "Immersive",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::TogglePageOverlap,
        chords: OVERLAP_KEYS,
        keys: "o",
        hud_keys: "o",
        label: "Toggle page overlap context",
        short_label: "Overlap",
        show_in_hud: false,
        hud_priority: 2,
    },
];

pub(crate) const DISCREET_BINDINGS: &[KeyBinding<ReadingCommand>] = &[
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
        command: ReadingCommand::Start,
        chords: START_KEYS,
        keys: "Home",
        hud_keys: "Home",
        label: "Go to start",
        short_label: "Start",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::End,
        chords: END_KEYS,
        keys: "End / G",
        hud_keys: "End",
        label: "Go to end",
        short_label: "End",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ReadingCommand::Bookmark,
        chords: BOOKMARK_KEYS,
        keys: "b",
        hud_keys: "b",
        label: "Add bookmark",
        short_label: "Bookmark",
        show_in_hud: true,
        hud_priority: 1,
    },
    KeyBinding {
        command: ReadingCommand::Discreet,
        chords: DISCREET_KEYS,
        keys: "p",
        hud_keys: "p",
        label: "Return to normal",
        short_label: "Normal",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ReadingCommand::Hide,
        chords: HIDE_KEYS,
        keys: "F12",
        hud_keys: "F12",
        label: "Hide",
        short_label: "Hide",
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
        show_in_hud: false,
        hud_priority: 2,
    },
];

pub(crate) fn action_for_key(key: KeyEvent, state: &ReaderState) -> ReaderAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return ReaderAction::Exit;
    }
    if key.code == KeyCode::F(12) {
        return ReaderAction::ToggleHidden;
    }
    if state.hidden {
        return ReaderAction::None;
    }
    if state.config.discreet_mode {
        return command_for(DISCREET_BINDINGS, key)
            .map(command_action)
            .or_else(|| {
                command_for(READING_BINDINGS, key).map(|_| ReaderAction::DiscreetUnavailable)
            })
            .unwrap_or(ReaderAction::None);
    }

    match &state.overlay {
        Overlay::None => command_for(READING_BINDINGS, key)
            .map(command_action)
            .unwrap_or(ReaderAction::None),
        Overlay::Percent(_) | Overlay::Search(_) => text_input_action(key),
        Overlay::SearchResults(_) => match key.code {
            KeyCode::Esc => ReaderAction::Cancel,
            KeyCode::Enter => ReaderAction::Confirm,
            KeyCode::Up | KeyCode::Char('k') => ReaderAction::Move(-1),
            KeyCode::Down | KeyCode::Char('j') => ReaderAction::Move(1),
            KeyCode::PageUp => ReaderAction::Move(-10),
            KeyCode::PageDown => ReaderAction::Move(10),
            KeyCode::Char('n') => ReaderAction::NextSearch,
            KeyCode::Char('N') => ReaderAction::PreviousSearch,
            _ => ReaderAction::None,
        },
        Overlay::Bookmarks(_) => match key.code {
            KeyCode::Esc => ReaderAction::Cancel,
            KeyCode::Enter => ReaderAction::Confirm,
            KeyCode::Up | KeyCode::Char('k') => ReaderAction::Move(-1),
            KeyCode::Down | KeyCode::Char('j') => ReaderAction::Move(1),
            KeyCode::PageUp => ReaderAction::Move(-10),
            KeyCode::PageDown => ReaderAction::Move(10),
            KeyCode::Home => ReaderAction::JumpStart,
            KeyCode::End => ReaderAction::JumpEnd,
            KeyCode::Char('d') => ReaderAction::DeleteBookmark,
            _ => ReaderAction::None,
        },
        Overlay::Contents(navigator) => contents_action(key, navigator.filter_active),
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
        ReadingCommand::Contents => ReaderAction::OpenContents,
        ReadingCommand::Search => ReaderAction::OpenSearch,
        ReadingCommand::SearchResults => ReaderAction::OpenSearchResults,
        ReadingCommand::Help => ReaderAction::OpenHelp,
        ReadingCommand::Quit => ReaderAction::Exit,
        ReadingCommand::Start => ReaderAction::JumpStart,
        ReadingCommand::End => ReaderAction::JumpEnd,
        ReadingCommand::NextMatch => ReaderAction::NextSearch,
        ReadingCommand::PreviousMatch => ReaderAction::PreviousSearch,
        ReadingCommand::Bookmark => ReaderAction::AddBookmark,
        ReadingCommand::Bookmarks => ReaderAction::OpenBookmarks,
        ReadingCommand::Metadata => ReaderAction::OpenMetadata,
        ReadingCommand::Discreet => ReaderAction::ToggleDiscreet,
        ReadingCommand::Hide => ReaderAction::ToggleHidden,
        ReadingCommand::Progress => ReaderAction::ToggleProgress,
        ReadingCommand::DecreaseWidth => ReaderAction::AdjustTextWidth(-2),
        ReadingCommand::IncreaseWidth => ReaderAction::AdjustTextWidth(2),
        ReadingCommand::ResetWidth => ReaderAction::ResetTextWidth,
        ReadingCommand::ToggleImmersive => ReaderAction::ToggleImmersive,
        ReadingCommand::TogglePageOverlap => ReaderAction::TogglePageOverlap,
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

fn contents_action(key: KeyEvent, filter_active: bool) -> ReaderAction {
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
                contents: vec![crate::book::ContentsEntry {
                    title: "One".into(),
                    depth: 0,
                    target: crate::book::TextPosition::new(0, 0),
                }],
                contents_source: crate::book::ContentsSource::Generated,
            },
            "book".into(),
            Rect::new(0, 0, 80, 24),
            ReaderConfig::default(),
            Progress::default(),
            Vec::new(),
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
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::TogglePageOverlap
        );
    }

    #[test]
    fn removed_theme_key_has_no_action() {
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
                &state()
            ),
            ReaderAction::None
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

    #[test]
    fn f12_is_global_and_hidden_mode_blocks_other_keys() {
        let mut state = state();
        state.overlay = Overlay::Search(Default::default());
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE), &state),
            ReaderAction::ToggleHidden
        );
        state.hidden = true;
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::None
        );
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &state
            ),
            ReaderAction::Exit
        );
        assert_eq!(
            action_for_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE), &state),
            ReaderAction::ToggleHidden
        );
    }

    #[test]
    fn discreet_mode_uses_an_explicit_command_whitelist() {
        let mut state = state();
        state.config.discreet_mode = true;

        for (code, expected) in [
            (KeyCode::Char('j'), ReaderAction::NextScreen),
            (KeyCode::Char('k'), ReaderAction::PreviousScreen),
            (KeyCode::Char('b'), ReaderAction::AddBookmark),
            (KeyCode::Char('p'), ReaderAction::ToggleDiscreet),
            (KeyCode::Char('q'), ReaderAction::Exit),
            (KeyCode::Home, ReaderAction::JumpStart),
            (KeyCode::End, ReaderAction::JumpEnd),
        ] {
            assert_eq!(
                action_for_key(KeyEvent::new(code, KeyModifiers::NONE), &state),
                expected
            );
        }

        for code in [
            KeyCode::Char('g'),
            KeyCode::Char('t'),
            KeyCode::Char('/'),
            KeyCode::Char('M'),
            KeyCode::Char('?'),
            KeyCode::Char('n'),
            KeyCode::Char('N'),
            KeyCode::Char('s'),
            KeyCode::Char('['),
            KeyCode::Char(']'),
            KeyCode::Char('\\'),
            KeyCode::Char('i'),
            KeyCode::Char('B'),
            KeyCode::F(1),
        ] {
            assert_eq!(
                action_for_key(KeyEvent::new(code, KeyModifiers::NONE), &state),
                ReaderAction::DiscreetUnavailable,
                "key: {code:?}"
            );
        }
    }

    #[test]
    fn width_and_immersive_keys_map_correctly() {
        let state = state();
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::AdjustTextWidth(-2)
        );
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::AdjustTextWidth(2)
        );
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::ResetTextWidth
        );
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::ToggleImmersive
        );
        assert_eq!(
            action_for_key(
                KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE),
                &state
            ),
            ReaderAction::OpenBookmarks
        );
    }
}
