use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::book::BookDocument;
use crate::pagination::{paginate, Page};
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
    message: String,
    search_mode: bool,
    search_query: String,
    search_results: Vec<usize>,
    search_index: usize,
}

pub fn run_reader(document: BookDocument, book_id: String, store: &mut Store) -> Result<()> {
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    let size = terminal.size()?;
    let pages = paginate(&document.sections, size.width, size.height);
    let saved = store.progress_for(&book_id).copied().unwrap_or_default();
    let mut app = ReaderApp {
        document,
        book_id,
        current_page: saved.page.min(pages.len().saturating_sub(1)),
        pages,
        message: "q quit | arrows/page keys move | b bookmark | / search".to_string(),
        search_mode: false,
        search_query: String::new(),
        search_results: Vec::new(),
        search_index: 0,
    };

    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if handle_key(key, &mut app, store)? {
            break;
        }
    }

    persist_progress(&app, store)?;
    store.save()?;
    Ok(())
}

fn handle_key(key: KeyEvent, app: &mut ReaderApp, store: &mut Store) -> Result<bool> {
    if app.search_mode {
        match key.code {
            KeyCode::Esc => {
                app.search_mode = false;
                app.message = "Search cancelled".to_string();
            }
            KeyCode::Enter => {
                app.search_mode = false;
                apply_search(app);
            }
            KeyCode::Backspace => {
                app.search_query.pop();
            }
            KeyCode::Char(ch) => app.search_query.push(ch),
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Down | KeyCode::PageDown | KeyCode::Char('j') | KeyCode::Char(' ') => {
            app.current_page = (app.current_page + 1).min(app.pages.len().saturating_sub(1));
        }
        KeyCode::Up | KeyCode::PageUp | KeyCode::Char('k') => {
            app.current_page = app.current_page.saturating_sub(1);
        }
        KeyCode::Char('g') => app.current_page = 0,
        KeyCode::Char('G') => app.current_page = app.pages.len().saturating_sub(1),
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
            app.search_mode = true;
            app.search_query.clear();
            app.message = "Type search query, Enter to jump, Esc to cancel".to_string();
        }
        KeyCode::Char('n') => jump_search(app, 1),
        KeyCode::Char('N') => jump_search(app, -1),
        _ => {}
    }

    persist_progress(app, store)?;
    Ok(false)
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
    let title = Line::from(vec![
        Span::styled(&app.document.title, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("  page {}/{}  {percent}%", app.current_page + 1, app.pages.len())),
    ]);
    frame.render_widget(Paragraph::new(title), chunks[0]);

    let body = page.lines.join("\n");
    let paragraph = Paragraph::new(body)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, chunks[1]);

    let status = if app.search_mode {
        Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(&app.search_query),
        ])
    } else {
        Line::from(app.message.as_str())
    };
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
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
        app.message = format!("{} matches for {:?}", app.search_results.len(), app.search_query);
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
    app.message = format!("Match {}/{}", app.search_index + 1, app.search_results.len());
}
