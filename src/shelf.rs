use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::book::BookFormat;
use crate::config::ReaderConfig;
use crate::storage::{LibraryBook, ScanDirResult, Store};
use crate::ui::{
    action_line, centered_rect, command_for, fill_background, help_lines, hud_chunks, KeyBinding,
    KeyChord, Palette,
};

pub enum ShelfOutcome {
    Exit,
    Open(LibraryBook),
}

const PROGRESS_BAR_WIDTH: usize = 5;

pub fn run_shelf(
    store: &mut Store,
    config: &mut ReaderConfig,
    scan_result: Option<ScanDirResult>,
) -> Result<ShelfOutcome> {
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    let size = terminal.size()?;
    let viewport = Rect::new(0, 0, size.width, size.height);

    let mut app = ShelfApp::new(store, viewport, scan_result);

    let outcome = loop {
        terminal.draw(|frame| draw(frame, &app, store, config))?;
        if !event::poll(std::time::Duration::from_millis(200))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if let Some(outcome) = handle_key(key, &mut app, store, config)? {
                    break outcome;
                }
            }
            Event::Resize(width, height) => {
                app.viewport = Rect::new(0, 0, width, height);
            }
            _ => {}
        }
    };
    Ok(outcome)
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShelfSort {
    Recent,
    Title,
    Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShelfMode {
    Browse,
    Search,
    ConfirmDelete,
    ConfirmDeleteFile,
    Help,
}

#[derive(Debug, Clone)]
struct ShelfRow {
    id: String,
    title: String,
    author: Option<String>,
    format: BookFormat,
    progress_tenths: u16,
    last_opened: Option<SystemTime>,
    bookmark_count: usize,
    missing: bool,
    #[allow(dead_code)]
    path: PathBuf,
}

struct ShelfApp {
    books: Vec<ShelfRow>,
    visible: Vec<usize>,
    selection: usize,
    sort: ShelfSort,
    query: String,
    mode: ShelfMode,
    viewport: Rect,
    message: String,
    pending_delete_file: Option<PathBuf>,
}

impl ShelfApp {
    fn new(store: &Store, viewport: Rect, scan_result: Option<ScanDirResult>) -> Self {
        let mut app = Self {
            books: Vec::new(),
            visible: Vec::new(),
            selection: 0,
            sort: ShelfSort::Recent,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport,
            message: String::new(),
            pending_delete_file: None,
        };
        app.refresh_from_store(store);
        if let Some(result) = scan_result {
            if result.imported > 0 || result.updated > 0 {
                app.message = format!("Imported {} (+{} updated)", result.imported, result.updated);
            } else if !result.errors.is_empty() {
                app.message = format!("{} file(s) failed to load", result.errors.len());
            }
        }
        app
    }

    fn refresh_from_store(&mut self, store: &Store) {
        self.books = store
            .library()
            .books()
            .iter()
            .map(|book| ShelfRow {
                id: book.id.clone(),
                title: book.title.clone(),
                author: book.author.clone(),
                format: book.format,
                progress_tenths: book.progress_tenths(),
                last_opened: book.last_opened,
                bookmark_count: book.bookmarks.len(),
                missing: book.missing,
                path: book.path.clone(),
            })
            .collect();
        self.recompute_visible();
    }

    fn recompute_visible(&mut self) {
        let needle = self.query.trim().to_lowercase();
        let mut indices: Vec<usize> = self
            .books
            .iter()
            .enumerate()
            .filter(|(_, book)| {
                if needle.is_empty() {
                    return true;
                }
                let title = book.title.to_lowercase();
                let author_match = book
                    .author
                    .as_deref()
                    .map(|author| author.to_lowercase().contains(&needle))
                    .unwrap_or(false);
                title.contains(&needle) || author_match
            })
            .map(|(index, _)| index)
            .collect();

        match self.sort {
            ShelfSort::Recent => indices.sort_by(|&a, &b| {
                let at = self.books[a].last_opened;
                let bt = self.books[b].last_opened;
                match (at, bt) {
                    (Some(at), Some(bt)) => bt.cmp(&at),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => self.books[a].title.cmp(&self.books[b].title),
                }
            }),
            ShelfSort::Title => {
                indices.sort_by(|&a, &b| self.books[a].title.cmp(&self.books[b].title));
            }
            ShelfSort::Progress => indices.sort_by(|&a, &b| {
                self.progress_percent(&self.books[b])
                    .partial_cmp(&self.progress_percent(&self.books[a]))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }

        self.visible = indices;
        if self.visible.is_empty() {
            self.selection = 0;
        } else if self.selection >= self.visible.len() {
            self.selection = self.visible.len() - 1;
        }
    }

    fn progress_percent(&self, row: &ShelfRow) -> f64 {
        f64::from(row.progress_tenths) / 10.0
    }

    fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            ShelfSort::Recent => ShelfSort::Title,
            ShelfSort::Title => ShelfSort::Progress,
            ShelfSort::Progress => ShelfSort::Recent,
        };
        self.recompute_visible();
        self.message = format!("Sort: {}", self.sort.label());
    }

    fn selected_row(&self) -> Option<&ShelfRow> {
        self.visible
            .get(self.selection)
            .map(|&index| &self.books[index])
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            self.selection = 0;
            return;
        }
        let len = self.visible.len() as isize;
        let current = self.selection as isize;
        let next = (current + delta).rem_euclid(len);
        self.selection = next as usize;
    }
}

impl ShelfSort {
    fn label(self) -> &'static str {
        match self {
            ShelfSort::Recent => "recent",
            ShelfSort::Title => "title",
            ShelfSort::Progress => "progress",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShelfCommandUi {
    Open,
    Next,
    Previous,
    First,
    Last,
    PageDown,
    PageUp,
    Search,
    Sort,
    Delete,
    Resume,
    Theme,
    Help,
    Quit,
}

const OPEN_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Enter)];
const NEXT_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('j')),
    KeyChord::plain(KeyCode::Down),
];
const PREVIOUS_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('k')),
    KeyChord::plain(KeyCode::Up),
];
const FIRST_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('g'))];
const LAST_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('G'))];
const PAGE_DOWN_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::PageDown)];
const PAGE_UP_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::PageUp)];
const SEARCH_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('/'))];
const SORT_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('s'))];
const DELETE_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('d'))];
const RESUME_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('r'))];
const THEME_KEYS: &[KeyChord] = &[KeyChord::plain(KeyCode::Char('c'))];
const HELP_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('?')),
    KeyChord::plain(KeyCode::F(1)),
];
const QUIT_KEYS: &[KeyChord] = &[
    KeyChord::plain(KeyCode::Char('q')),
    KeyChord::plain(KeyCode::Esc),
    KeyChord::control('c'),
];

const SHELF_BINDINGS: &[KeyBinding<ShelfCommandUi>] = &[
    KeyBinding {
        command: ShelfCommandUi::Open,
        chords: OPEN_KEYS,
        keys: "Enter",
        hud_keys: "Enter",
        label: "Open selected book",
        short_label: "Open",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ShelfCommandUi::Next,
        chords: NEXT_KEYS,
        keys: "j / Down",
        hud_keys: "Down",
        label: "Next book",
        short_label: "Next",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ShelfCommandUi::Previous,
        chords: PREVIOUS_KEYS,
        keys: "k / Up",
        hud_keys: "Up",
        label: "Previous book",
        short_label: "Prev",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ShelfCommandUi::Search,
        chords: SEARCH_KEYS,
        keys: "/",
        hud_keys: "/",
        label: "Filter title or author",
        short_label: "Search",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ShelfCommandUi::Sort,
        chords: SORT_KEYS,
        keys: "s",
        hud_keys: "s",
        label: "Cycle sort order",
        short_label: "Sort",
        show_in_hud: true,
        hud_priority: 1,
    },
    KeyBinding {
        command: ShelfCommandUi::Delete,
        chords: DELETE_KEYS,
        keys: "d",
        hud_keys: "d",
        label: "Remove selected book",
        short_label: "Delete",
        show_in_hud: true,
        hud_priority: 1,
    },
    KeyBinding {
        command: ShelfCommandUi::Resume,
        chords: RESUME_KEYS,
        keys: "r",
        hud_keys: "r",
        label: "Resume most recent book",
        short_label: "Resume",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ShelfCommandUi::Help,
        chords: HELP_KEYS,
        keys: "? / F1",
        hud_keys: "?",
        label: "Full help",
        short_label: "Help",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ShelfCommandUi::Quit,
        chords: QUIT_KEYS,
        keys: "q / Esc / Ctrl+C",
        hud_keys: "q",
        label: "Quit shelf",
        short_label: "Quit",
        show_in_hud: true,
        hud_priority: 0,
    },
    KeyBinding {
        command: ShelfCommandUi::First,
        chords: FIRST_KEYS,
        keys: "g",
        hud_keys: "g",
        label: "First book",
        short_label: "First",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ShelfCommandUi::Last,
        chords: LAST_KEYS,
        keys: "G",
        hud_keys: "G",
        label: "Last book",
        short_label: "Last",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ShelfCommandUi::PageDown,
        chords: PAGE_DOWN_KEYS,
        keys: "PageDown",
        hud_keys: "PgDn",
        label: "Move down ten books",
        short_label: "Down 10",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ShelfCommandUi::PageUp,
        chords: PAGE_UP_KEYS,
        keys: "PageUp",
        hud_keys: "PgUp",
        label: "Move up ten books",
        short_label: "Up 10",
        show_in_hud: false,
        hud_priority: 2,
    },
    KeyBinding {
        command: ShelfCommandUi::Theme,
        chords: THEME_KEYS,
        keys: "c",
        hud_keys: "c",
        label: "Switch dark/light theme",
        short_label: "Theme",
        show_in_hud: false,
        hud_priority: 2,
    },
];

fn handle_key(
    key: KeyEvent,
    app: &mut ShelfApp,
    store: &mut Store,
    config: &mut ReaderConfig,
) -> Result<Option<ShelfOutcome>> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(Some(ShelfOutcome::Exit));
    }

    match app.mode {
        ShelfMode::Browse => handle_browse_key(key, app, store, config),
        ShelfMode::Search => handle_search_key(key, app),
        ShelfMode::ConfirmDelete => handle_confirm_delete_key(key, app, store, config),
        ShelfMode::ConfirmDeleteFile => handle_confirm_delete_file_key(key, app),
        ShelfMode::Help => {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::F(1)
            ) {
                app.mode = ShelfMode::Browse;
            }
            Ok(None)
        }
    }
}

fn handle_browse_key(
    key: KeyEvent,
    app: &mut ShelfApp,
    store: &mut Store,
    config: &mut ReaderConfig,
) -> Result<Option<ShelfOutcome>> {
    match command_for(SHELF_BINDINGS, key) {
        Some(ShelfCommandUi::Quit) => return Ok(Some(ShelfOutcome::Exit)),
        Some(ShelfCommandUi::Next) => app.move_selection(1),
        Some(ShelfCommandUi::Previous) => app.move_selection(-1),
        Some(ShelfCommandUi::First) => app.selection = 0,
        Some(ShelfCommandUi::Last) if !app.visible.is_empty() => {
            app.selection = app.visible.len() - 1;
        }
        Some(ShelfCommandUi::PageDown) => app.move_selection(10),
        Some(ShelfCommandUi::PageUp) => app.move_selection(-10),
        Some(ShelfCommandUi::Search) => {
            app.mode = ShelfMode::Search;
            app.query.clear();
        }
        Some(ShelfCommandUi::Sort) => app.cycle_sort(),
        Some(ShelfCommandUi::Theme) => {
            config.toggle_theme();
            config.save()?;
            app.message = format!("Theme: {}", config.theme.label());
        }
        Some(ShelfCommandUi::Help) => app.mode = ShelfMode::Help,
        Some(ShelfCommandUi::Delete) => {
            if app.selected_row().is_some() {
                app.mode = ShelfMode::ConfirmDelete;
                if let Some(row) = app.selected_row() {
                    app.message = format!("Delete {:?}? (y/n)", row.title);
                }
            } else {
                app.message = "Nothing to delete".to_string();
            }
        }
        Some(ShelfCommandUi::Resume) => {
            let Some(id) = store.library().last_book_id.as_deref() else {
                app.message = "No recent book to resume".to_string();
                return Ok(None);
            };
            let Some(entry) = store.library().books().iter().find(|b| b.id == id).cloned() else {
                app.message = "Recent book not found in library".to_string();
                return Ok(None);
            };
            if entry.missing {
                app.message = "Recent book is missing. Restore it under bookshelf_dir and refresh."
                    .to_string();
                return Ok(None);
            }
            return Ok(Some(ShelfOutcome::Open(entry)));
        }
        Some(ShelfCommandUi::Open) => {
            let Some(row) = app.selected_row() else {
                app.message = "No book selected".to_string();
                return Ok(None);
            };
            let id = row.id.clone();
            if row.missing {
                app.message =
                    "Book file is missing. Restore it under bookshelf_dir and refresh.".to_string();
                return Ok(None);
            }
            let Some(entry) = store.library().books().iter().find(|b| b.id == id).cloned() else {
                bail!("Selected book disappeared from library");
            };
            return Ok(Some(ShelfOutcome::Open(entry)));
        }
        _ => {}
    }
    Ok(None)
}

fn handle_search_key(key: KeyEvent, app: &mut ShelfApp) -> Result<Option<ShelfOutcome>> {
    match key.code {
        KeyCode::Esc => {
            app.mode = ShelfMode::Browse;
            app.query.clear();
            app.recompute_visible();
            app.message.clear();
        }
        KeyCode::Enter => {
            app.mode = ShelfMode::Browse;
            app.recompute_visible();
            app.message = if app.query.is_empty() {
                String::new()
            } else {
                format!("Filter: {:?}", app.query)
            };
        }
        KeyCode::Backspace => {
            app.query.pop();
        }
        KeyCode::Char(ch) => app.query.push(ch),
        _ => {}
    }
    Ok(None)
}

fn handle_confirm_delete_key(
    key: KeyEvent,
    app: &mut ShelfApp,
    store: &mut Store,
    config: &ReaderConfig,
) -> Result<Option<ShelfOutcome>> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Some(row) = app.selected_row() {
                let id = row.id.clone();
                let title = row.title.clone();
                let path = row.path.clone();
                match store.remove_book(&id) {
                    Ok(removed) => {
                        crate::bookshelf::remove_sidecar_for_book(&removed)?;
                        store.save()?;
                        app.refresh_from_store(store);
                        if should_prompt_delete_file(&path, config) {
                            app.pending_delete_file = Some(path);
                            app.mode = ShelfMode::ConfirmDeleteFile;
                            app.message = format!("Also delete file for {title:?}? (y/n)");
                            return Ok(None);
                        }
                        app.message = format!("Removed: {title}");
                    }
                    Err(err) => {
                        app.mode = ShelfMode::Browse;
                        app.message = format!("Delete failed: {err}");
                        return Ok(None);
                    }
                }
            }
            app.mode = ShelfMode::Browse;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = ShelfMode::Browse;
            app.message = "Delete cancelled".to_string();
        }
        _ => {}
    }
    Ok(None)
}

fn handle_confirm_delete_file_key(
    key: KeyEvent,
    app: &mut ShelfApp,
) -> Result<Option<ShelfOutcome>> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Some(path) = app.pending_delete_file.take() {
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("Failed to delete {}", path.display()))?;
                }
                app.message = "Removed from library and deleted file".to_string();
            }
            app.mode = ShelfMode::Browse;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.pending_delete_file = None;
            app.mode = ShelfMode::Browse;
            app.message = "Removed from library; file kept".to_string();
        }
        _ => {}
    }
    Ok(None)
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &ShelfApp, store: &Store, config: &ReaderConfig) {
    let palette = Palette::for_theme(config.theme);
    fill_background(frame, palette);
    let [status_area, body_area, action_area] = hud_chunks(app.viewport);

    let status = match app.mode {
        ShelfMode::Search => Line::from(vec![
            Span::styled("Search: /", palette.accent()),
            Span::styled(app.query.clone(), palette.body()),
        ]),
        _ => Line::from(vec![
            Span::styled("My Library", palette.accent()),
            Span::styled(
                format!(
                    "  {} of {} books  ·  sort: {}",
                    app.visible.len(),
                    app.books.len(),
                    app.sort.label()
                ),
                palette.body(),
            ),
        ]),
    };
    frame.render_widget(Paragraph::new(status).style(palette.base()), status_area);

    let inner = Block::default().borders(Borders::NONE).inner(body_area);
    if app.books.is_empty() {
        frame.render_widget(
            Paragraph::new("No books in library. Add a book with `reader add <file>`.")
                .style(palette.muted())
                .wrap(Wrap { trim: false }),
            inner,
        );
    } else if app.visible.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("No matches for {:?}", app.query)).style(palette.muted()),
            inner,
        );
    } else {
        let last_book_id = store.library().last_book_id.as_deref();
        let max_items = inner.height as usize;
        let scroll = app.selection.saturating_sub(max_items.saturating_sub(1));
        let lines: Vec<Line> = app
            .visible
            .iter()
            .skip(scroll)
            .take(max_items)
            .enumerate()
            .map(|(offset, &index)| {
                let row = &app.books[index];
                let real_index = scroll + offset;
                build_row_line(row, real_index == app.selection, last_book_id, palette)
            })
            .collect();
        frame.render_widget(Paragraph::new(lines).style(palette.body()), inner);
    }

    let actions = match app.mode {
        ShelfMode::Browse => action_line(SHELF_BINDINGS, app.viewport.width, palette),
        ShelfMode::Search => shelf_prompt_actions(
            &[("Enter", "Apply"), ("Backspace", "Edit"), ("Esc", "Cancel")],
            palette,
        ),
        ShelfMode::ConfirmDelete | ShelfMode::ConfirmDeleteFile => {
            shelf_prompt_actions(&[("y/Enter", "Confirm"), ("n/Esc", "Cancel")], palette)
        }
        ShelfMode::Help => shelf_prompt_actions(&[("Esc/Enter", "Close")], palette),
    };
    frame.render_widget(Paragraph::new(actions).style(palette.base()), action_area);

    if !app.message.is_empty() && body_area.height > 0 {
        let width = (app.message.chars().count() as u16 + 4).min(body_area.width.max(1));
        let area = Rect::new(
            body_area.right().saturating_sub(width),
            body_area.bottom().saturating_sub(1),
            width,
            1,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(format!("  {}  ", app.message)).style(palette.accent()),
            area,
        );
    }

    if app.mode == ShelfMode::Help {
        draw_shelf_help(frame, palette);
    }
}

fn build_row_line(
    row: &ShelfRow,
    selected: bool,
    last_book_id: Option<&str>,
    palette: Palette,
) -> Line<'static> {
    let marker = if last_book_id == Some(row.id.as_str()) {
        "*"
    } else {
        " "
    };
    let prefix = if selected { "> " } else { "  " };
    let title = truncate(&row.title, 30);
    let author = row
        .author
        .as_deref()
        .map(|value| truncate(value, 16))
        .unwrap_or_else(|| "-".to_string());
    let percent = compute_percent(row);
    let bar = progress_bar(percent, PROGRESS_BAR_WIDTH);
    let last = row
        .last_opened
        .map(format_relative_time)
        .unwrap_or_else(|| "-".to_string());
    let bookmarks = if row.bookmark_count == 0 {
        "-".to_string()
    } else {
        format!("{}bm", row.bookmark_count)
    };
    let status = if row.missing { "MISS" } else { "OK" };
    let format = match row.format {
        BookFormat::Epub => "EPUB",
        BookFormat::Txt => "TXT",
    };
    let text = format!(
        "{prefix}{marker} {title:<30}  {author:<16}  {bar} {percent:>3}%  {last:>10}  {bookmarks:>4}  {format} {status}"
    );
    let style = if selected {
        palette.selected()
    } else if row.missing {
        palette.error()
    } else {
        palette.body()
    };
    Line::from(Span::styled(text, style))
}

fn compute_percent(row: &ShelfRow) -> u16 {
    (row.progress_tenths + 5) / 10
}

fn shelf_prompt_actions(actions: &[(&str, &str)], palette: Palette) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (keys, label)) in actions.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", palette.muted()));
        }
        spans.push(Span::styled(format!("[{keys}]"), palette.accent()));
        spans.push(Span::styled(format!(" {label}"), palette.muted()));
    }
    Line::from(spans)
}

fn draw_shelf_help(frame: &mut ratatui::Frame<'_>, palette: Palette) {
    let area = centered_rect(72, 82, frame.area());
    frame.render_widget(Clear, area);
    let lines = help_lines("Shelf", SHELF_BINDINGS, palette);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Shelf help ")
                    .borders(Borders::ALL)
                    .border_style(palette.border())
                    .style(palette.popup()),
            )
            .style(palette.popup())
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn progress_bar(percent: u16, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let filled = (percent as usize * width + 50) / 100;
    let filled = filled.min(width);
    let mut out = String::with_capacity(width * 3 + 1);
    for _ in 0..filled {
        out.push('\u{2588}');
    }
    for _ in filled..width {
        out.push('\u{2591}');
    }
    out
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else if max <= 1 {
        "\u{2026}".to_string()
    } else {
        let mut out: String = text.chars().take(max - 1).collect();
        out.push('\u{2026}');
        out
    }
}

fn should_prompt_delete_file(path: &Path, config: &ReaderConfig) -> bool {
    let Some(dir) = config.bookshelf_dir.as_deref() else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    if let (Ok(path), Ok(dir)) = (path.canonicalize(), dir.canonicalize()) {
        path.starts_with(dir)
    } else {
        path.starts_with(dir)
    }
}

pub fn format_relative_time(time: SystemTime) -> String {
    let Ok(duration) = SystemTime::now().duration_since(time) else {
        return "future".to_string();
    };
    let secs = duration.as_secs();
    if secs < 60 {
        return "just now".to_string();
    }
    let minutes = secs / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    let epoch_secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days_since_epoch = epoch_secs / 86_400;
    let (year, month, day) = epoch_days_to_ymd(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}")
}

fn epoch_days_to_ymd(mut days: i64) -> (i32, u32, u32) {
    // Howard Hinnant's days_from_civil algorithm.
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn row(id: &str, title: &str, last: Option<SystemTime>) -> ShelfRow {
        ShelfRow {
            id: id.into(),
            title: title.into(),
            author: None,
            format: BookFormat::Txt,
            progress_tenths: 0,
            last_opened: last,
            bookmark_count: 0,
            path: PathBuf::from(format!("/tmp/{id}.txt")),
            missing: false,
        }
    }

    #[test]
    fn recompute_visible_keeps_books_when_query_empty() {
        let mut app = ShelfApp {
            books: vec![row("a", "Alpha", None), row("b", "Beta", None)],
            visible: vec![],
            selection: 0,
            sort: ShelfSort::Recent,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.recompute_visible();
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn search_filter_matches_title_and_author() {
        let mut app = ShelfApp {
            books: vec![row("a", "Rust Programming", None), row("b", "Other", None)],
            visible: vec![],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Search,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.books[1].author = Some("Rustacean".into());
        app.query = "rust".into();
        app.recompute_visible();
        let titles: Vec<&str> = app
            .visible
            .iter()
            .map(|&i| app.books[i].title.as_str())
            .collect();
        assert!(titles.contains(&"Rust Programming"));
        assert!(titles.contains(&"Other"));
    }

    #[test]
    fn sort_recent_puts_most_recent_first_and_unopened_last() {
        let now = SystemTime::now();
        let mut app = ShelfApp {
            books: vec![
                row("a", "Alpha", None),
                row("b", "Beta", Some(now - Duration::from_secs(7200))),
                row("c", "Gamma", Some(now)),
            ],
            visible: vec![],
            selection: 0,
            sort: ShelfSort::Recent,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.recompute_visible();
        assert_eq!(app.books[app.visible[0]].id, "c");
        assert_eq!(app.books[app.visible[1]].id, "b");
        assert_eq!(app.books[app.visible[2]].id, "a");
    }

    #[test]
    fn sort_title_uses_alphabetical_order() {
        let mut app = ShelfApp {
            books: vec![
                row("a", "Charlie", None),
                row("b", "Alpha", None),
                row("c", "Bravo", None),
            ],
            visible: vec![],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.recompute_visible();
        let titles: Vec<&str> = app
            .visible
            .iter()
            .map(|&i| app.books[i].title.as_str())
            .collect();
        assert_eq!(titles, vec!["Alpha", "Bravo", "Charlie"]);
    }

    #[test]
    fn sort_progress_orders_high_to_low() {
        let mut app = ShelfApp {
            books: vec![
                row("a", "Half", None),
                row("b", "Quarter", None),
                row("c", "Full", None),
            ],
            visible: vec![],
            selection: 0,
            sort: ShelfSort::Progress,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.books[0].progress_tenths = 500;
        app.books[1].progress_tenths = 250;
        app.books[2].progress_tenths = 1000;
        app.recompute_visible();
        let titles: Vec<&str> = app
            .visible
            .iter()
            .map(|&i| app.books[i].title.as_str())
            .collect();
        assert_eq!(titles, vec!["Full", "Half", "Quarter"]);
    }

    #[test]
    fn cycle_sort_walks_through_modes() {
        let mut app = ShelfApp {
            books: vec![row("a", "Alpha", None)],
            visible: vec![],
            selection: 0,
            sort: ShelfSort::Recent,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.cycle_sort();
        assert_eq!(app.sort, ShelfSort::Title);
        app.cycle_sort();
        assert_eq!(app.sort, ShelfSort::Progress);
        app.cycle_sort();
        assert_eq!(app.sort, ShelfSort::Recent);
    }

    #[test]
    fn move_selection_wraps_around() {
        let mut app = ShelfApp {
            books: vec![
                row("a", "A", None),
                row("b", "B", None),
                row("c", "C", None),
            ],
            visible: vec![0, 1, 2],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        app.move_selection(-1);
        assert_eq!(app.selection, 2);
        app.move_selection(1);
        assert_eq!(app.selection, 0);
    }

    #[test]
    fn enter_returns_open_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.txt");
        std::fs::write(&path, "hello").unwrap();
        let document = crate::book::BookDocument {
            id: "id".into(),
            title: "T".into(),
            path: path.clone(),
            format: crate::book::BookFormat::Txt,
            metadata: crate::book::BookMetadata {
                title: "T".into(),
                author: None,
                language: None,
                publisher: None,
                format: crate::book::BookFormat::Txt,
                path: path.clone(),
                section_count: 1,
            },
            sections: vec![crate::book::Section {
                title: "S".into(),
                text: "hello".into(),
            }],
        };
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        store.add_book(&document, &document.path).unwrap();
        let mut app = ShelfApp {
            books: vec![row("id", "T", None)],
            visible: vec![0],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        let outcome = handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert!(matches!(outcome, Some(ShelfOutcome::Open(_))));
    }

    #[test]
    fn r_with_no_recent_sets_message_and_does_not_exit() {
        let mut store = make_empty_store();
        let mut app = ShelfApp {
            books: vec![row("a", "A", None)],
            visible: vec![0],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        let outcome = handle_key(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert!(outcome.is_none());
        assert!(app.message.contains("No recent"));
    }

    #[test]
    fn r_with_recent_returns_open_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "x").unwrap();
        let document = crate::book::BookDocument {
            id: "a".into(),
            title: "A".into(),
            path: path.clone(),
            format: crate::book::BookFormat::Txt,
            metadata: crate::book::BookMetadata {
                title: "A".into(),
                author: None,
                language: None,
                publisher: None,
                format: crate::book::BookFormat::Txt,
                path: path.clone(),
                section_count: 1,
            },
            sections: vec![crate::book::Section {
                title: "S".into(),
                text: "x".into(),
            }],
        };
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        store.add_book(&document, &document.path).unwrap();
        store.record_open("a");
        let mut app = ShelfApp {
            books: vec![row("a", "A", None)],
            visible: vec![0],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        let outcome = handle_key(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert!(matches!(outcome, Some(ShelfOutcome::Open(_))));
    }

    #[test]
    fn q_exits_from_browse_mode() {
        let mut store = make_empty_store();
        let mut app = ShelfApp {
            books: vec![row("a", "A", None)],
            visible: vec![0],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::Browse,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        let outcome = handle_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert!(matches!(outcome, Some(ShelfOutcome::Exit)));
    }

    #[test]
    fn search_enter_applies_filter() {
        let mut store = make_empty_store();
        let mut app = ShelfApp {
            books: vec![row("a", "Alpha", None), row("b", "Beta", None)],
            visible: vec![0, 1],
            selection: 0,
            sort: ShelfSort::Title,
            query: "alp".into(),
            mode: ShelfMode::Search,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert_eq!(app.mode, ShelfMode::Browse);
        assert_eq!(app.visible, vec![0]);
    }

    fn make_empty_store() -> Store {
        let dir = tempfile::tempdir().unwrap();
        Store::open(dir.path().join("library.json")).unwrap()
    }

    #[test]
    fn search_esc_cancels_and_restores_full_list() {
        let mut store = make_empty_store();
        let mut app = ShelfApp {
            books: vec![row("a", "Alpha", None), row("b", "Beta", None)],
            visible: vec![0, 1],
            selection: 0,
            sort: ShelfSort::Title,
            query: "alp".into(),
            mode: ShelfMode::Search,
            viewport: Rect::new(0, 0, 80, 24),
            message: String::new(),
            pending_delete_file: None,
        };
        handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert_eq!(app.mode, ShelfMode::Browse);
        assert_eq!(app.visible.len(), 2);
        assert!(app.query.is_empty());
    }

    #[test]
    fn delete_confirm_y_removes_book_from_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.txt");
        std::fs::write(&path, "x").unwrap();
        let document = crate::book::BookDocument {
            id: "abc".into(),
            title: "Book".into(),
            path: path.clone(),
            format: crate::book::BookFormat::Txt,
            metadata: crate::book::BookMetadata {
                title: "Book".into(),
                author: None,
                language: None,
                publisher: None,
                format: crate::book::BookFormat::Txt,
                path: path.clone(),
                section_count: 1,
            },
            sections: vec![crate::book::Section {
                title: "S".into(),
                text: "x".into(),
            }],
        };
        let mut store = Store::open(dir.path().join("library.json")).unwrap();
        store.add_book(&document, &document.path).unwrap();
        let mut app = ShelfApp {
            books: vec![row("abc", "Book", None)],
            visible: vec![0],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::ConfirmDelete,
            viewport: Rect::new(0, 0, 80, 24),
            message: "Delete \"Book\"? (y/n)".into(),
            pending_delete_file: None,
        };
        let outcome = handle_key(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert!(outcome.is_none());
        assert_eq!(app.mode, ShelfMode::Browse);
        assert!(app.message.contains("Removed: Book"));
        assert!(app.books.is_empty());
        assert_eq!(store.library().books().len(), 0);
    }

    #[test]
    fn delete_confirm_n_cancels() {
        let mut store = make_empty_store();
        let mut app = ShelfApp {
            books: vec![row("abc", "Book", None)],
            visible: vec![0],
            selection: 0,
            sort: ShelfSort::Title,
            query: String::new(),
            mode: ShelfMode::ConfirmDelete,
            viewport: Rect::new(0, 0, 80, 24),
            message: "Delete \"Book\"? (y/n)".into(),
            pending_delete_file: None,
        };
        let outcome = handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            &mut app,
            &mut store,
            &mut ReaderConfig::default(),
        )
        .unwrap();
        assert!(outcome.is_none());
        assert_eq!(app.mode, ShelfMode::Browse);
        assert!(app.message.contains("cancelled"));
    }

    #[test]
    fn progress_bar_handles_full_and_zero() {
        assert_eq!(
            progress_bar(0, 5),
            "\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}"
        );
        assert_eq!(
            progress_bar(100, 5),
            "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}"
        );
        assert_eq!(
            progress_bar(40, 5),
            "\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}"
        );
    }

    #[test]
    fn format_relative_time_caps_at_date() {
        let now = SystemTime::now();
        assert_eq!(
            format_relative_time(now - Duration::from_secs(5)),
            "just now"
        );
        assert_eq!(format_relative_time(now - Duration::from_secs(120)), "2m");
        assert_eq!(format_relative_time(now - Duration::from_secs(7200)), "2h");
        assert_eq!(
            format_relative_time(now - Duration::from_secs(86_400 * 3)),
            "3d"
        );
    }

    #[test]
    fn shelf_themes_and_help_render_with_shared_palette() {
        for theme in [crate::ui::Theme::Dark, crate::ui::Theme::Light] {
            let store = make_empty_store();
            let mut app = ShelfApp {
                books: vec![row("a", "Alpha", None)],
                visible: vec![0],
                selection: 0,
                sort: ShelfSort::Title,
                query: String::new(),
                mode: ShelfMode::Help,
                viewport: Rect::new(0, 0, 80, 20),
                message: String::new(),
                pending_delete_file: None,
            };
            app.books[0].progress_tenths = 500;
            let mut config = ReaderConfig::default();
            config.theme = theme;
            let backend = TestBackend::new(80, 20);
            let mut terminal = Terminal::new(backend).unwrap();

            terminal
                .draw(|frame| draw(frame, &app, &store, &config))
                .unwrap();

            let palette = Palette::for_theme(theme);
            let cell = terminal.backend().buffer().cell((0, 0)).unwrap();
            assert_eq!(cell.bg, palette.background);
            let screen = format!("{}", terminal.backend());
            assert!(screen.contains("Shelf help"));
            assert!(screen.contains("Open selected book"));
        }
    }
}
