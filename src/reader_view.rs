use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::book::ContentsSource;
use crate::discreet::{AgentDensity, AgentLayout, AgentViewMode};
use crate::reader_input::{DISCREET_BINDINGS, LOCAL_COMMANDS, READING_BINDINGS};
use crate::reader_state::{
    format_tenths, ContentsNavigator, NotificationKind, Overlay, ReaderState,
};
use crate::ui::{action_line, centered_rect, fill_background, help_lines, hud_chunks, Palette};

pub(crate) fn draw(frame: &mut ratatui::Frame<'_>, state: &ReaderState) {
    let palette = Palette::terminal();
    fill_background(frame, palette);
    if state.hidden {
        draw_hidden(frame, palette);
        return;
    }
    if state.config.discreet_mode {
        draw_discreet(frame, state, palette);
        return;
    }

    let body_area = if state.config.immersive_mode {
        frame.area()
    } else {
        let [status_area, body_area, action_area] = hud_chunks(frame.area());
        frame.render_widget(
            Paragraph::new(status_line(state, palette)).style(palette.base()),
            status_area,
        );
        frame.render_widget(
            Paragraph::new(context_action_line(state, palette)).style(palette.base()),
            action_area,
        );
        body_area
    };

    let text_area = reading_text_area(body_area, state.config.text_width);
    frame.render_widget(
        Paragraph::new(render_reading_lines(state, palette)).style(palette.body()),
        text_area,
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
        Overlay::Contents(navigator) => draw_contents(frame, state, navigator, palette),
        Overlay::Search(input) => draw_search(frame, input, palette),
        Overlay::SearchResults(navigator) => draw_search_results(frame, state, navigator, palette),
        Overlay::Bookmarks(navigator) => draw_bookmarks(frame, state, navigator, palette),
        Overlay::Metadata => draw_metadata(frame, state, palette),
        Overlay::Help => draw_help(frame, palette),
    }
}

fn status_line(state: &ReaderState, palette: Palette) -> Line<'static> {
    let section_index = state.current_section();
    let section_count = state.document.sections.len();
    let section_label = state.document.format.section_label();
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
                format!(
                    "  {section_label} {}/{}",
                    section_index + 1,
                    section_count.max(1)
                ),
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
                "{section_label} {}/{}: {}",
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
        Overlay::Contents(navigator) if navigator.filter_active => prompt_actions(
            &[
                ("Type", "Filter"),
                ("Up/Down", "Select"),
                ("Enter", "Jump"),
                ("Esc", "Clear"),
            ],
            palette,
        ),
        Overlay::Contents(_) => prompt_actions(
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
        Overlay::SearchResults(_) => prompt_actions(
            &[
                ("j/k", "Select"),
                ("Enter", "Jump"),
                ("n/N", "Next/Prev"),
                ("Esc", "Close"),
            ],
            palette,
        ),
        Overlay::Bookmarks(_) => prompt_actions(
            &[
                ("j/k", "Select"),
                ("Enter", "Jump"),
                ("d", "Delete"),
                ("Esc", "Close"),
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

fn draw_contents(
    frame: &mut ratatui::Frame<'_>,
    state: &ReaderState,
    navigator: &ContentsNavigator,
    palette: Palette,
) {
    let area = centered_rect(78, 78, frame.area());
    frame.render_widget(Clear, area);
    let title = if state.document.contents_source == ContentsSource::Generated {
        " Contents (generated) "
    } else {
        " Contents "
    };
    frame.render_widget(popup_block(title, palette), area);
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
            Span::styled("Entry number: ", palette.body()),
            Span::styled(navigator.number_input.clone(), palette.accent()),
        ])
    };
    let count = state.visible_contents(navigator).len();
    frame.render_widget(
        Paragraph::new(vec![
            prompt,
            Line::from(Span::styled(
                format!("{count} matching entries"),
                palette.muted(),
            )),
        ])
        .style(palette.popup()),
        chunks[0],
    );

    let visible = state.visible_contents(navigator);
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
        .map(|&entry_index| {
            let entry = &state.document.contents[entry_index];
            let indentation = "  ".repeat(entry.depth.min(12));
            let text = format!(" {:>4}  {indentation}{} ", entry_index + 1, entry.title);
            if entry_index == navigator.selection {
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

fn render_reading_lines(state: &ReaderState, palette: Palette) -> Vec<Line<'static>> {
    let page = &state.pages[state.current_page];
    let overlap_count = page.overlap_lines;
    let relevant_matches: Vec<(usize, usize, usize)> = if state.search_results.is_empty() {
        Vec::new()
    } else {
        state
            .search_results
            .iter()
            .enumerate()
            .filter(|(_, result)| result.position.section_index == page.section_index)
            .map(|(idx, result)| {
                (
                    idx,
                    result.position.char_offset,
                    result.end_position.char_offset,
                )
            })
            .collect()
    };

    if relevant_matches.is_empty() {
        return page
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let style = if idx < overlap_count {
                    palette.muted()
                } else {
                    palette.body()
                };
                Line::from(Span::styled(line.clone(), style))
            })
            .collect();
    }

    let mut rendered_lines = Vec::with_capacity(page.anchored_lines.len());
    for (line_idx, line) in page.anchored_lines.iter().enumerate() {
        if line.text.is_empty() {
            rendered_lines.push(Line::from(""));
            continue;
        }

        let base_style = if line_idx < overlap_count {
            palette.muted()
        } else {
            palette.body()
        };

        let has_overlap = relevant_matches
            .iter()
            .any(|&(_, start, end)| start < line.end_char && end > line.start_char);

        if !has_overlap || line.char_offsets.is_empty() {
            rendered_lines.push(Line::from(Span::styled(line.text.clone(), base_style)));
            continue;
        }

        let chars: Vec<char> = line.text.chars().collect();
        let mut spans = Vec::new();
        let mut span_chars = String::new();
        let mut current_style = None;

        for (i, &ch) in chars.iter().enumerate() {
            let src_offset = line.char_offsets.get(i).copied().unwrap_or(line.start_char);
            let mut char_style = base_style;
            for &(match_idx, m_start, m_end) in &relevant_matches {
                if src_offset >= m_start && src_offset < m_end {
                    if match_idx == state.search_index {
                        char_style = palette.search_match_current();
                        break;
                    } else {
                        char_style = palette.search_match_secondary();
                    }
                }
            }

            match current_style {
                None => {
                    current_style = Some(char_style);
                    span_chars.push(ch);
                }
                Some(style) if style == char_style => {
                    span_chars.push(ch);
                }
                Some(style) => {
                    spans.push(Span::styled(std::mem::take(&mut span_chars), style));
                    current_style = Some(char_style);
                    span_chars.push(ch);
                }
            }
        }

        if !span_chars.is_empty() {
            spans.push(Span::styled(
                span_chars,
                current_style.unwrap_or(base_style),
            ));
        }

        rendered_lines.push(Line::from(spans));
    }

    rendered_lines
}

fn draw_search_results(
    frame: &mut ratatui::Frame<'_>,
    state: &ReaderState,
    navigator: &crate::reader_state::SearchResultsNavigator,
    palette: Palette,
) {
    let area = centered_rect(78, 68, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(popup_block(" Search Results ", palette), area);
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

    let query_label = state.search_query.as_deref().unwrap_or("");
    let count = state.search_results.len();
    let prompt = Line::from(vec![
        Span::styled("Query: /", palette.body()),
        Span::styled(query_label, palette.accent()),
    ]);
    frame.render_widget(
        Paragraph::new(vec![
            prompt,
            Line::from(Span::styled(
                format!(
                    "{count} matching results (Selected {}/{count})",
                    navigator.selection + 1
                ),
                palette.muted(),
            )),
        ])
        .style(palette.popup()),
        chunks[0],
    );

    let visible_items = chunks[1].height as usize;
    let selected_row = navigator.selection.min(count.saturating_sub(1));
    let start = selected_row.saturating_sub(visible_items.saturating_sub(1) / 2);
    let lines = state
        .search_results
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_items)
        .map(|(idx, result)| {
            let section_idx = result.position.section_index;
            let section_title = state
                .document
                .sections
                .get(section_idx)
                .map(|s| s.title.as_str())
                .unwrap_or("Section");
            let text = format!(
                " {:>3}/{}  [{}]  {} ",
                idx + 1,
                count,
                section_title,
                result.preview
            );
            if idx == navigator.selection {
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
        .map(|err| Line::from(Span::styled(err.to_string(), palette.error())))
        .unwrap_or_else(|| {
            Line::from(Span::styled(
                "Enter jumps · j/k moves · Esc closes · n/N moves through results",
                palette.muted(),
            ))
        });
    frame.render_widget(Paragraph::new(footer).style(palette.popup()), chunks[2]);
}

fn bookmark_preview(
    document: &crate::book::BookDocument,
    position: crate::book::TextPosition,
) -> String {
    let Some(section) = document.sections.get(position.section_index) else {
        return String::new();
    };
    let chars: Vec<char> = section.text.chars().collect();
    let start = position.char_offset;
    let end = (start + 35).min(chars.len());
    if start >= chars.len() {
        return String::new();
    }
    let snippet: String = chars[start..end].iter().collect();
    snippet.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn draw_bookmarks(
    frame: &mut ratatui::Frame<'_>,
    state: &ReaderState,
    navigator: &crate::reader_state::BookmarksNavigator,
    palette: Palette,
) {
    let area = centered_rect(78, 68, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(popup_block(" Bookmarks ", palette), area);
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

    let count = state.bookmarks.len();
    let header = if count == 0 {
        Line::from(Span::styled("No saved bookmarks", palette.muted()))
    } else {
        Line::from(vec![Span::styled(
            format!(
                "{count} saved bookmark{} (Selected {}/{count})",
                if count == 1 { "" } else { "s" },
                navigator.selection + 1
            ),
            palette.muted(),
        )])
    };
    frame.render_widget(Paragraph::new(header).style(palette.popup()), chunks[0]);

    if count == 0 {
        let empty_msg = Paragraph::new(vec![
            Line::from(Span::styled("No bookmarks found.", palette.body())),
            Line::from(Span::styled(
                "Press 'b' during reading to bookmark the current position.",
                palette.muted(),
            )),
        ])
        .style(palette.popup());
        frame.render_widget(empty_msg, chunks[1]);
    } else {
        let visible_items = chunks[1].height as usize;
        let selected_row = navigator.selection.min(count.saturating_sub(1));
        let start = selected_row.saturating_sub(visible_items.saturating_sub(1) / 2);
        let lines = state
            .bookmarks
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_items)
            .map(|(idx, bookmark)| {
                let section_idx = bookmark.position.section_index;
                let section_title = state
                    .document
                    .sections
                    .get(section_idx)
                    .map(|s| s.title.as_str())
                    .unwrap_or("Section");
                let pct = format_tenths(state.metrics.progress_tenths(bookmark.position));
                let preview = bookmark_preview(&state.document, bookmark.position);
                let preview_part = if preview.is_empty() {
                    String::new()
                } else {
                    format!(" · \"{preview}\"")
                };
                let text = format!(
                    " {:>2}. [{}] {} ({}){} ",
                    idx + 1,
                    section_title,
                    bookmark.label,
                    pct,
                    preview_part,
                );
                if idx == navigator.selection {
                    Line::from(Span::styled(text, palette.selected()))
                } else {
                    Line::from(Span::styled(text, palette.body()))
                }
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).style(palette.popup()), chunks[1]);
    }

    let footer = navigator
        .error
        .as_deref()
        .map(|err| Line::from(Span::styled(err.to_string(), palette.error())))
        .unwrap_or_else(|| {
            Line::from(Span::styled(
                "Enter jumps · j/k moves · d deletes · Esc closes",
                palette.muted(),
            ))
        });
    frame.render_widget(Paragraph::new(footer).style(palette.popup()), chunks[2]);
}

fn draw_metadata(frame: &mut ratatui::Frame<'_>, state: &ReaderState, palette: Palette) {
    let area = centered_rect(72, 62, frame.area());
    frame.render_widget(Clear, area);
    let metadata = &state.document.metadata;
    let mut lines = vec![
        metadata_line("Title", &metadata.title, palette),
        metadata_line("Format", metadata.format.label(), palette),
        metadata_line("Path", &metadata.path.display().to_string(), palette),
        metadata_line(
            metadata.format.section_label_plural(),
            &metadata.section_count.to_string(),
            palette,
        ),
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
        Line::from(Span::styled("Contents navigator", palette.accent())),
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

fn agent_symbol(symbol: &'static str, fallback: &'static str) -> &'static str {
    if std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        fallback
    } else {
        symbol
    }
}

fn split_row_line(
    left_spans: Vec<Span<'static>>,
    right_spans: Vec<Span<'static>>,
    width: u16,
) -> Line<'static> {
    let left_len: usize = left_spans.iter().map(|s| s.content.width()).sum();
    let right_len: usize = right_spans.iter().map(|s| s.content.width()).sum();
    let pad = usize::from(width).saturating_sub(left_len + right_len);
    let mut spans = left_spans;
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    spans.extend(right_spans);
    Line::from(spans)
}

fn draw_discreet(frame: &mut ratatui::Frame<'_>, state: &ReaderState, palette: Palette) {
    let layout = AgentLayout::new(frame.area(), state.config.text_width);
    if frame.area().height < 6 || layout.body.width == 0 {
        frame.render_widget(
            Paragraph::new("Resize terminal").style(palette.muted()),
            frame.area(),
        );
        return;
    }
    let (start, end, total) = state.physical_row_range();
    let net_height = layout.net_text_height();
    let width = frame.area().width;

    // 1. Top Session Context Banner (Full mode only)
    if layout.mode == AgentViewMode::Full && layout.banner.height >= 3 {
        let banner_lines = vec![
            split_row_line(
                vec![
                    Span::styled("workspace: ", palette.muted()),
                    Span::styled("Reader-cli", palette.body()),
                ],
                vec![
                    Span::styled("session: ", palette.muted()),
                    Span::styled("local", palette.body()),
                ],
                width,
            ),
            split_row_line(
                vec![
                    Span::styled("mode: ", palette.muted()),
                    Span::styled("Agent Terminal Reading", palette.body()),
                ],
                vec![
                    Span::styled("file: ", palette.muted()),
                    Span::styled("context.txt", palette.body()),
                ],
                width,
            ),
            Line::styled(
                agent_symbol("─", "-").repeat(usize::from(width)),
                palette.muted(),
            ),
        ];
        frame.render_widget(Paragraph::new(banner_lines), layout.banner);
    }

    // 2. Session Context & Tool Activity (header)
    let inspect_line = Line::from(vec![
        Span::styled(format!("{} ", agent_symbol("❯", ">")), palette.accent()),
        Span::styled("inspect context", palette.body()),
    ]);
    let tool_call_line = Line::from(vec![
        Span::styled(format!("{} ", agent_symbol("●", "*")), palette.accent()),
        Span::styled("Read(local chunk)", palette.body()),
    ]);
    let tool_res_line = Line::from(vec![Span::styled(
        format!(
            "  {} Loaded source range {start}–{end}",
            agent_symbol("⎿", "\\")
        ),
        palette.muted(),
    )]);

    let header_lines = if layout.mode == AgentViewMode::Full {
        if layout.header.height >= 6 {
            vec![
                inspect_line,
                Line::from(vec![
                    Span::styled(format!("{} ", agent_symbol("●", "*")), palette.accent()),
                    Span::styled("Context initialized", palette.body()),
                ]),
                Line::styled("  source: local text corpus", palette.muted()),
                Line::styled("  chunking: paragraph-aware", palette.muted()),
                tool_call_line,
                tool_res_line,
            ]
        } else {
            vec![
                inspect_line,
                Line::from(vec![
                    Span::styled(format!("{} ", agent_symbol("●", "*")), palette.accent()),
                    Span::styled(
                        "Context initialized · source: local text corpus",
                        palette.muted(),
                    ),
                ]),
                tool_call_line,
                tool_res_line,
            ]
        }
    } else {
        match layout.header.height {
            1 => vec![Line::from(vec![
                Span::styled(format!("{} ", agent_symbol("●", "*")), palette.accent()),
                Span::styled(
                    format!("Read(local chunk) · range {start}–{end}"),
                    palette.body(),
                ),
            ])],
            _ => vec![tool_call_line, tool_res_line],
        }
    };
    frame.render_widget(Paragraph::new(header_lines), layout.header);

    // 3. Assistant Response (body)
    let mut body_lines = Vec::new();
    if state.agent.help {
        let mut help_entries = vec![
            Line::from("Local commands · j/k scroll · Esc close"),
            Line::from(""),
        ];
        help_entries.extend(
            DISCREET_BINDINGS
                .iter()
                .map(|b| Line::from(format!("  {:<12} {}", b.keys, b.label))),
        );
        help_entries.push(Line::from(""));
        help_entries.push(Line::from("Executable local commands:"));
        help_entries.extend(
            LOCAL_COMMANDS
                .iter()
                .map(|(name, _)| Line::from(format!("  :{:<11} execute command", name))),
        );
        let visible_help = help_entries
            .into_iter()
            .skip(state.agent.help_offset)
            .take(usize::from(net_height));
        for line in visible_help {
            body_lines.push(Line::from(vec![
                Span::styled("  ", palette.muted()),
                Span::styled(line.to_string(), palette.body()),
            ]));
        }
    } else {
        body_lines.push(Line::from(vec![
            Span::styled(format!("{} ", agent_symbol("●", "*")), palette.accent()),
            Span::styled("Assistant", palette.body()),
        ]));

        let visible_lines = state.agent.view.visible(net_height);
        for (i, vline) in visible_lines.iter().enumerate() {
            let line_num = start + i;
            let is_overlap = i < state.agent.view.context_rows;
            let text_style = if is_overlap {
                palette.muted()
            } else {
                palette.body()
            };
            let raw_text = &vline.line.text;
            match state.agent.density {
                AgentDensity::Comfortable => {
                    body_lines.push(Line::from(vec![Span::styled(raw_text.clone(), text_style)]));
                }
                AgentDensity::Inspect => {
                    body_lines.push(Line::from(vec![
                        Span::styled(format!("{:>4} │ ", line_num), palette.muted()),
                        Span::styled(raw_text.clone(), text_style),
                    ]));
                }
            }
        }

        if layout.body.height > net_height + 1 {
            body_lines.push(Line::from(vec![Span::styled(
                "  [Output complete]",
                palette.muted(),
            )]));
        }
    }
    frame.render_widget(Paragraph::new(body_lines), layout.body);

    // 4. Activity & Actions (footer)
    let current_chunk = (start / net_height as usize).saturating_add(1);
    let total_chunks = (total / net_height as usize).max(1);
    let chunk_label = format!("chunk {current_chunk}/{total_chunks}");

    let status_hint = match state.agent.receipt {
        Some(receipt) => format!("✓ {receipt}"),
        None if state.agent.command.is_some() => {
            "Enter execute · Esc cancel · Tab complete".to_string()
        }
        None if state.agent.help => "j/k scroll · Esc close".to_string(),
        None => "j/k scroll · : cmd · / find · F12 fold".to_string(),
    };

    let prompt_spans = match &state.agent.command {
        Some(text) => vec![
            Span::styled(format!("{} :", agent_symbol("❯", ">")), palette.accent()),
            Span::styled(text.clone(), palette.body()),
            Span::styled("█", palette.accent()),
        ],
        None => vec![
            Span::styled(format!("{} ", agent_symbol("❯", ">")), palette.muted()),
            Span::styled("continue_", palette.muted()),
        ],
    };

    let footer_lines = if layout.mode == AgentViewMode::Full && layout.footer.height >= 3 {
        vec![
            Line::styled(
                agent_symbol("─", "-").repeat(usize::from(width)),
                palette.muted(),
            ),
            split_row_line(
                vec![
                    Span::styled("activity: ", palette.muted()),
                    Span::styled("init ✓   read ✓   render ✓", palette.accent()),
                ],
                vec![Span::styled(chunk_label, palette.muted())],
                width,
            ),
            split_row_line(
                prompt_spans,
                vec![Span::styled(status_hint, palette.muted())],
                width,
            ),
        ]
    } else {
        match layout.footer.height {
            0 => Vec::new(),
            1 => vec![Line::from(prompt_spans)],
            _ => vec![
                Line::from(prompt_spans),
                Line::styled(format!("  {status_hint}"), palette.muted()),
            ],
        }
    };
    frame.render_widget(Paragraph::new(footer_lines), layout.footer);

    if let Some(text) = state.agent.command.as_ref() {
        let prompt_row_offset = if layout.mode == AgentViewMode::Full && layout.footer.height >= 3 {
            2
        } else {
            0
        };
        let row = layout.footer.y + prompt_row_offset;
        let col = (3 + text.width() as u16).min(layout.footer.width.saturating_sub(1));
        frame.set_cursor_position((layout.footer.x + col, row));
    }
}

// Deliberately accepts no ReaderState, document, notification or input buffer.
fn draw_hidden(frame: &mut ratatui::Frame<'_>, palette: Palette) {
    let layout = AgentLayout::new(frame.area(), None);
    let width = frame.area().width;

    if layout.mode == AgentViewMode::Full && layout.banner.height >= 3 {
        let banner_lines = vec![
            split_row_line(
                vec![
                    Span::styled("workspace: ", palette.muted()),
                    Span::styled("Reader-cli", palette.body()),
                ],
                vec![
                    Span::styled("session: ", palette.muted()),
                    Span::styled("local", palette.body()),
                ],
                width,
            ),
            split_row_line(
                vec![
                    Span::styled("mode: ", palette.muted()),
                    Span::styled("Agent Terminal Reading", palette.body()),
                ],
                vec![
                    Span::styled("file: ", palette.muted()),
                    Span::styled("context.txt", palette.body()),
                ],
                width,
            ),
            Line::styled(
                agent_symbol("─", "-").repeat(usize::from(width)),
                palette.muted(),
            ),
        ];
        frame.render_widget(Paragraph::new(banner_lines), layout.banner);
    }

    let inspect_line = Line::from(vec![
        Span::styled(format!("{} ", agent_symbol("❯", ">")), palette.accent()),
        Span::styled("inspect context", palette.body()),
    ]);
    let tool_call_line = Line::from(vec![
        Span::styled(format!("{} ", agent_symbol("●", "*")), palette.accent()),
        Span::styled("Read(local chunk)", palette.body()),
    ]);
    let tool_res_line = Line::from(vec![Span::styled(
        format!(
            "  {} Session paused · Output folded (F12 resume)",
            agent_symbol("⎿", "\\")
        ),
        palette.muted(),
    )]);

    let header_lines = if layout.mode == AgentViewMode::Full {
        vec![inspect_line, tool_call_line, tool_res_line]
    } else {
        match layout.header.height {
            1 => vec![tool_call_line],
            _ => vec![tool_call_line, tool_res_line],
        }
    };
    frame.render_widget(Paragraph::new(header_lines), layout.header);

    let prompt_spans = vec![
        Span::styled(format!("{} ", agent_symbol("❯", ">")), palette.muted()),
        Span::styled("continue_", palette.muted()),
    ];

    let footer_lines = if layout.mode == AgentViewMode::Full && layout.footer.height >= 3 {
        vec![
            Line::styled(
                agent_symbol("─", "-").repeat(usize::from(width)),
                palette.muted(),
            ),
            split_row_line(
                vec![
                    Span::styled("activity: ", palette.muted()),
                    Span::styled("session paused", palette.muted()),
                ],
                vec![Span::styled("chunk folded", palette.muted())],
                width,
            ),
            split_row_line(
                prompt_spans,
                vec![Span::styled("F12 resume", palette.muted())],
                width,
            ),
        ]
    } else {
        match layout.footer.height {
            0 => Vec::new(),
            1 => vec![Line::from(prompt_spans)],
            _ => vec![
                Line::from(prompt_spans),
                Line::styled("  F12 resume", palette.muted()),
            ],
        }
    };
    frame.render_widget(Paragraph::new(footer_lines), layout.footer);
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
    use crate::book::{
        BookDocument, BookFormat, BookMetadata, ContentsEntry, ContentsSource, Section,
        TextPosition,
    };
    use crate::config::ReaderConfig;
    use crate::storage::Progress;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn state() -> ReaderState {
        let path = std::path::PathBuf::from("book.txt");
        let sections = vec![Section {
            title: "Opening".into(),
            text: "bright readable text".into(),
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
                contents: vec![ContentsEntry {
                    title: "Opening".into(),
                    depth: 0,
                    target: crate::book::TextPosition::new(0, 0),
                }],
                contents_source: ContentsSource::Generated,
            },
            "book".into(),
            Rect::new(0, 0, 80, 12),
            ReaderConfig::default(),
            Progress::default(),
            Vec::new(),
        )
    }

    fn pdf_state() -> ReaderState {
        let path = std::path::PathBuf::from("manual.pdf");
        let sections = vec![
            Section {
                title: "Page 1".into(),
                text: "first PDF page".into(),
            },
            Section {
                title: "Page 2".into(),
                text: "second PDF page".into(),
            },
        ];
        ReaderState::new(
            BookDocument {
                id: "pdf".into(),
                title: "Manual".into(),
                path: path.clone(),
                format: BookFormat::Pdf,
                metadata: BookMetadata {
                    title: "Manual".into(),
                    author: Some("Writer".into()),
                    language: None,
                    publisher: None,
                    format: BookFormat::Pdf,
                    path,
                    section_count: sections.len(),
                },
                sections,
                contents: vec![
                    ContentsEntry {
                        title: "Introduction".into(),
                        depth: 0,
                        target: crate::book::TextPosition::new(0, 0),
                    },
                    ContentsEntry {
                        title: "Nested topic".into(),
                        depth: 1,
                        target: crate::book::TextPosition::new(1, 0),
                    },
                ],
                contents_source: ContentsSource::Document,
            },
            "pdf".into(),
            Rect::new(0, 0, 80, 20),
            ReaderConfig::default(),
            Progress::default(),
            Vec::new(),
        )
    }

    #[test]
    fn reading_view_inherits_terminal_foreground_and_background() {
        let state = state();
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let cell = terminal.backend().buffer().cell((0, 1)).unwrap();
        assert_eq!(cell.fg, ratatui::style::Color::Reset);
        assert_eq!(cell.bg, ratatui::style::Color::Reset);
    }

    #[test]
    fn narrow_hud_keeps_core_keys() {
        let mut state = state();
        state.resize(20, 12);
        let line = context_action_line(&state, Palette::terminal());
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
            (Overlay::Contents(Default::default()), "Contents"),
            (Overlay::Search(Default::default()), "Search"),
            (Overlay::Metadata, "Book information"),
            (Overlay::Help, "Reader help"),
        ];

        for (overlay, expected) in cases {
            let mut state = state();
            state.overlay = overlay;
            let backend = TestBackend::new(80, 20);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| draw(frame, &state)).unwrap();
            assert!(format!("{}", terminal.backend()).contains(expected));
        }
    }

    #[test]
    fn generated_contents_are_explicitly_labeled() {
        let mut state = state();
        state.overlay = Overlay::Contents(Default::default());
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();

        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("Contents (generated)"));
    }

    #[test]
    fn pdf_reader_uses_page_and_contents_labels() {
        let mut state = pdf_state();
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("Page 1/2: Page 1"));

        state.overlay = Overlay::Contents(Default::default());
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("Contents"));
        assert!(screen.contains("Entry number:"));
        assert!(screen.contains("2 matching entries"));
        assert!(screen.contains("Nested topic"));
        assert!(screen.contains("    2    Nested topic"));

        state.overlay = Overlay::Metadata;
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("Format: PDF"));
        assert!(screen.contains("Pages: 2"));
    }

    #[test]
    fn agent_terminal_renders_real_content_without_metadata_or_logs() {
        let mut s = state();
        s.resize(80, 24);
        s.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        s.notify("SECRET notification", NotificationKind::Error);
        s.overlay = Overlay::Metadata;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &s)).unwrap();
        let screen = format!("{}", terminal.backend());
        for expected in [
            "Read(local chunk)",
            "Assistant",
            "Loaded source range",
            "bright readable text",
            "continue_",
        ] {
            assert!(screen.contains(expected), "{screen}");
        }
        for secret in [
            "SECRET",
            "TypeError",
            "INFO",
            "WARN",
            "Book information",
            "book.txt",
        ] {
            assert!(!screen.contains(secret), "Leaked secret: {secret}");
        }
        let first = terminal.backend().buffer().clone();
        terminal.draw(|f| draw(f, &s)).unwrap();
        assert_eq!(&first, terminal.backend().buffer());
    }

    #[test]
    fn agent_terminal_four_semantic_regions_and_comfortable_rows() {
        for (w, h, expected_net_rows, is_full) in
            [(80, 24, 7, true), (60, 18, 5, false), (120, 30, 9, true)]
        {
            let mut s = state();
            s.resize(w, h);
            s.update(crate::reader_state::ReaderAction::ToggleDiscreet);
            let layout = AgentLayout::new(s.viewport, s.config.text_width);
            assert_eq!(layout.net_text_height(), expected_net_rows);
            // 60:40 verification: prose <= 40% of height, Agent >= 50%
            assert!(f64::from(layout.net_text_height()) / f64::from(h) <= 0.40);
            assert!(
                f64::from(layout.banner.height + layout.header.height + layout.footer.height)
                    / f64::from(h)
                    >= 0.50
            );

            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &s)).unwrap();
            let screen = format!("{}", terminal.backend());

            if is_full {
                // Top Banner & Scheme A context
                assert!(screen.contains("workspace: Reader-cli"));
                assert!(screen.contains("session: local"));
                assert!(screen.contains("mode: Agent Terminal Reading"));
                assert!(screen.contains("file: context.txt"));
                assert!(screen.contains("Context initialized"));
                assert!(screen.contains("activity: init ✓   read ✓   render ✓"));
                assert!(screen.contains("chunk "));
            }

            // Region 1 & 2: Session Context and Tool Activity
            assert!(screen.contains("Read(local chunk)"));
            assert!(screen.contains("Loaded source range"));

            // Region 3: Assistant Response with verbatim prose
            assert!(screen.contains("Assistant"));
            assert!(screen.contains("bright readable text"));
            assert!(screen.contains("[Output complete]"));

            // Region 4: Input / Action
            assert!(screen.contains("continue_"));
        }
    }

    #[test]
    fn agent_result_preserves_blank_rows_indent_and_terminal_colors() {
        let mut s = state();
        s.document.sections[0].text = "  indented line\n\n  next paragraph".into();
        s.resize(80, 24);
        s.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &s)).unwrap();
        let area = AgentLayout::new(s.viewport, s.config.text_width).body;
        let buffer = terminal.backend().buffer();
        // Comfortable mode (default): Assistant banner on row 0, content starts row 1
        // Row 1: 2-space original indent puts 'i' at area.x + 2
        assert_eq!(buffer.cell((area.x + 2, area.y + 1)).unwrap().symbol(), "i");
        // Row 2 is blank line; Row 3 is next paragraph
        assert_eq!(buffer.cell((area.x + 2, area.y + 3)).unwrap().symbol(), "n");
        let body = buffer.cell((area.x + 2, area.y + 1)).unwrap();
        assert_eq!(body.fg, ratatui::style::Color::Reset);
        assert_eq!(body.bg, ratatui::style::Color::Reset);
        assert!(!body.modifier.contains(ratatui::style::Modifier::DIM));

        // Inspect mode: toggle density to show line numbers
        s.update(crate::reader_state::ReaderAction::ToggleDensity);
        terminal.draw(|f| draw(f, &s)).unwrap();
        let inspect_buf = terminal.backend().buffer();
        // Line number 1 is "    1 │ " (7 columns) followed by 2 spaces indent, 'i' is at area.x + 7 + 2 = 9
        assert_eq!(
            inspect_buf.cell((area.x + 9, area.y + 1)).unwrap().symbol(),
            "i"
        );
    }

    #[test]
    fn f12_clears_old_frames_in_every_approved_size_and_after_resize() {
        let mut s = state();
        s.document.title = "SECRET_TITLE".into();
        s.document.path = "C:/SECRET_PATH/book.txt".into();
        s.document.metadata.author = Some("SECRET_AUTHOR".into());
        s.document.sections[0].text = "SECRET_BODY ".repeat(1000);
        s.search_query = Some("SECRET_QUERY".into());
        s.notify("SECRET_NOTICE", NotificationKind::Error);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        for (w, h) in [(80, 24), (120, 30), (60, 18), (3, 2)] {
            terminal.backend_mut().resize(w, h);
            terminal.resize(Rect::new(0, 0, w, h)).unwrap();
            s.resize(w, h);
            s.hidden = false;
            terminal.draw(|f| draw(f, &s)).unwrap();
            s.update(crate::reader_state::ReaderAction::ToggleHidden);
            terminal.draw(|f| draw(f, &s)).unwrap();
            let screen = format!("{}", terminal.backend());
            assert!(!screen.contains("SECRET"));
            assert!(!screen.contains("book.txt"));
            assert!(!screen.contains("TypeError"));
            if w > 30 {
                assert!(screen.contains("tool output folded") || screen.contains("Session paused"));
            }
        }
    }

    #[test]
    fn agent_checkpoint_receipt_never_moves_the_body() {
        let mut s = state();
        s.resize(80, 24);
        s.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &s)).unwrap();
        let before = terminal.backend().buffer().clone();
        s.agent.receipt = Some("Checkpoint saved");
        s.agent.checkpoints = 1;
        terminal.draw(|f| draw(f, &s)).unwrap();
        let area = AgentLayout::new(s.viewport, s.config.text_width).body;
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                assert_eq!(
                    before.cell((x, y)),
                    terminal.backend().buffer().cell((x, y))
                );
            }
        }
    }

    #[test]
    fn search_highlighting_distinguishes_current_and_secondary_matches_and_handles_multiline() {
        let mut state = state();
        state.search_query = Some("bright".into());
        // "bright readable text" 中 "bright" 在开头 0..6
        state.search_results = vec![
            crate::book::SearchResult {
                position: TextPosition::new(0, 0),
                end_position: TextPosition::new(0, 6),
                preview: "bright readable text".into(),
            },
            crate::book::SearchResult {
                position: TextPosition::new(0, 7),
                end_position: TextPosition::new(0, 15),
                preview: "readable".into(),
            },
        ];
        state.search_index = 0;

        let palette = Palette::terminal();
        let rendered = render_reading_lines(&state, palette);
        let has_current = rendered.iter().any(|line| {
            line.spans
                .iter()
                .any(|s| s.style == palette.search_match_current() && s.content == "bright")
        });
        assert!(
            has_current,
            "Should highlight current match with current style"
        );

        state.search_index = 1;
        let rendered_next = render_reading_lines(&state, palette);
        let has_current_next = rendered_next.iter().any(|line| {
            line.spans
                .iter()
                .any(|s| s.style == palette.search_match_current() && s.content == "readable")
        });
        assert!(
            has_current_next,
            "Should highlight match 1 as current after switching index"
        );
    }

    #[test]
    fn search_results_overlay_renders_list_with_selected_row() {
        let mut state = state();
        state.search_query = Some("test".into());
        state.search_results = vec![
            crate::book::SearchResult {
                position: TextPosition::new(0, 0),
                end_position: TextPosition::new(0, 4),
                preview: "test match 1".into(),
            },
            crate::book::SearchResult {
                position: TextPosition::new(0, 20),
                end_position: TextPosition::new(0, 24),
                preview: "test match 2".into(),
            },
        ];
        state.overlay = Overlay::SearchResults(crate::reader_state::SearchResultsNavigator {
            selection: 1,
            error: None,
        });

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("Search Results"));
        assert!(screen.contains("test match 1"));
        assert!(screen.contains("test match 2"));
        assert!(screen.contains("2 matching results"));
    }

    #[test]
    fn immersive_mode_hides_status_and_action_bars() {
        let mut state = state();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // Normal mode renders title and HUD
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let normal_screen = format!("{}", terminal.backend());
        assert!(normal_screen.contains("Book"));
        assert!(normal_screen.contains("Next"));

        // Immersive mode hides title and HUD
        state.config.immersive_mode = true;
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let immersive_screen = format!("{}", terminal.backend());
        assert!(!immersive_screen.contains("Book  ·"));
        assert!(!immersive_screen.contains("[j] Next"));
    }

    #[test]
    fn bookmarks_overlay_renders_list_and_handles_empty() {
        let mut state = state();
        state.overlay = Overlay::Bookmarks(crate::reader_state::BookmarksNavigator::default());

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // Empty state
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let empty_screen = format!("{}", terminal.backend());
        assert!(empty_screen.contains("Bookmarks"));
        assert!(empty_screen.contains("No bookmarks found"));

        // With bookmarks
        state.bookmarks = vec![crate::storage::Bookmark {
            id: "bm1".into(),
            label: "Chapter 1 Highlight".into(),
            position: TextPosition::new(0, 0),
        }];
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let populated_screen = format!("{}", terminal.backend());
        assert!(populated_screen.contains("Chapter 1 Highlight"));
        assert!(populated_screen.contains("1 saved bookmark"));
        assert!(populated_screen.contains("Enter jumps · j/k moves · d deletes · Esc closes"));
    }

    #[test]
    fn overlap_context_line_is_rendered_with_muted_style() {
        let mut state = state();
        state.pages[0].overlap_lines = 1;
        state.pages[0].lines = vec!["Context Line".into(), "New Content".into()];
        state.pages[0].anchored_lines = vec![
            crate::pagination::AnchoredLine::new("Context Line".into(), 0, 12, (0..12).collect()),
            crate::pagination::AnchoredLine::new("New Content".into(), 12, 23, (12..23).collect()),
        ];

        let palette = Palette::terminal();
        let rendered = render_reading_lines(&state, palette);
        assert_eq!(rendered.len(), 2);
        // 第 0 行是重叠行，样式应为 muted
        assert_eq!(rendered[0].spans[0].style, palette.muted());
        // 第 1 行是新内容，样式应为 body
        assert_eq!(rendered[1].spans[0].style, palette.body());
    }
}
