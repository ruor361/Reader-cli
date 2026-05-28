use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::book::BookDocument;
use crate::config::{ReaderConfig, Theme};
use crate::pagination::{first_page_for_section, paginate_with_text_width, Page};
use crate::storage::{Progress, Store};

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

#[derive(Debug)]
struct ReaderApp {
    document: BookDocument,
    book_id: String,
    pages: Vec<Page>,
    current_page: usize,
    mode: ReaderMode,
    toc_selection: usize,
    message: String,
    search_query: String,
    search_results: Vec<usize>,
    search_index: usize,
    config: ReaderConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReaderMode {
    Reading,
    Toc,
    Metadata,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyOutcome {
    Continue,
    Exit,
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
    let pages = paginate_with_text_width(
        &document.sections,
        size.width,
        size.height,
        config.text_width,
    );
    let saved = store.progress_for(&book_id).copied().unwrap_or_default();
    let mut app = ReaderApp {
        document,
        book_id,
        current_page: saved.page.min(pages.len().saturating_sub(1)),
        pages,
        mode: ReaderMode::Reading,
        toc_selection: saved.section_index,
        message: reading_help().to_string(),
        search_query: String::new(),
        search_results: Vec::new(),
        search_index: 0,
        config: config.clone(),
    };

    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if !should_handle_key_event(key) {
            continue;
        }
        if handle_key(key, &mut app, store)? {
            break;
        }
    }

    persist_progress(&app, store)?;
    store.save()?;
    *config = app.config.clone();
    config.save()?;
    Ok(())
}

fn handle_key(key: KeyEvent, app: &mut ReaderApp, store: &mut Store) -> Result<bool> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    let outcome = match app.mode {
        ReaderMode::Reading => handle_reading_key(key, app, store)?,
        ReaderMode::Search => handle_search_key(key, app),
        ReaderMode::Toc => handle_toc_key(key, app),
        ReaderMode::Metadata => handle_metadata_key(key, app),
    };

    Ok(outcome == KeyOutcome::Exit)
}

fn should_handle_key_event(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

fn handle_reading_key(key: KeyEvent, app: &mut ReaderApp, store: &mut Store) -> Result<KeyOutcome> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Ok(KeyOutcome::Exit),
        KeyCode::Down | KeyCode::PageDown | KeyCode::Char('j') | KeyCode::Char(' ') => {
            app.current_page = (app.current_page + 1).min(app.pages.len().saturating_sub(1));
        }
        KeyCode::Up | KeyCode::PageUp | KeyCode::Char('k') => {
            app.current_page = app.current_page.saturating_sub(1);
        }
        KeyCode::Char('g') => app.current_page = 0,
        KeyCode::Char('G') => app.current_page = app.pages.len().saturating_sub(1),
        KeyCode::Char('t') => {
            app.toc_selection = app.pages[app.current_page].section_index;
            app.mode = ReaderMode::Toc;
            app.message = "Enter jump | Esc back | q back".to_string();
        }
        KeyCode::Char('M') => {
            app.mode = ReaderMode::Metadata;
            app.message = "Esc/Enter/q close".to_string();
        }
        KeyCode::Char('c') => {
            app.config.toggle_theme();
            app.config.save()?;
            app.message = format!("Theme: {}", theme_label(app.config.theme));
        }
        KeyCode::Char('s') => {
            app.config.toggle_progress();
            app.config.save()?;
            app.message = if app.config.show_progress {
                "Progress shown".to_string()
            } else {
                "Progress hidden".to_string()
            };
        }
        KeyCode::Char('b') => {
            persist_progress(app, store)?;
            let bookmark = store.add_bookmark(
                &app.book_id,
                format!("Page {}", app.current_page + 1),
                current_progress(app),
            )?;
            app.message = format!("Bookmark added: {}", bookmark.id);
        }
        KeyCode::Char('/') => {
            app.mode = ReaderMode::Search;
            app.search_query.clear();
            app.message = "Enter search | Esc cancel".to_string();
        }
        KeyCode::Char('n') => jump_search(app, 1),
        KeyCode::Char('N') => jump_search(app, -1),
        _ => {}
    }

    persist_progress(app, store)?;
    Ok(KeyOutcome::Continue)
}

fn handle_search_key(key: KeyEvent, app: &mut ReaderApp) -> KeyOutcome {
    match key.code {
        KeyCode::Esc => {
            return_to_reading(app, "Search cancelled");
        }
        KeyCode::Enter => {
            app.mode = ReaderMode::Reading;
            apply_search(app);
        }
        KeyCode::Backspace => {
            app.search_query.pop();
        }
        KeyCode::Char(ch) => app.search_query.push(ch),
        _ => {}
    }
    KeyOutcome::Continue
}

fn handle_toc_key(key: KeyEvent, app: &mut ReaderApp) -> KeyOutcome {
    match app.mode {
        ReaderMode::Toc => {}
        _ => return KeyOutcome::Continue,
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') => {
            return_to_reading(app, reading_help());
        }
        KeyCode::Down | KeyCode::PageDown | KeyCode::Char('j') => {
            app.toc_selection =
                (app.toc_selection + 1).min(app.document.sections.len().saturating_sub(1));
        }
        KeyCode::Up | KeyCode::PageUp | KeyCode::Char('k') => {
            app.toc_selection = app.toc_selection.saturating_sub(1);
        }
        KeyCode::Home | KeyCode::Char('g') => app.toc_selection = 0,
        KeyCode::End | KeyCode::Char('G') => {
            app.toc_selection = app.document.sections.len().saturating_sub(1);
        }
        KeyCode::Enter => {
            if let Some(page) = first_page_for_section(&app.pages, app.toc_selection) {
                app.current_page = page;
                let title = app.document.sections[app.toc_selection].title.clone();
                return_to_reading(app, format!("Chapter: {title}"));
            }
        }
        _ => {}
    }
    KeyOutcome::Continue
}

fn handle_metadata_key(key: KeyEvent, app: &mut ReaderApp) -> KeyOutcome {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('M') => {
            return_to_reading(app, reading_help());
        }
        _ => {}
    }
    KeyOutcome::Continue
}

fn return_to_reading(app: &mut ReaderApp, message: impl Into<String>) {
    app.mode = ReaderMode::Reading;
    app.message = message.into();
}

fn reading_help() -> &'static str {
    "q quit | t toc | M metadata | c theme | s progress | / search"
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &ReaderApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let page = &app.pages[app.current_page];
    let percent = if app.pages.len() <= 1 {
        100
    } else {
        ((app.current_page + 1) * 100) / app.pages.len()
    };
    let mut title_spans = vec![Span::styled(
        &app.document.title,
        Style::default().add_modifier(Modifier::BOLD),
    )];
    if app.config.show_progress {
        title_spans.push(Span::raw(format!(
            "  page {}/{}  {percent}%",
            app.current_page + 1,
            app.pages.len()
        )));
    }
    let title = Line::from(title_spans);
    frame.render_widget(Paragraph::new(title), chunks[0]);

    let body = page.lines.join("\n");
    let paragraph = Paragraph::new(body)
        .block(Block::default().borders(Borders::NONE))
        .style(body_style(app.config.theme))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, chunks[1]);

    let status = if app.mode == ReaderMode::Search {
        Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(&app.search_query),
            Span::raw("  Enter search | Esc cancel"),
        ])
    } else {
        Line::from(app.message.as_str())
    };
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );

    match app.mode {
        ReaderMode::Toc => draw_toc(frame, app),
        ReaderMode::Metadata => draw_metadata(frame, app),
        ReaderMode::Reading | ReaderMode::Search => {}
    }
}

fn current_progress(app: &ReaderApp) -> Progress {
    let page = &app.pages[app.current_page];
    Progress {
        page: app.current_page,
        section_index: page.section_index,
        start_line: page.start_line,
    }
}

fn persist_progress(app: &ReaderApp, store: &mut Store) -> Result<()> {
    store.set_progress(&app.book_id, current_progress(app));
    Ok(())
}

fn apply_search(app: &mut ReaderApp) {
    let query = app.search_query.trim().to_lowercase();
    if query.is_empty() {
        app.message = "Empty search".to_string();
        return;
    }

    app.search_results = app
        .pages
        .iter()
        .enumerate()
        .filter_map(|(index, page)| {
            let haystack = page.lines.join("\n").to_lowercase();
            haystack.contains(&query).then_some(index)
        })
        .collect();
    app.search_index = 0;

    if let Some(page) = app.search_results.first().copied() {
        app.current_page = page;
        app.message = format!(
            "{} matches for {:?}",
            app.search_results.len(),
            app.search_query
        );
    } else {
        app.message = format!("No matches for {:?}", app.search_query);
    }
}

fn jump_search(app: &mut ReaderApp, delta: isize) {
    if app.search_results.is_empty() {
        app.message = "No active search results".to_string();
        return;
    }
    let len = app.search_results.len() as isize;
    app.search_index = (app.search_index as isize + delta).rem_euclid(len) as usize;
    app.current_page = app.search_results[app.search_index];
    app.message = format!(
        "Match {}/{}",
        app.search_index + 1,
        app.search_results.len()
    );
}

fn draw_toc(frame: &mut ratatui::Frame<'_>, app: &ReaderApp) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);
    let max_items = area.height.saturating_sub(2) as usize;
    let half = max_items / 2;
    let start = app.toc_selection.saturating_sub(half);
    let end = (start + max_items).min(app.document.sections.len());
    let lines = app.document.sections[start..end]
        .iter()
        .enumerate()
        .map(|(offset, section)| {
            let index = start + offset;
            let prefix = if index == app.toc_selection {
                "> "
            } else {
                "  "
            };
            Line::from(format!("{prefix}{}  {}", index + 1, section.title))
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Table of Contents")
                .borders(Borders::ALL),
        )
        .style(popup_style(app.config.theme));
    frame.render_widget(paragraph, area);
}

fn draw_metadata(frame: &mut ratatui::Frame<'_>, app: &ReaderApp) {
    let area = centered_rect(68, 58, frame.area());
    frame.render_widget(Clear, area);
    let metadata = &app.document.metadata;
    let mut lines = vec![
        Line::from(format!("Title: {}", metadata.title)),
        Line::from(format!("Format: {}", metadata.format.label())),
        Line::from(format!("Path: {}", metadata.path.display())),
        Line::from(format!("Sections: {}", metadata.section_count)),
    ];
    if let Some(author) = metadata.author.as_deref() {
        lines.push(Line::from(format!("Author: {author}")));
    }
    if let Some(language) = metadata.language.as_deref() {
        lines.push(Line::from(format!("Language: {language}")));
    }
    if let Some(publisher) = metadata.publisher.as_deref() {
        lines.push(Line::from(format!("Publisher: {publisher}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Esc/Enter/q close"));

    let paragraph = Paragraph::new(lines)
        .block(Block::default().title("Metadata").borders(Borders::ALL))
        .style(popup_style(app.config.theme))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn body_style(theme: Theme) -> Style {
    match theme {
        Theme::Dark => Style::default().fg(Color::Gray).bg(Color::Black),
        Theme::Light => Style::default().fg(Color::Black).bg(Color::White),
    }
}

fn popup_style(theme: Theme) -> Style {
    match theme {
        Theme::Dark => Style::default().fg(Color::White).bg(Color::Black),
        Theme::Light => Style::default().fg(Color::Black).bg(Color::White),
    }
}

fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookFormat, BookMetadata, Section};

    fn make_app() -> ReaderApp {
        let path = std::path::PathBuf::from("book.txt");
        let sections = vec![
            Section {
                title: "One".into(),
                text: "first".into(),
            },
            Section {
                title: "Two".into(),
                text: "second".into(),
            },
        ];
        let document = BookDocument {
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
                section_count: sections.len(),
            },
            sections,
        };
        let pages = paginate_with_text_width(&document.sections, 80, 24, None);
        ReaderApp {
            document,
            book_id: "book".into(),
            pages,
            current_page: 0,
            mode: ReaderMode::Reading,
            toc_selection: 0,
            message: String::new(),
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: 0,
            config: ReaderConfig::default(),
        }
    }

    fn make_store() -> Store {
        Store::open(tempfile::tempdir().unwrap().path().join("library.json")).unwrap()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    fn release(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Release)
    }

    #[test]
    fn toc_enter_jumps_to_selected_section() {
        let mut app = make_app();
        app.mode = ReaderMode::Toc;
        app.toc_selection = 1;

        handle_toc_key(key(KeyCode::Enter), &mut app);

        assert_eq!(app.mode, ReaderMode::Reading);
        assert_eq!(app.current_page, 1);
    }

    #[test]
    fn key_release_events_are_ignored_by_event_loop_filter() {
        assert!(should_handle_key_event(key(KeyCode::Char('t'))));
        assert!(!should_handle_key_event(release(KeyCode::Char('t'))));
    }

    #[test]
    fn metadata_escape_returns_to_reading_without_progress_change() {
        let mut app = make_app();
        app.current_page = 1;
        app.mode = ReaderMode::Metadata;

        let mut store = make_store();
        let done = handle_key(key(KeyCode::Esc), &mut app, &mut store).unwrap();

        assert!(!done);
        assert_eq!(app.mode, ReaderMode::Reading);
        assert_eq!(app.current_page, 1);
    }

    #[test]
    fn esc_returns_from_child_modes_but_exits_reading() {
        for mode in [ReaderMode::Toc, ReaderMode::Metadata, ReaderMode::Search] {
            let mut app = make_app();
            app.current_page = 1;
            app.mode = mode;
            let mut store = make_store();

            let done = handle_key(key(KeyCode::Esc), &mut app, &mut store).unwrap();

            assert!(!done);
            assert_eq!(app.mode, ReaderMode::Reading);
            assert_eq!(app.current_page, 1);
        }

        let mut app = make_app();
        let mut store = make_store();
        assert!(handle_key(key(KeyCode::Esc), &mut app, &mut store).unwrap());
    }

    #[test]
    fn ctrl_c_exits_from_every_mode() {
        for mode in [
            ReaderMode::Reading,
            ReaderMode::Toc,
            ReaderMode::Metadata,
            ReaderMode::Search,
        ] {
            let mut app = make_app();
            app.mode = mode;
            let mut store = make_store();

            assert!(handle_key(ctrl_c(), &mut app, &mut store).unwrap());
        }
    }

    #[test]
    fn q_exits_reading_returns_from_panels_and_types_in_search() {
        let mut app = make_app();
        let mut store = make_store();
        assert!(handle_key(key(KeyCode::Char('q')), &mut app, &mut store).unwrap());

        for mode in [ReaderMode::Toc, ReaderMode::Metadata] {
            let mut app = make_app();
            app.mode = mode;
            let mut store = make_store();

            let done = handle_key(key(KeyCode::Char('q')), &mut app, &mut store).unwrap();

            assert!(!done);
            assert_eq!(app.mode, ReaderMode::Reading);
        }

        let mut app = make_app();
        app.mode = ReaderMode::Search;
        let mut store = make_store();
        let done = handle_key(key(KeyCode::Char('q')), &mut app, &mut store).unwrap();

        assert!(!done);
        assert_eq!(app.mode, ReaderMode::Search);
        assert_eq!(app.search_query, "q");
    }

    #[test]
    fn enter_confirms_child_modes() {
        let mut app = make_app();
        app.mode = ReaderMode::Search;
        app.search_query = "second".to_string();
        let mut store = make_store();
        let done = handle_key(key(KeyCode::Enter), &mut app, &mut store).unwrap();

        assert!(!done);
        assert_eq!(app.mode, ReaderMode::Reading);
        assert_eq!(app.current_page, 1);

        let mut app = make_app();
        app.mode = ReaderMode::Metadata;
        app.current_page = 1;
        let mut store = make_store();
        let done = handle_key(key(KeyCode::Enter), &mut app, &mut store).unwrap();

        assert!(!done);
        assert_eq!(app.mode, ReaderMode::Reading);
        assert_eq!(app.current_page, 1);
    }

    #[test]
    fn cancelling_child_modes_does_not_change_current_page() {
        for mode in [ReaderMode::Toc, ReaderMode::Metadata, ReaderMode::Search] {
            let mut app = make_app();
            app.mode = mode;
            app.current_page = 1;
            app.toc_selection = 0;
            let mut store = make_store();

            handle_key(key(KeyCode::Esc), &mut app, &mut store).unwrap();

            assert_eq!(app.mode, ReaderMode::Reading);
            assert_eq!(app.current_page, 1);
        }
    }

    #[test]
    fn toggles_progress_setting_in_memory() {
        let mut config = ReaderConfig::default();

        config.toggle_progress();

        assert!(!config.show_progress);
    }
}
