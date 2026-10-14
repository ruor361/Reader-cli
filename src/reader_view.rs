use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::book::ContentsSource;
use crate::discreet::{discreet_layout, discreet_stack, LogLevel, DISCREET_ERROR_LINE};
use crate::reader_input::READING_BINDINGS;
use crate::reader_state::{
    discreet_chunks, format_tenths, ContentsNavigator, NotificationKind, Overlay, ReaderState,
};
use crate::ui::{
    action_line, centered_rect, fill_background, help_lines, hud_chunks, DiscreetLogColors, Palette,
};

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

fn left_aligned_reading_text_area(area: Rect, text_width: Option<u16>) -> Rect {
    let mut text_area = reading_text_area(area, text_width);
    text_area.x = area.x.saturating_add(u16::from(area.width > 1));
    text_area
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

fn draw_discreet(frame: &mut ratatui::Frame<'_>, state: &ReaderState, palette: Palette) {
    let log_colors = DiscreetLogColors::terminal();
    let palette = palette.discreet_mode();
    let [stack_area, reading_area, action_area] = discreet_chunks(frame.area());
    frame.render_widget(
        Paragraph::new(discreet_diagnostic_lines(
            palette,
            stack_area.height,
            reading_area.height,
        ))
        .style(palette.base()),
        stack_area,
    );
    let layout = discreet_layout(frame.area(), state.config.text_width);
    let text_area = left_aligned_reading_text_area(reading_area, Some(layout.total_width()));
    frame.render_widget(
        Paragraph::new(discreet_reading_lines(state, layout, palette, log_colors))
            .style(palette.body()),
        text_area,
    );
    let (start, end, total) = state.physical_row_range();
    let status_area = Rect::new(
        action_area.x.saturating_add(1),
        action_area.y,
        action_area.width.saturating_sub(1),
        action_area.height,
    );
    frame.render_widget(
        Paragraph::new(format!("rows {start}-{end}/{total}")).style(palette.muted()),
        status_area,
    );
    if let Some(notification) = state.notification.as_ref() {
        draw_notification(
            frame,
            reading_area,
            notification.text.as_str(),
            notification.kind,
            palette,
        );
    }
}

fn discreet_reading_lines(
    state: &ReaderState,
    layout: crate::discreet::DiscreetLayout,
    palette: Palette,
    log_colors: DiscreetLogColors,
) -> Vec<Line<'static>> {
    let page = &state.pages[state.current_page];
    let prefixes = state
        .discreet_log
        .as_ref()
        .map(|log| log.prefixes(&page.lines, layout.prefix_mode))
        .unwrap_or_else(|| vec![None; page.lines.len()]);
    page.lines
        .iter()
        .zip(prefixes)
        .map(|(body, prefix)| {
            let Some(prefix) = prefix else {
                return Line::from(vec![
                    Span::styled(
                        " ".repeat(usize::from(layout.prefix_mode.width())),
                        palette.muted(),
                    ),
                    Span::styled(body.clone(), palette.body()),
                ]);
            };
            let level_color = match prefix.level {
                LogLevel::Info => log_colors.info,
                LogLevel::Warn => log_colors.warning,
                LogLevel::Error => log_colors.error,
            };
            Line::from(vec![
                Span::styled(prefix.leading, palette.muted()),
                Span::styled(
                    prefix.keyword,
                    palette
                        .base()
                        .fg(level_color)
                        .add_modifier(ratatui::style::Modifier::DIM),
                ),
                Span::styled(prefix.trailing, palette.muted()),
                Span::styled(body.clone(), palette.body()),
            ])
        })
        .collect()
}

fn draw_hidden(frame: &mut ratatui::Frame<'_>, palette: Palette) {
    frame.render_widget(
        Paragraph::new(diagnostic_lines(palette))
            .style(palette.base())
            .wrap(Wrap { trim: false }),
        frame.area(),
    );
}

fn diagnostic_lines(palette: Palette) -> Vec<Line<'static>> {
    discreet_stack()
        .lines()
        .map(|line| {
            if line == DISCREET_ERROR_LINE || line.trim() == "^" {
                Line::from(Span::styled(line.to_string(), palette.error()))
            } else if let Some(frame) = stack_frame_line(line, palette) {
                frame
            } else if line.trim().is_empty() {
                Line::from(" ")
            } else if line.contains("dist") && line.ends_with(":184") {
                Line::from(Span::styled(line.to_string(), palette.muted()))
            } else {
                Line::from(Span::styled(line.to_string(), palette.body()))
            }
        })
        .collect()
}

fn discreet_diagnostic_lines(
    palette: Palette,
    stack_height: u16,
    reading_height: u16,
) -> Vec<Line<'static>> {
    let lines = diagnostic_lines(palette);
    let error_index = discreet_stack()
        .lines()
        .position(|line| line == DISCREET_ERROR_LINE)
        .unwrap_or(0);
    let available_call_frame_rows =
        usize::from(stack_height).saturating_sub(error_index.saturating_add(1));

    if available_call_frame_rows < usize::from(reading_height) {
        lines.into_iter().skip(error_index).collect()
    } else {
        lines
    }
}

fn stack_frame_line(line: &str, palette: Palette) -> Option<Line<'static>> {
    let trimmed = line.trim_start();
    let frame = trimmed.strip_prefix("at ")?;
    let indent = " ".repeat(line.len().saturating_sub(trimmed.len()));
    if let Some((function, location)) = frame.rsplit_once(" (") {
        return Some(Line::from(vec![
            Span::styled(format!("{indent}at "), palette.muted()),
            Span::styled(function.to_string(), palette.body()),
            Span::styled(format!(" ({location}"), palette.muted()),
        ]));
    }
    Some(Line::from(vec![
        Span::styled(format!("{indent}at "), palette.muted()),
        Span::styled(frame.to_string(), palette.body()),
    ]))
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
    fn discreet_render_hides_hud_and_metadata() {
        let mut state = state();
        state.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        state.overlay = Overlay::Metadata;
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("TypeError"));
        assert!(screen.contains("bright readable text"));
        assert!(screen.contains("rows 1-1/1"));
        assert!(!screen.contains("Book information"));
        assert!(!screen.contains("[g]"));
        assert!(!screen.contains("F12"));
        assert_eq!(screen.matches("TypeError").count(), 1);
    }

    #[test]
    fn discreet_render_compacts_the_preamble_to_show_more_stack_frames() {
        let mut state = state();
        state.resize(80, 14);
        state.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let backend = TestBackend::new(80, 14);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();

        let screen = format!("{}", terminal.backend());
        assert_eq!(discreet_chunks(Rect::new(0, 0, 80, 14))[1].height, 4);
        for frame in [
            "resolveActiveSession",
            "renderWorkspace",
            "commitHookEffectListMount",
            "commitPassiveMountOnFiber",
        ] {
            assert!(screen.contains(frame), "missing stack frame: {frame}");
        }
        assert!(!screen.contains("node .\\dist\\server.js"));
        assert!(!screen.contains("session.js:184\n  const profile"));
    }

    #[test]
    fn discreet_log_decoration_is_stable_between_redraws() {
        let mut state = state();
        state.resize(100, 18);
        state.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let first = format!("{}", terminal.backend());
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let second = format!("{}", terminal.backend());

        assert_eq!(first, second);
        assert!(
            first.contains(" INFO  ") || first.contains(" WARN  ") || first.contains(" ERROR ")
        );
    }

    #[test]
    fn discreet_prefix_colors_only_the_level_keyword() {
        let mut state = state();
        state.resize(100, 18);
        state.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();

        let palette = Palette::terminal().discreet_mode();
        let colors = DiscreetLogColors::terminal();
        let prefix = state.discreet_log.as_ref().unwrap().prefixes(
            &state.pages[state.current_page].lines,
            crate::discreet::DiscreetPrefixMode::Full,
        )[0]
        .clone()
        .unwrap();
        let expected_level_color = match prefix.level {
            LogLevel::Info => colors.info,
            LogLevel::Warn => colors.warning,
            LogLevel::Error => colors.error,
        };
        let reading_y = discreet_chunks(Rect::new(0, 0, 100, 18))[1].y;
        let text_x = 1;
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((text_x, reading_y))
                .unwrap()
                .fg,
            palette.muted
        );
        let keyword_x = text_x + u16::try_from(prefix.leading.len()).unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .cell((keyword_x, reading_y))
            .unwrap()
            .modifier
            .contains(ratatui::style::Modifier::DIM));
        assert!(!terminal
            .backend()
            .buffer()
            .cell((16, reading_y))
            .unwrap()
            .modifier
            .contains(ratatui::style::Modifier::DIM));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((keyword_x, reading_y))
                .unwrap()
                .fg,
            expected_level_color
        );
        let trailing_x = keyword_x + u16::try_from(prefix.keyword.len()).unwrap();
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((trailing_x, reading_y))
                .unwrap()
                .fg,
            palette.muted
        );
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((16, reading_y))
                .unwrap()
                .fg,
            palette.body
        );
        assert_ne!(palette.body, expected_level_color);
    }

    #[test]
    fn discreet_render_keeps_pagination_indentation_and_blank_lines() {
        let mut state = state();
        state.document.sections[0].text = "  indented line\n\n  next paragraph".into();
        state.resize(100, 24);
        state.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &state)).unwrap();
        let y = discreet_chunks(state.viewport)[1].y;
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((16, y)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((18, y)).unwrap().symbol(), "i");
        assert_eq!(buffer.cell((18, y + 2)).unwrap().symbol(), "n");
        assert!((1..80).all(|x| buffer.cell((x, y + 1)).unwrap().symbol() == " "));
    }

    #[test]
    fn discreet_status_bar_only_shows_muted_physical_row_range() {
        let mut state = state();
        state.resize(100, 18);
        state.update(crate::reader_state::ReaderAction::ToggleDiscreet);
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();

        let discreet = Palette::terminal().discreet_mode();
        let action_y = discreet_chunks(Rect::new(0, 0, 100, 18))[2].y;
        let first_status_cell = terminal.backend().buffer().cell((1, action_y)).unwrap();
        assert_eq!(first_status_cell.fg, discreet.muted);

        let (start, end, total) = state.physical_row_range();
        let screen = format!("{}", terminal.backend());
        assert!(screen.contains(&format!("rows {start}-{end}/{total}")));
        for hidden_hint in ["[k]", "[j]", "Bookmark", "Normal", "F12"] {
            assert!(!screen.contains(hidden_hint));
        }
    }

    #[test]
    fn hidden_render_contains_only_neutral_diagnostics() {
        let mut state = state();
        state.overlay = Overlay::Search(crate::reader_state::SearchInput {
            query: "secret query".into(),
            error: None,
        });
        state.notify("secret notification", NotificationKind::Info);
        state.hidden = true;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &state)).unwrap();

        let screen = format!("{}", terminal.backend());
        assert!(screen.contains("resolveActiveSession"));
        for secret in [
            "bright readable text",
            "secret query",
            "secret notification",
            "Book information",
            "DiscreetOverlay",
            "PrivacyOverlay",
        ] {
            assert!(!screen.contains(secret), "leaked: {secret}");
        }
    }

    #[test]
    fn diagnostics_only_color_the_error_and_caret_red() {
        let palette = Palette::terminal();
        let lines = diagnostic_lines(palette);
        let error = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content == DISCREET_ERROR_LINE)
            })
            .unwrap();
        assert_eq!(error.spans[0].style.fg, Some(palette.error));
        let caret = lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content.trim() == "^"))
            .unwrap();
        assert_eq!(caret.spans[0].style.fg, Some(palette.error));
        let stack = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.contains("resolveActiveSession"))
            })
            .unwrap();
        assert!(stack
            .spans
            .iter()
            .all(|span| span.style.fg != Some(palette.error)));
    }

    #[test]
    fn discreet_reading_area_is_left_aligned() {
        let area = Rect::new(10, 5, 120, 8);
        let text_area = left_aligned_reading_text_area(area, Some(72));

        assert_eq!(text_area.x, 11);
        assert_eq!(text_area.width, 72);
        assert_ne!(text_area.x, area.x + (area.width - text_area.width) / 2);
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
