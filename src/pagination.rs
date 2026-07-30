use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::book::{ContentMetrics, Section, TextPosition};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Page {
    pub index: usize,
    pub section_index: usize,
    pub source_start: TextPosition,
    pub source_end: TextPosition,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchoredLine {
    pub text: String,
    pub start_char: usize,
    pub end_char: usize,
}

pub fn paginate(sections: &[Section], width: u16, height: u16) -> Vec<Page> {
    paginate_with_text_width(sections, width, height, None)
}

pub fn paginate_with_text_width(
    sections: &[Section],
    width: u16,
    height: u16,
    text_width: Option<u16>,
) -> Vec<Page> {
    let content_width = resolved_content_width(width, text_width);
    let content_height = usize::from(height.max(6)).saturating_sub(2).max(1);
    paginate_with_content_dimensions(sections, content_width, content_height)
}

pub fn paginate_with_content_height(
    sections: &[Section],
    width: u16,
    content_height: u16,
    text_width: Option<u16>,
) -> Vec<Page> {
    paginate_with_content_dimensions(
        sections,
        resolved_content_width(width, text_width),
        usize::from(content_height.max(1)),
    )
}

pub(crate) fn paginate_with_exact_content_dimensions(
    sections: &[Section],
    content_width: u16,
    content_height: u16,
) -> Vec<Page> {
    paginate_with_content_dimensions(
        sections,
        usize::from(content_width.max(1)),
        usize::from(content_height.max(1)),
    )
}

fn resolved_content_width(width: u16, text_width: Option<u16>) -> usize {
    let available = usize::from(width.saturating_sub(2).max(1));
    text_width
        .map(|value| usize::from(value.max(20)))
        .unwrap_or(available)
        .min(available)
}

fn paginate_with_content_dimensions(
    sections: &[Section],
    content_width: usize,
    content_height: usize,
) -> Vec<Page> {
    let mut pages = Vec::new();

    for (section_index, section) in sections.iter().enumerate() {
        let wrapped = wrap_text_anchored(&section.text, content_width);
        for chunk in wrapped.chunks(content_height.max(1)) {
            let start_char = chunk.first().map(|line| line.start_char).unwrap_or(0);
            let end_char = chunk.last().map(|line| line.end_char).unwrap_or(start_char);
            pages.push(Page {
                index: pages.len(),
                section_index,
                source_start: TextPosition::new(section_index, start_char),
                source_end: TextPosition::new(section_index, end_char),
                lines: chunk.iter().map(|line| line.text.clone()).collect(),
            });
        }
    }

    if pages.is_empty() {
        pages.push(Page {
            index: 0,
            section_index: 0,
            source_start: TextPosition::default(),
            source_end: TextPosition::default(),
            lines: vec![String::new()],
        });
    }

    pages
}

pub fn first_page_for_section(pages: &[Page], section_index: usize) -> Option<usize> {
    pages
        .iter()
        .find(|page| page.section_index == section_index)
        .map(|page| page.index)
}

pub fn page_for_position(pages: &[Page], position: TextPosition) -> usize {
    pages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, page)| {
            page.section_index == position.section_index
                && page.source_start.char_offset <= position.char_offset
        })
        .or_else(|| {
            pages
                .iter()
                .enumerate()
                .rev()
                .find(|(_, page)| page.section_index <= position.section_index)
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

pub fn page_for_tenths(pages: &[Page], metrics: &ContentMetrics, tenths: u16) -> usize {
    page_for_position(pages, metrics.position_at_tenths(tenths))
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap_text_anchored(text, width)
        .into_iter()
        .map(|line| line.text)
        .collect()
}

pub fn wrap_text_anchored(text: &str, width: usize) -> Vec<AnchoredLine> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut raw_start_char = 0;

    for raw_line in text.split('\n') {
        let raw_char_count = raw_line.chars().count();
        if raw_line.trim().is_empty() {
            lines.push(AnchoredLine {
                text: String::new(),
                start_char: raw_start_char,
                end_char: raw_start_char + raw_char_count,
            });
            raw_start_char += raw_char_count + 1;
            continue;
        }

        let mut current = String::new();
        let mut current_start = raw_start_char;
        let mut current_end = raw_start_char;
        let mut search_byte = 0;

        for word in raw_line.split_whitespace() {
            let relative = raw_line[search_byte..].find(word).unwrap_or(0);
            let word_byte_start = search_byte + relative;
            let word_start = raw_start_char + raw_line[..word_byte_start].chars().count();
            let word_end = word_start + word.chars().count();
            search_byte = word_byte_start + word.len();

            let pending_width = current.width() + usize::from(!current.is_empty()) + word.width();
            if pending_width > width && !current.is_empty() {
                push_line(&mut lines, &mut current, current_start, current_end);
            }

            if word.width() > width {
                if !current.is_empty() {
                    push_line(&mut lines, &mut current, current_start, current_end);
                }
                let remainder = split_oversized_word(word, word_start, width, &mut lines);
                current = remainder.text;
                current_start = remainder.start_char;
                current_end = remainder.end_char;
                continue;
            }

            if current.is_empty() {
                current_start = word_start;
            } else {
                current.push(' ');
            }
            current.push_str(word);
            current_end = word_end;
        }

        if !current.is_empty() {
            push_line(&mut lines, &mut current, current_start, current_end);
        }
        raw_start_char += raw_char_count + 1;
    }

    lines
}

fn push_line(lines: &mut Vec<AnchoredLine>, text: &mut String, start_char: usize, end_char: usize) {
    lines.push(AnchoredLine {
        text: std::mem::take(text),
        start_char,
        end_char,
    });
}

fn split_oversized_word(
    word: &str,
    word_start: usize,
    width: usize,
    lines: &mut Vec<AnchoredLine>,
) -> AnchoredLine {
    let mut part = String::new();
    let mut part_width = 0;
    let mut part_start = word_start;
    let mut part_end = word_start;

    for (char_index, ch) in word.chars().enumerate() {
        let char_width = ch.width().unwrap_or(0);
        if part_width + char_width > width && !part.is_empty() {
            lines.push(AnchoredLine {
                text: std::mem::take(&mut part),
                start_char: part_start,
                end_char: part_end,
            });
            part_width = 0;
            part_start = word_start + char_index;
        }
        part.push(ch);
        part_width += char_width;
        part_end = word_start + char_index + 1;
    }

    AnchoredLine {
        text: part,
        start_char: part_start,
        end_char: part_end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(text: &str) -> Section {
        Section {
            title: "A".into(),
            text: text.into(),
        }
    }

    #[test]
    fn wraps_long_lines() {
        assert_eq!(wrap_text("alpha beta gamma", 8), ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn anchors_survive_reflow() {
        let sections = [section("alpha beta gamma delta epsilon zeta")];
        let narrow = paginate_with_content_height(&sections, 20, 2, None);
        let wide = paginate_with_content_height(&sections, 40, 2, None);
        let position = narrow.last().unwrap().source_start;

        let page = page_for_position(&wide, position);

        assert!(wide[page].source_start.char_offset <= position.char_offset);
        assert!(wide[page].source_end.char_offset >= position.char_offset);
    }

    #[test]
    fn paginates_empty_input_to_one_blank_page() {
        let pages = paginate(&[], 80, 24);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].lines, vec![String::new()]);
    }

    #[test]
    fn splits_words_that_exceed_width_with_anchors() {
        let lines = wrap_text_anchored("abcdefghij", 4);
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["abcd", "efgh", "ij"]
        );
        assert_eq!((lines[2].start_char, lines[2].end_char), (8, 10));
    }

    #[test]
    fn wraps_by_terminal_display_width() {
        let lines = wrap_text("你好世界和平", 6);
        assert_eq!(lines, ["你好世", "界和平"]);
        assert!(lines.iter().all(|line| line.width() <= 6));
    }

    #[test]
    fn wraps_mixed_width_text_without_overflowing() {
        let lines = wrap_text("hello 世界 reader", 10);
        assert_eq!(lines, ["hello 世界", "reader"]);
        assert!(lines.iter().all(|line| line.width() <= 10));
    }

    #[test]
    fn preserves_empty_lines_and_their_source_offsets() {
        let lines = wrap_text_anchored("alpha\n\nbeta", 20);

        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "", "beta"]
        );
        assert_eq!((lines[1].start_char, lines[1].end_char), (6, 6));
        assert_eq!(lines[2].start_char, 7);
    }

    #[test]
    fn finds_first_page_for_section() {
        let pages = paginate(
            &[
                section("one two three four five six seven eight"),
                Section {
                    title: "B".into(),
                    text: "nine ten".into(),
                },
            ],
            12,
            8,
        );
        assert_eq!(first_page_for_section(&pages, 1), Some(1));
        assert_eq!(first_page_for_section(&pages, 99), None);
    }

    #[test]
    fn percentage_targets_book_edges() {
        let sections = [section("one two three four five six seven eight")];
        let pages = paginate_with_content_height(&sections, 20, 1, None);
        let metrics = ContentMetrics::from_sections(&sections);

        assert_eq!(page_for_tenths(&pages, &metrics, 0), 0);
        assert_eq!(page_for_tenths(&pages, &metrics, 1000), pages.len() - 1);
    }
}
