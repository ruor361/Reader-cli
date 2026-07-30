use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;

use crate::book::BookDocument;
use crate::config::ReaderConfig;
use crate::reader_input::action_for_key;
use crate::reader_state::{NotificationKind, ReaderEffect, ReaderState};
use crate::storage::Store;

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

pub fn run_reader(
    document: BookDocument,
    book_id: String,
    store: &mut Store,
    config: &mut ReaderConfig,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    let size = terminal.size()?;
    let viewport = Rect::new(0, 0, size.width, size.height);

    store.update_document_metrics(&book_id, &document);
    let saved = store.progress_for(&book_id).copied().unwrap_or_default();
    store.record_open(&book_id);
    let mut state = ReaderState::new(document, book_id, viewport, config.clone(), saved);
    persist_book_state(&state, store)?;
    let mut exit = false;

    while !exit {
        state.expire_notification(Instant::now());
        terminal.draw(|frame| crate::reader_view::draw(frame, &state))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if should_handle_key_event(key) => {
                let action = action_for_key(key, &state);
                let effects = state.update(action);
                exit = apply_effects(effects, &mut state, store, config)?;
            }
            Event::Resize(width, height) => state.resize(width, height),
            _ => {}
        }
    }

    store.set_progress(&state.book_id, state.current_progress());
    persist_book_state(&state, store)?;
    *config = state.config.clone();
    config.save()?;
    Ok(())
}

fn apply_effects(
    effects: Vec<ReaderEffect>,
    state: &mut ReaderState,
    store: &mut Store,
    config: &mut ReaderConfig,
) -> Result<bool> {
    let mut exit = false;
    for effect in effects {
        match effect {
            ReaderEffect::SaveProgress(progress) => {
                store.set_progress(&state.book_id, progress);
                persist_book_state(state, store)?;
            }
            ReaderEffect::SaveConfig => {
                *config = state.config.clone();
                config.save()?;
            }
            ReaderEffect::AddBookmark { label, progress } => {
                let bookmark = store.add_bookmark(&state.book_id, label, progress)?;
                state.notify(
                    if state.config.discreet_mode {
                        "Bookmark saved".to_string()
                    } else {
                        format!("Bookmark added: {}", bookmark.id)
                    },
                    NotificationKind::Success,
                );
                persist_book_state(state, store)?;
            }
            ReaderEffect::Exit => exit = true,
        }
    }
    Ok(exit)
}

fn persist_book_state(state: &ReaderState, store: &Store) -> Result<()> {
    store.save()?;
    if let Some(directory) = state.config.bookshelf_dir.as_deref() {
        crate::bookshelf::sync_sidecar_for_book(store, &state.book_id, directory)?;
    }
    Ok(())
}

fn should_handle_key_event(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn only_key_presses_reach_the_state_machine() {
        assert!(should_handle_key_event(KeyEvent::new(
            KeyCode::Char('g'),
            KeyModifiers::NONE
        )));
        assert!(!should_handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Char('g'),
            KeyModifiers::NONE,
            KeyEventKind::Release
        )));
    }
}
