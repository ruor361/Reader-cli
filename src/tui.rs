use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;

use crate::book::BookDocument;
use crate::config::ReaderConfig;
use crate::reader_input::action_for_key;
use crate::reader_state::{NotificationKind, ReaderAction, ReaderEffect, ReaderState};
use crate::reading_data::{ReadingDataStore, SessionCheckpoint, SessionTracker};
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
    let data = ReadingDataStore::open_default(config)?;
    let sync_report = data.synchronize_store(store)?;
    for warning in sync_report.warnings {
        eprintln!("warning: {warning}");
    }
    if sync_report.state_changed {
        store.save()?;
    }

    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    let size = terminal.size()?;
    let viewport = Rect::new(0, 0, size.width, size.height);

    store.update_document_metrics(&book_id, &document);
    let saved = store.progress_for(&book_id).copied().unwrap_or_default();
    let bookmarks = store.bookmarks_for(&book_id).to_vec();
    store.record_open(&book_id);
    let mut state = ReaderState::new(
        document,
        book_id,
        viewport,
        config.clone(),
        saved,
        bookmarks,
    );
    let monotonic_now = Instant::now();
    let (mut session, initial_checkpoint) = SessionTracker::start(
        state.book_id.clone(),
        state.position,
        time::OffsetDateTime::now_utc(),
        monotonic_now,
    );
    persist_session_checkpoint(&data, &initial_checkpoint, &mut state)?;
    persist_book_state(&mut state, store)?;
    let mut exit = false;

    while !exit {
        let monotonic_now = Instant::now();
        state.expire_notification(monotonic_now);
        if let Some(checkpoint) = session.tick(
            state.position,
            time::OffsetDateTime::now_utc(),
            monotonic_now,
        ) {
            persist_session_checkpoint(&data, &checkpoint, &mut state)?;
        }
        terminal.draw(|frame| crate::reader_view::draw(frame, &state))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if should_handle_key_event(key) => {
                let action = action_for_key(key, &state);
                let was_hidden = state.hidden;
                let wall_now = time::OffsetDateTime::now_utc();
                let monotonic_now = Instant::now();

                if matches!(action, ReaderAction::ToggleHidden) && !was_hidden {
                    if let Some(checkpoint) =
                        session.finish(state.position, wall_now, monotonic_now)
                    {
                        persist_session_checkpoint(&data, &checkpoint, &mut state)?;
                    }
                } else if !was_hidden && !matches!(action, ReaderAction::None) {
                    if let Some(checkpoint) =
                        session.on_activity(state.position, wall_now, monotonic_now)
                    {
                        persist_session_checkpoint(&data, &checkpoint, &mut state)?;
                    }
                }

                let previous_position = state.position;
                let effects = state.update(action);
                exit = apply_effects(effects, &mut state, store, config, &data)?;

                if matches!(action, ReaderAction::ToggleHidden) && was_hidden && !state.hidden {
                    if let Some(checkpoint) = session.on_activity(
                        state.position,
                        time::OffsetDateTime::now_utc(),
                        Instant::now(),
                    ) {
                        persist_session_checkpoint(&data, &checkpoint, &mut state)?;
                    }
                } else if exit {
                    if let Some(checkpoint) = session.finish(
                        state.position,
                        time::OffsetDateTime::now_utc(),
                        Instant::now(),
                    ) {
                        persist_session_checkpoint(&data, &checkpoint, &mut state)?;
                    }
                } else if state.position != previous_position {
                    if let Some(checkpoint) = session.checkpoint(
                        state.position,
                        time::OffsetDateTime::now_utc(),
                        Instant::now(),
                        true,
                    ) {
                        persist_session_checkpoint(&data, &checkpoint, &mut state)?;
                    }
                }
            }
            Event::Resize(width, height) => state.resize(width, height),
            _ => {}
        }
    }

    store.set_progress(&state.book_id, state.current_progress());
    persist_book_state(&mut state, store)?;
    *config = state.config.clone();
    config.save()?;
    Ok(())
}

fn apply_effects(
    effects: Vec<ReaderEffect>,
    state: &mut ReaderState,
    store: &mut Store,
    config: &mut ReaderConfig,
    data: &ReadingDataStore,
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
                let warnings = data.append_bookmark_add(&state.book_id, &bookmark)?;
                state.bookmarks.push(bookmark.clone());
                state.notify(
                    if !warnings.is_empty() {
                        "Bookmark saved locally; cloud sync is pending".to_string()
                    } else if state.config.discreet_mode {
                        "Bookmark saved".to_string()
                    } else {
                        format!(
                            "Bookmark added: {}",
                            bookmark.id.chars().take(12).collect::<String>()
                        )
                    },
                    NotificationKind::Success,
                );
                persist_book_state(state, store)?;
            }
            ReaderEffect::DeleteBookmark { id } => {
                if let Ok(removed) = store.delete_bookmark(&state.book_id, &id) {
                    let warnings = data.append_bookmark_delete(&state.book_id, &removed.id)?;
                    state.notify(
                        if !warnings.is_empty() {
                            "Bookmark deleted locally; cloud sync is pending".to_string()
                        } else {
                            format!(
                                "Bookmark deleted: {}",
                                removed.id.chars().take(12).collect::<String>()
                            )
                        },
                        NotificationKind::Info,
                    );
                    persist_book_state(state, store)?;
                }
            }
            ReaderEffect::Exit => exit = true,
        }
    }
    Ok(exit)
}

fn persist_session_checkpoint(
    data: &ReadingDataStore,
    checkpoint: &SessionCheckpoint,
    state: &mut ReaderState,
) -> Result<()> {
    let warnings = data.upsert_event(checkpoint)?;
    if !warnings.is_empty() {
        state.notify(
            "Reading data saved locally; cloud sync is pending",
            NotificationKind::Info,
        );
    }
    Ok(())
}

fn persist_book_state(state: &mut ReaderState, store: &Store) -> Result<()> {
    store.save()?;
    if let Some(directory) = state.config.bookshelf_dir.clone() {
        if crate::bookshelf::sync_sidecar_for_book(store, &state.book_id, &directory).is_err() {
            state.notify(
                "Book state saved locally; sidecar sync is pending",
                NotificationKind::Info,
            );
        }
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
