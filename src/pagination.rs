#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Page {
    pub index: usize,
    pub section_index: usize,
    pub start_line: usize,
    pub lines: Vec<String>,
}

pub fn paginate(sections: &[crate::book::Section], width: u16, height: u16) -> Vec<Page> {
    paginate_with_text_width(sections, width, height, None)
}

pub fn paginate_with_text_width(
    sections: &[crate::book::Section],
    width: u16,
    height: u16,
    text_width: Option<u16>,
) -> Vec<Page> {
    let content_width = usize::from(width.max(20)).saturating_sub(2).max(20);
    let content_width = text_width
        .map(|value| usize::from(value.max(20)))
        .unwrap_or(content_width)
        .min(content_width);
    let content_height = usize::from(height.max(8)).saturating_sub(4).max(4);
    let mut pages = Vec::new();

    for (section_index, section) in sections.iter().enumerate() {
        let wrapped = wrap_text(&section.text, content_width);
        for (chunk_index, chunk) in wrapped.chunks(content_height).enumerate() {
            pages.push(Page {
                index: pages.len(),
                section_index,
                start_line: chunk_index * content_height,
                lines: chunk.to_vec(),
            });
        }
    }

    if pages.is_empty() {
        pages.push(Page {
            index: 0,
            section_index: 0,
            start_line: 0,
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

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        if raw_line.trim().is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let pending_len =
                current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
            if pending_len > width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }

            if word.chars().count() > width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                let mut part = String::new();
                for ch in word.chars() {
                    if part.chars().count() == width {
                        lines.push(part);
                        part = String::new();
                    }
                    part.push(ch);
                }
                current = part;
                continue;
            }

            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }

        if !current.is_empty() {
            lines.push(current);
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Section;

    #[test]
    fn wraps_long_lines() {
        let lines = wrap_text("alpha beta gamma", 8);
        assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn paginates_sections() {
        let pages = paginate(
            &[Section {
                title: "A".into(),
                text: "one two three four five six seven eight".into(),
            }],
            12,
            8,
        );
        assert!(!pages.is_empty());
    }

    #[test]
    fn paginates_empty_input_to_one_blank_page() {
        let pages = paginate(&[], 80, 24);

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].index, 0);
        assert_eq!(pages[0].lines, vec![String::new()]);
    }

    #[test]
    fn splits_words_that_exceed_width() {
        let lines = wrap_text("abcdefghij", 4);

        assert_eq!(lines, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn finds_first_page_for_section() {
        let pages = paginate(
            &[
                Section {
                    title: "A".into(),
                    text: "one two three four five six seven eight".into(),
                },
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
    fn optional_text_width_caps_wrapping_width() {
        let pages = paginate_with_text_width(
            &[Section {
                title: "A".into(),
                text: "alpha beta gamma delta".into(),
            }],
            80,
            24,
            Some(20),
        );

        assert_eq!(pages[0].lines, vec!["alpha beta gamma", "delta"]);
    }
}
