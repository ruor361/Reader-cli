use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::book::TextPosition;
use crate::config::ReaderConfig;
use crate::storage::{Bookmark, Progress, Store};

pub const SYNC_SCHEMA_VERSION: u32 = 1;
pub const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(60);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

const EVENTS_DIR: &str = "reading-events";
const BOOKMARK_OPS_DIR: &str = "bookmark-ops";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReadingEvent {
    pub book_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ended_at: OffsetDateTime,
    pub start_position: TextPosition,
    pub end_position: TextPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReadingEvent {
    pub id: String,
    pub event: ReadingEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BookmarkOperation {
    pub book_id: String,
    pub bookmark_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
    #[serde(flatten)]
    pub action: BookmarkOperationAction,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BookmarkOperationAction {
    Add {
        label: String,
        position: TextPosition,
    },
    Delete,
}

#[derive(Debug, Default, Clone)]
pub struct ReadingDataSyncReport {
    pub events_copied: usize,
    pub bookmark_ops_copied: usize,
    pub state_changed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReadingDataStore {
    local_root: PathBuf,
    cloud_root: Option<PathBuf>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SyncManifest {
    schema_version: u32,
}

#[derive(Debug, Clone)]
pub struct SessionCheckpoint {
    pub id: String,
    pub event: ReadingEvent,
}

#[derive(Debug)]
struct ActiveSession {
    id: String,
    event: ReadingEvent,
    last_activity: Instant,
    last_activity_wall: OffsetDateTime,
    last_checkpoint: Instant,
}

#[derive(Debug)]
pub struct SessionTracker {
    book_id: String,
    active: Option<ActiveSession>,
}

impl ReadingDataStore {
    pub fn open_default(config: &ReaderConfig) -> Result<Self> {
        let local_root = crate::paths::reading_data_dir()?;
        let cloud_root = config
            .bookshelf_dir
            .as_deref()
            .map(|directory| directory.join(".reader"));
        Ok(Self::new(local_root, cloud_root))
    }

    pub fn new(local_root: PathBuf, cloud_root: Option<PathBuf>) -> Self {
        Self {
            local_root,
            cloud_root,
        }
    }

    pub fn local_root(&self) -> &Path {
        &self.local_root
    }

    pub fn cloud_root(&self) -> Option<&Path> {
        self.cloud_root.as_deref()
    }

    pub fn reconcile(&self) -> Result<ReadingDataSyncReport> {
        ensure_root(&self.local_root)?;
        let mut report = ReadingDataSyncReport::default();
        let Some(cloud_root) = self.cloud_root.as_deref() else {
            return Ok(report);
        };
        if let Err(err) = ensure_cloud_root(cloud_root) {
            report.warnings.push(format!(
                "Reading-data cloud mirror is unavailable at {}: {err}",
                cloud_root.display()
            ));
            return Ok(report);
        }

        merge_category(
            &self.local_root,
            cloud_root,
            EVENTS_DIR,
            RecordKind::ReadingEvent,
            &mut report,
        );
        merge_category(
            &self.local_root,
            cloud_root,
            BOOKMARK_OPS_DIR,
            RecordKind::BookmarkOperation,
            &mut report,
        );
        Ok(report)
    }

    pub fn upsert_event(&self, checkpoint: &SessionCheckpoint) -> Result<Vec<String>> {
        ensure_root(&self.local_root)?;
        validate_event(&checkpoint.event)?;
        let relative = event_relative_path(&checkpoint.event.book_id, &checkpoint.id);
        write_json_record(
            &self.local_root.join(&relative),
            &checkpoint.event,
            "reading event",
        )?;

        let mut warnings = Vec::new();
        if let Some(cloud_root) = self.cloud_root.as_deref() {
            if let Err(err) = ensure_cloud_root(cloud_root).and_then(|()| {
                write_json_record(
                    &cloud_root.join(relative),
                    &checkpoint.event,
                    "cloud reading event",
                )
            }) {
                warnings.push(format!(
                    "Reading event was saved locally but could not be mirrored to {}: {err}",
                    cloud_root.display()
                ));
            }
        }
        Ok(warnings)
    }

    pub fn append_bookmark_add(&self, book_id: &str, bookmark: &Bookmark) -> Result<Vec<String>> {
        let operation = BookmarkOperation {
            book_id: book_id.to_string(),
            bookmark_id: bookmark.id.clone(),
            occurred_at: OffsetDateTime::now_utc(),
            action: BookmarkOperationAction::Add {
                label: bookmark.label.clone(),
                position: bookmark.position,
            },
        };
        self.append_bookmark_operation(&Uuid::new_v4().to_string(), &operation)
    }

    pub fn append_bookmark_delete(&self, book_id: &str, bookmark_id: &str) -> Result<Vec<String>> {
        let operation = BookmarkOperation {
            book_id: book_id.to_string(),
            bookmark_id: bookmark_id.to_string(),
            occurred_at: OffsetDateTime::now_utc(),
            action: BookmarkOperationAction::Delete,
        };
        self.append_bookmark_operation(&Uuid::new_v4().to_string(), &operation)
    }

    pub fn ensure_bookmark_seed(
        &self,
        book_id: &str,
        bookmark: &Bookmark,
        occurred_at: Option<SystemTime>,
    ) -> Result<Vec<String>> {
        let seed = format!(
            "reader-bookmark-seed|{book_id}|{}|{}|{}|{}",
            bookmark.id,
            bookmark.label,
            bookmark.position.section_index,
            bookmark.position.char_offset
        );
        let operation_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string();
        let operation = BookmarkOperation {
            book_id: book_id.to_string(),
            bookmark_id: bookmark.id.clone(),
            occurred_at: occurred_at
                .map(OffsetDateTime::from)
                .unwrap_or(OffsetDateTime::UNIX_EPOCH),
            action: BookmarkOperationAction::Add {
                label: bookmark.label.clone(),
                position: bookmark.position,
            },
        };
        self.append_bookmark_operation(&operation_id, &operation)
    }

    pub fn events_strict(&self) -> Result<Vec<StoredReadingEvent>> {
        ensure_root(&self.local_root)?;
        if let Some(cloud_root) = self.cloud_root.as_deref() {
            ensure_cloud_root(cloud_root)?;
            let _ = load_events(cloud_root, true, &mut Vec::new())?;
        }
        let mut events = load_events(&self.local_root, true, &mut Vec::new())?;
        sort_events(&mut events);
        Ok(events)
    }

    pub fn write_export(path: &Path, contents: &[u8]) -> Result<()> {
        crate::atomic_file::write(path, contents)
            .with_context(|| format!("Failed to write export {}", path.display()))
    }

    pub fn synchronize_store(&self, store: &mut Store) -> Result<ReadingDataSyncReport> {
        let mut seed_warnings = Vec::new();
        for book in store.library().books() {
            for bookmark in &book.bookmarks {
                seed_warnings.extend(self.ensure_bookmark_seed(
                    &book.id,
                    bookmark,
                    book.last_opened,
                )?);
            }
        }
        let mut report = self.reconcile()?;
        report.warnings.extend(seed_warnings);
        let events = load_events(&self.local_root, false, &mut report.warnings)?;
        let operations = load_bookmark_operations(&self.local_root, false, &mut report.warnings)?;

        let mut latest_by_book: HashMap<String, StoredReadingEvent> = HashMap::new();
        for stored in events {
            let replace = latest_by_book
                .get(&stored.event.book_id)
                .is_none_or(|existing| compare_stored_events(existing, &stored) == Ordering::Less);
            if replace {
                latest_by_book.insert(stored.event.book_id.clone(), stored);
            }
        }

        let mut latest_book: Option<StoredReadingEvent> = None;
        for stored in latest_by_book.values() {
            if store.book_by_id(&stored.event.book_id).is_none() {
                continue;
            }
            if let Some(last_opened) = system_time_from_offset(stored.event.ended_at) {
                report.state_changed |= store.apply_synced_progress(
                    &stored.event.book_id,
                    Progress::new(stored.event.end_position),
                    last_opened,
                );
            }
            if latest_book
                .as_ref()
                .is_none_or(|existing| compare_stored_events(existing, stored) == Ordering::Less)
            {
                latest_book = Some(stored.clone());
            }
        }
        if let Some(latest) = latest_book {
            report.state_changed |= store.set_last_book_id(&latest.event.book_id);
        }

        let mut additions: HashMap<(String, String), (String, TextPosition)> = HashMap::new();
        let mut deletions: HashSet<(String, String)> = HashSet::new();
        for (_, operation) in operations {
            let key = (operation.book_id.clone(), operation.bookmark_id.clone());
            match operation.action {
                BookmarkOperationAction::Add { label, position } => {
                    additions.insert(key, (label, position));
                }
                BookmarkOperationAction::Delete => {
                    deletions.insert(key);
                }
            }
        }
        for ((book_id, bookmark_id), (label, position)) in additions {
            if deletions.contains(&(book_id.clone(), bookmark_id.clone())) {
                continue;
            }
            report.state_changed |= store.merge_bookmark(
                &book_id,
                Bookmark {
                    id: bookmark_id,
                    label,
                    position,
                },
            );
        }
        for (book_id, bookmark_id) in deletions {
            report.state_changed |= store.remove_synced_bookmark(&book_id, &bookmark_id);
        }

        Ok(report)
    }

    fn append_bookmark_operation(
        &self,
        operation_id: &str,
        operation: &BookmarkOperation,
    ) -> Result<Vec<String>> {
        ensure_root(&self.local_root)?;
        validate_bookmark_operation(operation)?;
        let relative = bookmark_operation_relative_path(&operation.book_id, operation_id);
        write_json_record(
            &self.local_root.join(&relative),
            operation,
            "bookmark operation",
        )?;

        let mut warnings = Vec::new();
        if let Some(cloud_root) = self.cloud_root.as_deref() {
            if let Err(err) = ensure_cloud_root(cloud_root).and_then(|()| {
                write_json_record(
                    &cloud_root.join(relative),
                    operation,
                    "cloud bookmark operation",
                )
            }) {
                warnings.push(format!(
                    "Bookmark change was saved locally but could not be mirrored to {}: {err}",
                    cloud_root.display()
                ));
            }
        }
        Ok(warnings)
    }
}

impl SessionTracker {
    pub fn start(
        book_id: String,
        position: TextPosition,
        wall_now: OffsetDateTime,
        monotonic_now: Instant,
    ) -> (Self, SessionCheckpoint) {
        let active = new_active_session(&book_id, position, wall_now, monotonic_now);
        let checkpoint = checkpoint_from_active(&active);
        (
            Self {
                book_id,
                active: Some(active),
            },
            checkpoint,
        )
    }

    pub fn on_activity(
        &mut self,
        position: TextPosition,
        wall_now: OffsetDateTime,
        monotonic_now: Instant,
    ) -> Option<SessionCheckpoint> {
        if let Some(active) = self.active.as_mut() {
            active.last_activity = monotonic_now;
            active.last_activity_wall = wall_now.max(active.event.started_at);
            return None;
        }
        let active = new_active_session(&self.book_id, position, wall_now, monotonic_now);
        let checkpoint = checkpoint_from_active(&active);
        self.active = Some(active);
        Some(checkpoint)
    }

    pub fn checkpoint(
        &mut self,
        position: TextPosition,
        wall_now: OffsetDateTime,
        monotonic_now: Instant,
        force: bool,
    ) -> Option<SessionCheckpoint> {
        let active = self.active.as_mut()?;
        if !force
            && monotonic_now.saturating_duration_since(active.last_checkpoint) < CHECKPOINT_INTERVAL
        {
            return None;
        }
        active.event.ended_at = wall_now.max(active.event.started_at);
        active.event.end_position = position;
        active.last_checkpoint = monotonic_now;
        Some(checkpoint_from_active(active))
    }

    pub fn tick(
        &mut self,
        position: TextPosition,
        wall_now: OffsetDateTime,
        monotonic_now: Instant,
    ) -> Option<SessionCheckpoint> {
        let active = self.active.as_mut()?;
        if monotonic_now.saturating_duration_since(active.last_activity) >= IDLE_TIMEOUT {
            active.event.ended_at = (active.last_activity_wall
                + time::Duration::seconds(IDLE_TIMEOUT.as_secs() as i64))
            .max(active.event.started_at);
            active.event.end_position = position;
            let checkpoint = checkpoint_from_active(active);
            self.active = None;
            return Some(checkpoint);
        }
        self.checkpoint(position, wall_now, monotonic_now, false)
    }

    pub fn finish(
        &mut self,
        position: TextPosition,
        wall_now: OffsetDateTime,
        monotonic_now: Instant,
    ) -> Option<SessionCheckpoint> {
        let mut active = self.active.take()?;
        let idle_end =
            active.last_activity_wall + time::Duration::seconds(IDLE_TIMEOUT.as_secs() as i64);
        active.event.ended_at =
            if monotonic_now.saturating_duration_since(active.last_activity) >= IDLE_TIMEOUT {
                idle_end
            } else {
                wall_now
            }
            .max(active.event.started_at);
        active.event.end_position = position;
        Some(checkpoint_from_active(&active))
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

fn new_active_session(
    book_id: &str,
    position: TextPosition,
    wall_now: OffsetDateTime,
    monotonic_now: Instant,
) -> ActiveSession {
    ActiveSession {
        id: Uuid::new_v4().to_string(),
        event: ReadingEvent {
            book_id: book_id.to_string(),
            started_at: wall_now,
            ended_at: wall_now,
            start_position: position,
            end_position: position,
        },
        last_activity: monotonic_now,
        last_activity_wall: wall_now,
        last_checkpoint: monotonic_now,
    }
}

fn checkpoint_from_active(active: &ActiveSession) -> SessionCheckpoint {
    SessionCheckpoint {
        id: active.id.clone(),
        event: active.event.clone(),
    }
}

fn ensure_root(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join(EVENTS_DIR))
        .with_context(|| format!("Failed to create {}", root.join(EVENTS_DIR).display()))?;
    fs::create_dir_all(root.join(BOOKMARK_OPS_DIR))
        .with_context(|| format!("Failed to create {}", root.join(BOOKMARK_OPS_DIR).display()))?;
    let manifest_path = root.join(MANIFEST_FILE);
    if manifest_path.exists() {
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: SyncManifest = serde_json::from_str(&text)
            .with_context(|| format!("Invalid sync manifest {}", manifest_path.display()))?;
        if manifest.schema_version != SYNC_SCHEMA_VERSION {
            bail!(
                "Unsupported sync schema {} at {}",
                manifest.schema_version,
                manifest_path.display()
            );
        }
    } else {
        write_json_record(
            &manifest_path,
            &SyncManifest {
                schema_version: SYNC_SCHEMA_VERSION,
            },
            "sync manifest",
        )?;
    }
    Ok(())
}

fn ensure_cloud_root(root: &Path) -> Result<()> {
    let bookshelf = root
        .parent()
        .with_context(|| format!("Cloud reading-data path has no parent: {}", root.display()))?;
    if !bookshelf.is_dir() {
        bail!(
            "Configured bookshelf directory is unavailable: {}",
            bookshelf.display()
        );
    }
    ensure_root(root)
}

#[derive(Clone, Copy)]
enum RecordKind {
    ReadingEvent,
    BookmarkOperation,
}

fn merge_category(
    local_root: &Path,
    cloud_root: &Path,
    category: &str,
    kind: RecordKind,
    report: &mut ReadingDataSyncReport,
) {
    let local = match collect_json_files(&local_root.join(category)) {
        Ok(files) => files,
        Err(err) => {
            report.warnings.push(err.to_string());
            return;
        }
    };
    let cloud = match collect_json_files(&cloud_root.join(category)) {
        Ok(files) => files,
        Err(err) => {
            report.warnings.push(err.to_string());
            return;
        }
    };
    let keys = local
        .keys()
        .chain(cloud.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();

    for relative in keys {
        let local_path = local.get(&relative);
        let cloud_path = cloud.get(&relative);
        let result = match (local_path, cloud_path, kind) {
            (Some(local), None, RecordKind::ReadingEvent) => {
                copy_valid_event(local, &cloud_root.join(category).join(&relative))
            }
            (None, Some(cloud), RecordKind::ReadingEvent) => {
                copy_valid_event(cloud, &local_root.join(category).join(&relative))
            }
            (Some(local), Some(cloud), RecordKind::ReadingEvent) => merge_event_pair(local, cloud),
            (Some(local), None, RecordKind::BookmarkOperation) => {
                copy_valid_bookmark_operation(local, &cloud_root.join(category).join(&relative))
            }
            (None, Some(cloud), RecordKind::BookmarkOperation) => {
                copy_valid_bookmark_operation(cloud, &local_root.join(category).join(&relative))
            }
            (Some(local), Some(cloud), RecordKind::BookmarkOperation) => {
                merge_bookmark_operation_pair(local, cloud)
            }
            (None, None, _) => Ok(false),
        };
        match result {
            Ok(true) => match kind {
                RecordKind::ReadingEvent => report.events_copied += 1,
                RecordKind::BookmarkOperation => report.bookmark_ops_copied += 1,
            },
            Ok(false) => {}
            Err(err) => report.warnings.push(err.to_string()),
        }
    }
}

fn collect_json_files(root: &Path) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let mut files = BTreeMap::new();
    collect_json_files_inner(root, root, &mut files)?;
    Ok(files)
}

fn collect_json_files_inner(
    base: &Path,
    current: &Path,
    files: &mut BTreeMap<PathBuf, PathBuf>,
) -> Result<()> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)
        .with_context(|| format!("Failed to read sync directory {}", current.display()))?
    {
        let entry =
            entry.with_context(|| format!("Failed to read an entry in {}", current.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files_inner(base, &path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let relative = path
                .strip_prefix(base)
                .expect("collected path is below its base")
                .to_path_buf();
            files.insert(relative, path);
        }
    }
    Ok(())
}

fn event_relative_path(book_id: &str, event_id: &str) -> PathBuf {
    PathBuf::from(EVENTS_DIR)
        .join(book_id)
        .join(format!("{event_id}.json"))
}

fn bookmark_operation_relative_path(book_id: &str, operation_id: &str) -> PathBuf {
    PathBuf::from(BOOKMARK_OPS_DIR)
        .join(book_id)
        .join(format!("{operation_id}.json"))
}

fn write_json_record<T: serde::Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)
        .with_context(|| format!("Failed to serialize {label}"))?;
    crate::atomic_file::write(path, text.as_bytes())
        .with_context(|| format!("Failed to write {label} {}", path.display()))
}

fn read_event(path: &Path) -> Result<ReadingEvent> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read reading event {}", path.display()))?;
    let event: ReadingEvent = serde_json::from_str(&text)
        .with_context(|| format!("Invalid reading event {}", path.display()))?;
    validate_event(&event).with_context(|| format!("Invalid reading event {}", path.display()))?;
    Ok(event)
}

fn validate_event(event: &ReadingEvent) -> Result<()> {
    if event.book_id.is_empty() {
        bail!("book_id is empty");
    }
    if event.ended_at < event.started_at {
        bail!("ended_at precedes started_at");
    }
    Ok(())
}

fn read_bookmark_operation(path: &Path) -> Result<BookmarkOperation> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read bookmark operation {}", path.display()))?;
    let operation: BookmarkOperation = serde_json::from_str(&text)
        .with_context(|| format!("Invalid bookmark operation {}", path.display()))?;
    validate_bookmark_operation(&operation)
        .with_context(|| format!("Invalid bookmark operation {}", path.display()))?;
    Ok(operation)
}

fn validate_bookmark_operation(operation: &BookmarkOperation) -> Result<()> {
    if operation.book_id.is_empty() {
        bail!("book_id is empty");
    }
    if operation.bookmark_id.is_empty() {
        bail!("bookmark_id is empty");
    }
    Ok(())
}

fn copy_valid_event(source: &Path, destination: &Path) -> Result<bool> {
    let event = read_event(source)?;
    write_json_record(destination, &event, "reading event")?;
    Ok(true)
}

fn copy_valid_bookmark_operation(source: &Path, destination: &Path) -> Result<bool> {
    let operation = read_bookmark_operation(source)?;
    write_json_record(destination, &operation, "bookmark operation")?;
    Ok(true)
}

fn merge_event_pair(local: &Path, cloud: &Path) -> Result<bool> {
    let local_event = read_event(local)?;
    let cloud_event = read_event(cloud)?;
    match compare_events(&local_event, &cloud_event) {
        Ordering::Greater => write_json_record(cloud, &local_event, "reading event")?,
        Ordering::Less => write_json_record(local, &cloud_event, "reading event")?,
        Ordering::Equal => return Ok(false),
    }
    Ok(true)
}

fn merge_bookmark_operation_pair(local: &Path, cloud: &Path) -> Result<bool> {
    let local_operation = read_bookmark_operation(local)?;
    let cloud_operation = read_bookmark_operation(cloud)?;
    if local_operation == cloud_operation {
        return Ok(false);
    }
    let local_text = serde_json::to_string(&local_operation)?;
    let cloud_text = serde_json::to_string(&cloud_operation)?;
    if local_text > cloud_text {
        write_json_record(cloud, &local_operation, "bookmark operation")?;
    } else {
        write_json_record(local, &cloud_operation, "bookmark operation")?;
    }
    Ok(true)
}

fn load_events(
    root: &Path,
    strict: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<StoredReadingEvent>> {
    let files = collect_json_files(&root.join(EVENTS_DIR))?;
    let mut events = Vec::with_capacity(files.len());
    for (relative, path) in files {
        match read_event(&path) {
            Ok(event) => {
                let folder_book_id = relative
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                if folder_book_id != event.book_id {
                    let err = anyhow::anyhow!(
                        "Reading event {} belongs to book {} but is stored under {}",
                        path.display(),
                        event.book_id,
                        folder_book_id
                    );
                    if strict {
                        return Err(err);
                    }
                    warnings.push(err.to_string());
                    continue;
                }
                let id = relative
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string();
                events.push(StoredReadingEvent { id, event });
            }
            Err(err) if strict => return Err(err),
            Err(err) => warnings.push(err.to_string()),
        }
    }
    Ok(events)
}

fn load_bookmark_operations(
    root: &Path,
    strict: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<(String, BookmarkOperation)>> {
    let files = collect_json_files(&root.join(BOOKMARK_OPS_DIR))?;
    let mut operations = Vec::with_capacity(files.len());
    for (relative, path) in files {
        match read_bookmark_operation(&path) {
            Ok(operation) => {
                let folder_book_id = relative
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                if folder_book_id != operation.book_id {
                    let err = anyhow::anyhow!(
                        "Bookmark operation {} belongs to book {} but is stored under {}",
                        path.display(),
                        operation.book_id,
                        folder_book_id
                    );
                    if strict {
                        return Err(err);
                    }
                    warnings.push(err.to_string());
                    continue;
                }
                let id = relative
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string();
                operations.push((id, operation));
            }
            Err(err) if strict => return Err(err),
            Err(err) => warnings.push(err.to_string()),
        }
    }
    operations.sort_by(|left, right| {
        left.1
            .occurred_at
            .cmp(&right.1.occurred_at)
            .then_with(|| left.0.cmp(&right.0))
    });
    Ok(operations)
}

fn compare_events(left: &ReadingEvent, right: &ReadingEvent) -> Ordering {
    left.ended_at
        .cmp(&right.ended_at)
        .then_with(|| left.started_at.cmp(&right.started_at))
        .then_with(|| {
            serde_json::to_string(left)
                .unwrap_or_default()
                .cmp(&serde_json::to_string(right).unwrap_or_default())
        })
}

fn compare_stored_events(left: &StoredReadingEvent, right: &StoredReadingEvent) -> Ordering {
    compare_events(&left.event, &right.event).then_with(|| left.id.cmp(&right.id))
}

fn sort_events(events: &mut [StoredReadingEvent]) {
    events.sort_by(|left, right| {
        left.event
            .started_at
            .cmp(&right.event.started_at)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn system_time_from_offset(time: OffsetDateTime) -> Option<SystemTime> {
    let nanos = time.unix_timestamp_nanos();
    if nanos < 0 {
        return None;
    }
    let seconds = u64::try_from(nanos / 1_000_000_000).ok()?;
    let subsecond_nanos = u32::try_from(nanos % 1_000_000_000).ok()?;
    Some(UNIX_EPOCH + Duration::new(seconds, subsecond_nanos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookDocument, BookFormat, BookMetadata, Section};

    fn add_test_book(store: &mut Store, path: &Path, id: &str) {
        let document = BookDocument {
            id: id.to_string(),
            title: "Book".into(),
            path: path.to_path_buf(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: "Book".into(),
                author: None,
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path: path.to_path_buf(),
                section_count: 1,
            },
            sections: vec![Section {
                title: "Book".into(),
                text: "abcdefghij".into(),
            }],
            contents: Vec::new(),
            contents_source: crate::book::ContentsSource::Generated,
        };
        store.add_book(&document, path).unwrap();
    }

    #[test]
    fn reading_event_serializes_only_the_five_public_fields() {
        let event = ReadingEvent {
            book_id: "book".into(),
            started_at: OffsetDateTime::UNIX_EPOCH,
            ended_at: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
            start_position: TextPosition::new(0, 1),
            end_position: TextPosition::new(0, 4),
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert_eq!(value["started_at"], "1970-01-01T00:00:00Z");
    }

    #[test]
    fn two_stores_merge_events_and_latest_event_restores_backward_progress() {
        let dir = tempfile::tempdir().unwrap();
        let local_a = dir.path().join("a");
        let local_b = dir.path().join("b");
        let cloud = dir.path().join("cloud");
        let data_a = ReadingDataStore::new(local_a, Some(cloud.clone()));
        let data_b = ReadingDataStore::new(local_b, Some(cloud));
        let base = OffsetDateTime::UNIX_EPOCH + time::Duration::days(20_000);

        data_a
            .upsert_event(&SessionCheckpoint {
                id: "first".into(),
                event: ReadingEvent {
                    book_id: "book".into(),
                    started_at: base,
                    ended_at: base + time::Duration::minutes(10),
                    start_position: TextPosition::new(0, 0),
                    end_position: TextPosition::new(0, 8),
                },
            })
            .unwrap();
        data_b.reconcile().unwrap();
        data_b
            .upsert_event(&SessionCheckpoint {
                id: "second".into(),
                event: ReadingEvent {
                    book_id: "book".into(),
                    started_at: base + time::Duration::hours(1),
                    ended_at: base + time::Duration::hours(1) + time::Duration::minutes(2),
                    start_position: TextPosition::new(0, 8),
                    end_position: TextPosition::new(0, 3),
                },
            })
            .unwrap();

        let library_path = dir.path().join("library.json");
        let book_path = dir.path().join("book.txt");
        fs::write(&book_path, "abcdefghij").unwrap();
        let mut store = Store::open(library_path).unwrap();
        add_test_book(&mut store, &book_path, "book");
        let report = data_a.synchronize_store(&mut store).unwrap();

        assert!(report.state_changed);
        assert_eq!(
            store.progress_for("book").unwrap().position,
            TextPosition::new(0, 3)
        );
        assert_eq!(data_a.events_strict().unwrap().len(), 2);
    }

    #[test]
    fn bookmark_additions_merge_and_delete_tombstone_wins() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("local");
        let cloud = dir.path().join("cloud");
        let data = ReadingDataStore::new(local, Some(cloud));
        let first = Bookmark {
            id: "one".into(),
            label: "First".into(),
            position: TextPosition::new(0, 2),
        };
        let second = Bookmark {
            id: "two".into(),
            label: "Second".into(),
            position: TextPosition::new(0, 5),
        };
        data.append_bookmark_add("book", &first).unwrap();
        data.append_bookmark_add("book", &second).unwrap();
        data.append_bookmark_delete("book", "one").unwrap();

        let library_path = dir.path().join("library.json");
        let book_path = dir.path().join("book.txt");
        fs::write(&book_path, "abcdefghij").unwrap();
        let mut store = Store::open(library_path).unwrap();
        add_test_book(&mut store, &book_path, "book");
        data.synchronize_store(&mut store).unwrap();

        assert_eq!(store.bookmarks_for("book"), &[second]);
    }

    #[test]
    fn tracker_checkpoints_and_splits_after_fifteen_idle_minutes() {
        let monotonic = Instant::now();
        let wall = OffsetDateTime::UNIX_EPOCH;
        let (mut tracker, initial) =
            SessionTracker::start("book".into(), TextPosition::new(0, 1), wall, monotonic);
        assert_eq!(initial.event.started_at, initial.event.ended_at);

        let minute = tracker
            .tick(
                TextPosition::new(0, 2),
                wall + time::Duration::minutes(1),
                monotonic + CHECKPOINT_INTERVAL,
            )
            .unwrap();
        assert_eq!(minute.event.end_position, TextPosition::new(0, 2));

        let idle = tracker
            .tick(
                TextPosition::new(0, 3),
                wall + time::Duration::minutes(15),
                monotonic + IDLE_TIMEOUT,
            )
            .unwrap();
        assert_eq!(idle.event.ended_at, wall + time::Duration::minutes(15));
        assert!(!tracker.is_active());

        let resumed = tracker
            .on_activity(
                TextPosition::new(0, 3),
                wall + time::Duration::minutes(16),
                monotonic + IDLE_TIMEOUT + Duration::from_secs(60),
            )
            .unwrap();
        assert_ne!(resumed.id, initial.id);
    }

    #[test]
    fn strict_export_validation_rejects_a_corrupt_cloud_event() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("local");
        let cloud = dir.path().join("cloud");
        let data = ReadingDataStore::new(local, Some(cloud.clone()));
        data.reconcile().unwrap();
        let corrupt = cloud.join(EVENTS_DIR).join("book").join("broken.json");
        fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
        fs::write(&corrupt, "{not-json").unwrap();

        assert!(data
            .events_strict()
            .unwrap_err()
            .to_string()
            .contains("Invalid"));
        assert!(corrupt.exists(), "corrupt cloud data must be preserved");
    }
}
