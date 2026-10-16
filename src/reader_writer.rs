//! Serialized filesystem effects. The terminal thread never waits for book writes.
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use crate::config::ReaderConfig;
use crate::reading_data::{ReadingDataStore, SessionCheckpoint};
use crate::storage::{Bookmark, Progress, Store};

pub(crate) enum WriteCommand {
    Progress(Progress),
    Config(ReaderConfig),
    Add {
        label: String,
        progress: Progress,
    },
    Delete(String),
    Session(SessionCheckpoint),
    Finish {
        progress: Progress,
        config: ReaderConfig,
    },
}

impl WriteCommand {
    fn barrier(&self) -> bool {
        matches!(
            self,
            Self::Add { .. } | Self::Delete(_) | Self::Finish { .. }
        )
    }

    fn replaces(&self, older: &Self) -> bool {
        match (self, older) {
            (Self::Progress(_), Self::Progress(_)) | (Self::Config(_), Self::Config(_)) => true,
            (Self::Session(new), Self::Session(old)) => new.id == old.id,
            _ => false,
        }
    }
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<WriteCommand>,
    closed: bool,
}

impl Queue {
    fn push(&mut self, command: WriteCommand) -> bool {
        if self.closed {
            return false;
        }
        // Coalesce only before the next ordered bookmark operation. Different
        // reading sessions must retain their separate journal records.
        if !command.barrier() {
            for previous in self.jobs.iter_mut().rev() {
                if previous.barrier() {
                    break;
                }
                if command.replaces(previous) {
                    *previous = command;
                    return true;
                }
            }
        }
        if self.jobs.len() >= 128 && !matches!(command, WriteCommand::Finish { .. }) {
            return false;
        }
        self.jobs.push_back(command);
        true
    }
}

pub(crate) struct WriteReceipt {
    pub text: &'static str,
    pub error: bool,
    pub checkpoint_saved: bool,
    pub bookmarks: Option<Vec<Bookmark>>,
}

pub(crate) struct WriterOutcome {
    pub store: Store,
    pub flushed: bool,
    pub had_errors: bool,
}

pub(crate) struct ReaderWriter {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    pub receipts: mpsc::Receiver<WriteReceipt>,
    worker: Option<JoinHandle<WriterOutcome>>,
}

impl ReaderWriter {
    pub fn start(
        store: Store,
        config: ReaderConfig,
        data: ReadingDataStore,
        book_id: String,
    ) -> std::io::Result<Self> {
        Self::start_with_hook(store, config, data, book_id, || {})
    }

    fn start_with_hook(
        mut store: Store,
        mut config: ReaderConfig,
        data: ReadingDataStore,
        book_id: String,
        mut before_write: impl FnMut() + Send + 'static,
    ) -> std::io::Result<Self> {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let work = Arc::clone(&queue);
        let (sender, receipts) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("reader-persistence".into())
            .spawn(move || {
                let mut flushed = false;
                let mut had_errors = false;
                loop {
                    let (lock, wake) = &*work;
                    let Ok(mut pending) = lock.lock() else {
                        break;
                    };
                    while pending.jobs.is_empty() && !pending.closed {
                        let Ok(next) = wake.wait(pending) else {
                            return WriterOutcome {
                                store,
                                flushed: false,
                                had_errors: true,
                            };
                        };
                        pending = next;
                    }
                    let Some(command) = pending.jobs.pop_front() else {
                        break;
                    };
                    drop(pending); // No I/O under the queue lock.
                    before_write();
                    let finishing = matches!(command, WriteCommand::Finish { .. });
                    let receipt = execute(command, &mut store, &mut config, &data, &book_id);
                    had_errors |= receipt.error;
                    if finishing {
                        flushed = !receipt.error;
                    }
                    let _ = sender.send(receipt);
                }
                WriterOutcome {
                    store,
                    flushed,
                    had_errors,
                }
            })?;
        Ok(Self {
            queue,
            receipts,
            worker: Some(worker),
        })
    }

    pub fn enqueue(&self, command: WriteCommand) -> bool {
        let (lock, wake) = &*self.queue;
        let Ok(mut pending) = lock.lock() else {
            return false;
        };
        let accepted = pending.push(command);
        wake.notify_one();
        accepted
    }

    pub fn finish(
        mut self,
        progress: Progress,
        config: ReaderConfig,
    ) -> anyhow::Result<WriterOutcome> {
        self.enqueue(WriteCommand::Finish { progress, config });
        self.close();
        self.worker
            .take()
            .ok_or_else(|| anyhow::anyhow!("Persistence worker unavailable"))?
            .join()
            .map_err(|_| anyhow::anyhow!("Persistence worker failed"))
    }

    fn close(&self) {
        let (lock, wake) = &*self.queue;
        if let Ok(mut queue) = lock.lock() {
            queue.closed = true;
        }
        wake.notify_one();
    }
}

impl Drop for ReaderWriter {
    fn drop(&mut self) {
        self.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn save_state(store: &Store, config: &ReaderConfig, book_id: &str) -> anyhow::Result<bool> {
    store.save()?;
    Ok(config.bookshelf_dir.as_deref().is_some_and(|directory| {
        crate::bookshelf::sync_sidecar_for_book(store, book_id, directory).is_err()
    }))
}

fn execute(
    command: WriteCommand,
    store: &mut Store,
    config: &mut ReaderConfig,
    data: &ReadingDataStore,
    book_id: &str,
) -> WriteReceipt {
    let mut receipt = WriteReceipt {
        text: "",
        error: false,
        checkpoint_saved: false,
        bookmarks: None,
    };
    let finishing = matches!(command, WriteCommand::Finish { .. });
    let command = match command {
        WriteCommand::Finish {
            progress,
            config: latest,
        } => {
            *config = latest;
            WriteCommand::Progress(progress)
        }
        other => other,
    };
    match command {
        WriteCommand::Finish { .. } => unreachable!("finish normalized above"),
        WriteCommand::Progress(progress) => {
            store.set_progress(book_id, progress);
            match save_state(store, config, book_id) {
                Ok(true) => receipt.text = "State saved locally; sync pending",
                Ok(false) => {}
                Err(_) => {
                    receipt.text = "State save failed";
                    receipt.error = true;
                }
            }
        }
        WriteCommand::Config(next) => {
            *config = next;
            if config.save().is_err() {
                receipt.text = "Configuration save failed";
                receipt.error = true;
            }
        }
        WriteCommand::Session(checkpoint) => match data.upsert_event(&checkpoint) {
            Ok(warnings) if !warnings.is_empty() => {
                receipt.text = "Session saved locally; sync pending"
            }
            Ok(_) => {}
            Err(_) => {
                receipt.text = "Session save failed";
                receipt.error = true;
            }
        },
        WriteCommand::Add { label, progress } => {
            let previous = store.clone();
            let result = store
                .add_bookmark(book_id, label, progress)
                .and_then(|bookmark| data.append_bookmark_add(book_id, &bookmark));
            match result {
                Ok(warnings) => {
                    // The local journal is durable at this point, even if a
                    // sidecar or the materialized library needs reconciliation.
                    let pending =
                        save_state(store, config, book_id).unwrap_or(true) || !warnings.is_empty();
                    receipt.text = if pending {
                        "Checkpoint saved locally; sync pending"
                    } else {
                        "Checkpoint saved"
                    };
                    receipt.checkpoint_saved = true;
                }
                Err(_) => {
                    *store = previous;
                    receipt.text = "Checkpoint failed";
                    receipt.error = true;
                }
            }
            receipt.bookmarks = Some(store.bookmarks_for(book_id).to_vec());
        }
        WriteCommand::Delete(id) => {
            let previous = store.clone();
            let result = store
                .delete_bookmark(book_id, &id)
                .and_then(|_| data.append_bookmark_delete(book_id, &id));
            match result {
                Ok(warnings) => {
                    let pending =
                        save_state(store, config, book_id).unwrap_or(true) || !warnings.is_empty();
                    receipt.text = if pending {
                        "Checkpoint removed locally; sync pending"
                    } else {
                        "Checkpoint removed"
                    };
                }
                Err(_) => {
                    *store = previous;
                    receipt.text = "Checkpoint removal failed";
                    receipt.error = true;
                }
            }
            receipt.bookmarks = Some(store.bookmarks_for(book_id).to_vec());
        }
    }
    if finishing && config.save().is_err() {
        receipt.text = "Configuration save failed";
        receipt.error = true;
    }
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::TextPosition;

    fn fixture(
        root: &std::path::Path,
    ) -> (
        Store,
        ReaderConfig,
        ReadingDataStore,
        String,
        crate::book::BookDocument,
    ) {
        let file = root.join("fixture.txt");
        std::fs::write(&file, "private-body\n\nsecond paragraph").unwrap();
        let document = crate::book::load_book(&file).unwrap();
        let mut store = Store::open(root.join("library.json")).unwrap();
        let id = store.add_book(&document, &file).unwrap().id.clone();
        let config = ReaderConfig::open(root.join("configuration.json")).unwrap();
        let data = ReadingDataStore::new(root.join("journal"), None);
        (store, config, data, id, document)
    }

    #[test]
    fn blocked_writer_does_not_block_f12_and_bookmark_ack_follows_durability() {
        let dir = tempfile::tempdir().unwrap();
        let (store, config, data, id, document) = fixture(dir.path());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut first = true;
        let writer =
            ReaderWriter::start_with_hook(store, config.clone(), data, id.clone(), move || {
                if first {
                    first = false;
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
            })
            .unwrap();
        let position = Progress::new(TextPosition::new(0, 3));
        assert!(writer.enqueue(WriteCommand::Add {
            label: "checkpoint".into(),
            progress: position
        }));
        started_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(writer.receipts.try_recv().is_err());
        assert!(!dir.path().join("library.json").exists());
        let mut state = crate::reader_state::ReaderState::new(
            document,
            id.clone(),
            ratatui::layout::Rect::new(0, 0, 80, 24),
            config.clone(),
            position,
            Vec::new(),
        );
        state.update(crate::reader_state::ReaderAction::ToggleHidden);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| crate::reader_view::draw(f, &state))
            .unwrap();
        let screen = format!("{}", terminal.backend());
        release_tx.send(()).unwrap();
        assert!(screen.contains("Output folded"));
        assert!(!screen.contains("private-body"));
        let receipt = writer
            .receipts
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(receipt.checkpoint_saved && !receipt.error);
        let reopened = Store::open(dir.path().join("library.json")).unwrap();
        assert_eq!(reopened.bookmarks_for(&id)[0].position, position.position);
        let outcome = writer.finish(position, config).unwrap();
        assert!(outcome.flushed && !outcome.had_errors);
    }

    #[test]
    fn failed_journal_cannot_emit_a_successful_checkpoint_or_mutate_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let (store, config, data, id, _) = fixture(dir.path());
        std::fs::write(dir.path().join("journal"), "not a directory").unwrap();
        let writer = ReaderWriter::start(store, config.clone(), data, id.clone()).unwrap();
        writer.enqueue(WriteCommand::Add {
            label: "checkpoint".into(),
            progress: Progress::default(),
        });
        let receipt = writer
            .receipts
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(receipt.error && !receipt.checkpoint_saved);
        assert_eq!(receipt.text, "Checkpoint failed");
        assert!(receipt.bookmarks.unwrap().is_empty());
        let outcome = writer.finish(Progress::default(), config).unwrap();
        assert!(outcome.had_errors);
        assert!(outcome.store.bookmarks_for(&id).is_empty());
    }

    #[test]
    fn progress_coalesces_but_cannot_cross_a_bookmark_barrier() {
        let mut queue = Queue::default();
        let p = |n| Progress::new(TextPosition::new(0, n));
        queue.push(WriteCommand::Progress(p(1)));
        queue.push(WriteCommand::Progress(p(2)));
        queue.push(WriteCommand::Add {
            label: "x".into(),
            progress: p(2),
        });
        queue.push(WriteCommand::Progress(p(3)));
        queue.push(WriteCommand::Progress(p(4)));
        assert_eq!(queue.jobs.len(), 3);
        assert!(matches!(queue.jobs[0], WriteCommand::Progress(v) if v == p(2)));
        assert!(matches!(queue.jobs[2], WriteCommand::Progress(v) if v == p(4)));
    }

    #[test]
    fn queue_is_bounded_and_reserves_shutdown_without_dropping_bookmarks() {
        let mut queue = Queue::default();
        for _ in 0..128 {
            assert!(queue.push(WriteCommand::Add {
                label: "x".into(),
                progress: Progress::default()
            }));
        }
        assert!(!queue.push(WriteCommand::Delete("x".into())));
        assert!(queue.push(WriteCommand::Finish {
            progress: Progress::default(),
            config: ReaderConfig::default()
        }));
        assert_eq!(queue.jobs.len(), 129);
    }
}
