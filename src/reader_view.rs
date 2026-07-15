use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::privacy::{privacy_stack, PRIVACY_ERROR_LINE};
use crate::reader_input::READING_BINDINGS;
use crate::reader_state::{
    format_tenths, privacy_chunks, ChapterNavigator, NotificationKind, Overlay, ReaderState,
};
use crate::ui::{action_line, centered_rect, fill_background, help_lines, hud_chunks, Palette};

pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, state: &ReaderState) {
    let palette = Palette::for_theme(state.config.theme);
    fill_background(frame, palette);
    if state.config.privacy_mode {
        draw_privacy(frame, state, palette);
        return;
    }

    let [status_area, body_area, action_area] = hud_chunks(frame.area());
    frame.render_widget(
        Paragraph::new(status_line(state, palette)).style(palette.base()),
        status_area,
    );

    let page = &state.pages[state.current_page];
    let text_area = reading_text_area(body_area, state.config.text_width);
    frame.render_widget(
        Paragraph::new(page.lines.join("\n"))
            .style(palette.body())
            .wrap(Wrap { trim: false }),
        text_area,
    );
    frame.render_widget(
        Paragraph::new(context_action_line(state, palette)).style(palette.base()),
        action_area,
    );

    if let Some(notification) = state.notification.as_ref() {
        draw_notification(
            frame,
            body_area,
            notification.text.as_str(),
            notification.kind,
            palette,
        );
    }

    match &state.overlay {
        Overlay::None => {}
        Overlay::Percent(navigator) => draw_percent(frame, navigator, palette),
        Overlay::Chapters(navigator) => draw_chapters(frame, state, navigator, palette),
        Overlay::Search(input) => draw_search(frame, input, palette),
        Overlay::Metadata => draw_metadata(frame, state, palette),
        Overlay::Help => draw_help(frame, palette),
    }
}

fn status_line(state: &ReaderState, palette: Palette) -> Line<'static> {
    let section_index = state.current_section();
    let section_count = state.document.sections.len();
    let section_title = state
        .document
        .sections
        .get(section_index)
        .map(|section| section.title.as_str())
        .unwrap_or("Untitled");

    if state.viewport.width < 62 {
        let mut spans = vec![
            Span::styled(truncate_width(&state.document.title, 22), palette.accent()),
            Span::styled(
                format!("  Ch {}/{}", section_index + 1, section_count.max(1)),
                palette.body(),
            ),
        ];
        if state.config.show_progress {
            spans.push(Span::styled(
                format!("  {}", format_tenths(state.progress_tenths())),
                palette.accent(),
            ));
        }
        return Line::from(spans);
    }

    let mut spans = vec![
        Span::styled(truncate_width(&state.document.title, 30), palette.accent()),
        Span::styled("  ·  ", palette.muted()),
        Span::styled(
            format!(
                "Chapter {}/{}: {}",
                section_index + 1,
                section_count.max(1),
                truncate_width(section_title, 36)
            ),
            palette.body(),
        ),
    ];
    if state.config.show_progress {
        spans.extend([
            Span::styled("  ·  ", palette.muted()),
            Span::styled(format_tenths(state.progress_tenths()), palette.accent()),
        ]);
    }
    Line::from(spans)
}

fn context_action_line(state: &ReaderState, palette: Palette) -> Line<'static> {
    match &state.overlay {
        Overlay::None => action_line(READING_BINDINGS, state.viewport.width, palette),
        Overlay::Percent(_) => prompt_actions(
            &[("Enter", "Jump"), ("Backspace", "Edit"), ("Esc", "Cancel")],
            palette,
        ),
        Overlay::Chapters(navigator) if navigator.filter_active => prompt_actions(
            &[
                ("Type", "Filter"),
                ("Up/Down", "Select"),
                ("Enter", "Jump"),
                ("Esc", "Clear"),
            ],
            palette,
        ),
        Overlay::Chapters(_) => prompt_actions(
            &[
                ("j/k", "Select"),
                ("0-9", "Number"),
                ("/", "Filter"),
                ("Enter", "Jump"),
                ("Esc", "Cancel"),
            ],
            palette,
        ),
        Overlay::Search(_) => prompt_actions(
            &[
                ("Enter", "Search"),
                ("Backspace", "Edit"),
                ("Esc", "Cancel"),
            ],
            palette,
        ),
        Overlay::Metadata | Overlay::Help => prompt_actions(&[("Esc/Enter", "Close")], palette),
    }
}

fn prompt_actions(actions: &[(&str, &str)], palette: Palette) -> Line<'static> {
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

fn reading_text_area(area: Rect, text_width: Option<u16>) -> Rect {
    let available = area.width.saturating_sub(2).max(1);
    let width = text_width
        .map(|width| width.max(20).min(available))
        .unwrap_or(available);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y,
        width,
        area.height,
    )
}

fn draw_notification(
    frame: &mut ratatui::Frame<'_>,
    body_area: Rect,
    text: &str,
    kind: NotificationKind,
    palette: Palette,
) {
    if body_area.height == 0 {
        return;
    }
    let width = (text.width() as u16 + 4).min(body_area.width.max(1));
    let area = Rect::new(
        body_area.right().saturating_sub(width),
        body_area.bottom().saturating_sub(1),
        width,
        1,
    );
    frame.render_widget(Clear, area);
    let style = match kind {
        NotificationKind::Info => palette.accent(),
        NotificationKind::Success => palette.success(),
        NotificationKind::Error => palette.error(),
    };
    frame.render_widget(
        Paragraph::new(format!("  {text}  ")).style(style.bg(palette.background)),
        area,
    );
}

fn popup_block<'a>(title: &'a str, palette: Palette) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(palette.border())
        .style(palette.popup())
        .padding(Padding::horizontal(1))
}

fn draw_percent(
    frame: &mut ratatui::Frame<'_>,
    navigator: &crate::reader_state::PercentNavigator,
    palette: Palette,
) {
    let area = centered_rect(56, 34, frame.area());
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::from(Span::styled(
            "Jump to a stable position in the book.",
            palette.muted(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Percentage: ", palette.body()),
            Span::styled(format!("{}%", navigator.input), palette.accent()),
        ]),
    ];
    if let Some(error) = navigator.error.as_deref() {
        lines.push(Line::from(Span::styled(error.to_string(), palette.error())));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter confirms · Esc cancels · Range 0–100, one decimal",
        palette.muted(),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(popup_block(" Go to percentage ", palette))
            .style(palette.popup())
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_chapters(
    frame: &mut ratatui::Frame<'_>,
    state: &ReaderState,
    navigator: &ChapterNavigator,
    palette: Palette,
) {
    let area = centered_rect(78, 78, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(popup_block(" Chapters ", palette), area);
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 1,
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let prompt = if navigator.filter_active {
        Line::from(vec![
            Span::styled("Filter: /", palette.body()),
            Span::styled(navigator.filter_query.clone(), palette.accent()),
        ])
    } else {
        Line::from(vec![
            Span::styled("Chapter number: ", palette.body()),
            Span::styled(navigator.number_input.clone(), palette.accent()),
        ])
    };
    let count = state.visible_chapters(navigator).len();
    frame.render_widget(
        Paragraph::new(vec![
            prompt,
            Line::from(Span::styled(
                format!("{count} matching chapters"),
                palette.muted(),
            )),
        ])
        .style(palette.popup()),
        chunks[0],
    );

    let visible = state.visible_chapters(navigator);
    let max_items = chunks[1].height as usize;
    let selected_row = visible
        .iter()
        .position(|index| *index == navigator.selection)
        .unwrap_or(0);
    let start = selected_row.saturating_sub(max_items.saturating_sub(1) / 2);
    let lines = visible
        .iter()
        .skip(start)
        .take(max_items)
        .map(|&section_index| {
            let text = format!(
                " {:>4}  {} ",
                section_index + 1,
                state.document.sections[section_index].title
            );
            if section_index == navigator.selection {
                Line::from(Span::styled(text, palette.selected()))
            } else {
                Line::from(Span::styled(text, palette.body()))
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).style(palette.popup()), chunks[1]);

    let footer = navigator
        .error
        .as_deref()
        .map(|error| Line::from(Span::styled(error.to_string(), palette.error())))
        .unwrap_or_else(|| {
            Line::from(Span::styled(
                if navigator.filter_active {
                    "Esc clears filter · Enter jumps"
                } else {
                    "/ filters titles · Enter jumps · Esc closes"
                },
                palette.muted(),
            ))
        });
    frame.render_widget(Paragraph::new(footer).style(palette.popup()), chunks[2]);
}

fn draw_search(
    frame: &mut ratatui::Frame<'_>,
    input: &crate::reader_state::SearchInput,
    palette: Palette,
) {
    let area = centered_rect(62, 32, frame.area());
    frame.render_widget(Clear, area);
    let mut lines = vec![Line::from(vec![
        Span::styled("/", palette.accent()),
        Span::styled(input.query.clone(), palette.body()),
    ])];
    if let Some(error) = input.error.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(error.to_string(), palette.error())));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter searches · Esc cancels · n/N moves through results",
        palette.muted(),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(popup_block(" Search ", palette))
            .style(palette.popup()),
        area,
    );
}

fn draw_metadata(frame: &mut ratatui::Frame<'_>, state: &ReaderState, palette: Palette) {
    let area = centered_rect(72, 62, frame.area());
    frame.render_widget(Clear, area);
    let metadata = &state.document.metadata;
    let mut lines = vec![
        metadata_line("Title", &metadata.title, palette),
        metadata_line("Format", metadata.format.label(), palette),
        metadata_line("Path", &metadata.path.display().to_string(), palette),
        metadata_line("Chapters", &metadata.section_count.to_string(), palette),
    ];
    if let Some(author) = metadata.author.as_deref() {
        lines.push(metadata_line("Author", author, palette));
    }
    if let Some(language) = metadata.language.as_deref() {
        lines.push(metadata_line("Language", language, palette));
    }
    if let Some(publisher) = metadata.publisher.as_deref() {
        lines.push(metadata_line("Publisher", publisher, palette));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc or Enter closes",
        palette.muted(),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(popup_block(" Book information ", palette))
            .style(palette.popup())
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn metadata_line(label: &str, value: &str, palette: Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), palette.muted()),
        Span::styled(value.to_string(), palette.body()),
    ])
}

fn draw_help(frame: &mut ratatui::Frame<'_>, palette: Palette) {
    let area = centered_rect(92, 90, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(popup_block(" Reader help ", palette), area);
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 1,
    });
    let midpoint = READING_BINDINGS.len().div_ceil(2);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let mut left = help_lines("Reading", &READING_BINDINGS[..midpoint], palette);
    left.push(Line::from(""));
    left.push(Line::from(Span::styled(
        "Esc does nothing at the reading root.",
        palette.muted(),
    )));
    let mut right = help_lines(
        "More reading commands",
        &READING_BINDINGS[midpoint..],
        palette,
    );
    right.extend([
        Line::from(""),
        Line::from(Span::styled("Chapter navigator", palette.accent())),
        Line::from(Span::styled(
            "  arrows/j/k browse · digits select · / filters",
            palette.body(),
        )),
        Line::from(Span::styled(
            "  first Esc clears a filter; second Esc closes",
            palette.body(),
        )),
        Line::from(""),
        Line::from(Span::styled("Esc or Enter closes help", palette.muted())),
    ]);
    frame.render_widget(Paragraph::new(left).style(palette.popup()), columns[0]);
    frame.render_widget(Paragraph::new(right).style(palette.popup()), columns[1]);
}

fn draw_privacy(frame: &mut ratatui::Frame<'_>, state: &ReaderState, palette: Palette) {
    let [stack_area, reading_area] = privacy_chunks(frame.area());
    let stack_lines = privacy_lines(stack_area.height as usize, palette);
    frame.render_widget(
        Paragraph::new(stack_lines)
            .style(palette.base())
            .wrap(Wrap { trim: false }),
        stack_area,
    );
    let page = &state.pages[state.current_page];
    frame.render_widget(
        Paragraph::new(page.lines.join("\n")).style(
            palette
                .muted()
                .add_modifier(Modifier::DIM)
                .bg(palette.background),
        ),
        reading_area,
    );
}

fn privacy_lines(height: usize, palette: Palette) -> Vec<Line<'static>> {
    let source = privacy_stack()
        .lines()
        .map(|line| {
            if line == PRIVACY_ERROR_LINE {
                Line::from(Span::styled(line.to_string(), palette.error()))
            } else if line.trim().is_empty() {
                Line::from(" ")
            } else {
                Line::from(Span::styled(line.to_string(), palette.body()))
            }
        })
        .collect::<Vec<_>>();
    if height == 0 {
        return source;
    }
    let mut lines = Vec::with_capacity(height);
    while lines.len() < height {
        lines.extend(source.iter().cloned());
    }
    lines.truncate(height);
    lines
}

fn truncate_width(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}…");
        if candidate.width() > width {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookDocument, BookFormat, BookMetadata, Section};
    use crate::config::ReaderConfig;
    use crate::storage::Progress;
    use crate::ui::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn state(theme: Theme) -> ReaderState {
        let path = std::path::PathBuf::from("book.txt");
        let sections = vec![Section {
            title: "Opening".into(),
            text: "bright readable text".into(),
        }];
        let mut config = ReaderConfig::default();
        config.theme = theme;
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
            Rect::new(0, 0, 80, 12),
            config,
            Progress::default(),
        )
    }

    #[test]
    fn dark_and_light_themes_use_explicit_high_contrast_body_colors() {
        for theme in [Theme::Dark, Theme::Light] {
            let state = state(theme);
            let backend = TestBackend::new(80, 12);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| draw(frame, &state)).unwrap();
            let cell = terminal.backend().buffer().cell((0, 1)).unwrap();
            let palette = Palette::for_theme(theme);
            assert_eq!(cell.fg, palette.body);
            assert_eq!(cell.bg, palette.background);
        }
    }

    #[test]
    fn narrow_hud_keeps_core_keys() {
        let mut state = state(Theme::Dark);
        state.resize(20, 12);
        let line = context_action_line(&state, Palette::for_theme(Theme::Dark));
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        for key in ["k", "j", "g", "t", "?", "q"] {
            assert!(text.contains(key));
        }
        assert!(text.width() <= usize::from(state.viewport.width));
    }

    #[test]
    fn every_reader_overlay_renders_with_its_context() {
        let cases = [
            (Overlay::Percent(Default::default()), "Go to percentage"),
            (Overlay::Chapters(Default::default()), "Chapters"),
            (Overlay::Search(Default::default()), "Search"),
            (Overlay::Metadata, "Book information"),
            (Overlay::Help, "Reader help"),
        ];

        for (overlay, expected) in cases {
            let mut state = state(Theme::Dark);
            state.overlay = overlay;
            let backend = TestBackend::new(80, 20);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| draw(frame, &state)).unwrap();
            assert!(format!("{}", terminal.backend()).contains(expected));
        }
    }

    #[test]
    fn privacy_render_hides_hud_and_metadata() {
        let mut state = state(Theme::Dark);
        state.config.privacy_mode = true;
        state.overlay = Overlay::Metadata;
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("Uncaught TypeError"));
        assert!(!screen.contains("Book information"));
        assert!(!screen.contains("[g]"));
    }
}
