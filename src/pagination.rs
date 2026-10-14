use unicode_linebreak::linebreaks;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::book::{ContentMetrics, Section, TextPosition};

/// UAX #14 换行算法所采用的 Unicode 版本 (major, minor, patch)
pub const UNICODE_VERSION: (u8, u8, u8) = unicode_linebreak::UNICODE_VERSION;

/// 格式化的 Unicode 版本字符串，例如 "15.0.0"
pub fn unicode_version_str() -> String {
    format!(
        "{}.{}.{}",
        UNICODE_VERSION.0, UNICODE_VERSION.1, UNICODE_VERSION.2
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Page {
    pub index: usize,
    pub section_index: usize,
    pub source_start: TextPosition,
    pub source_end: TextPosition,
    pub lines: Vec<String>,
    #[serde(default)]
    pub anchored_lines: Vec<AnchoredLine>,
    #[serde(default)]
    pub overlap_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct AnchoredLine {
    pub text: String,
    pub start_char: usize,
    pub end_char: usize,
    #[serde(default)]
    pub char_offsets: Vec<usize>,
}

impl AnchoredLine {
    pub fn new(text: String, start_char: usize, end_char: usize, char_offsets: Vec<usize>) -> Self {
        Self {
            text,
            start_char,
            end_char,
            char_offsets,
        }
    }

    /// 该行显示文本在终端中的列宽
    pub fn width(&self) -> usize {
        self.text.width()
    }

    /// 由显示字符索引映射到原文 Unicode 字符偏移
    pub fn display_to_source(&self, display_char_idx: usize) -> Option<usize> {
        self.char_offsets.get(display_char_idx).copied()
    }

    /// 给定原文字符区间 [source_start, source_end)，返回本行内对应的显示字符切片范围 [display_start, display_end)
    pub fn source_range_to_display(
        &self,
        source_start: usize,
        source_end: usize,
    ) -> Option<(usize, usize)> {
        if self.char_offsets.is_empty() || source_start >= source_end {
            return None;
        }
        let mut first = None;
        let mut last = None;
        for (i, &src_offset) in self.char_offsets.iter().enumerate() {
            if src_offset >= source_start && src_offset < source_end {
                if first.is_none() {
                    first = Some(i);
                }
                last = Some(i + 1);
            }
        }
        first.zip(last)
    }
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
    paginate_with_options(sections, width, content_height, text_width, false, false)
}

pub fn paginate_with_options(
    sections: &[Section],
    width: u16,
    content_height: u16,
    text_width: Option<u16>,
    natural_breaks: bool,
    page_overlap: bool,
) -> Vec<Page> {
    paginate_with_layout(
        sections,
        resolved_content_width(width, text_width),
        usize::from(content_height.max(1)),
        natural_breaks,
        page_overlap,
    )
}

#[cfg(test)]
fn paginate_with_exact_content_dimensions(
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

pub(crate) fn paginate_discreet(
    sections: &[Section],
    content_width: u16,
    content_height: u16,
) -> Vec<Page> {
    paginate_with_layout(
        sections,
        usize::from(content_width.max(1)),
        usize::from(content_height.max(1)),
        true,
        false,
    )
}

pub fn resolved_content_width(width: u16, text_width: Option<u16>) -> usize {
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
    paginate_with_layout(sections, content_width, content_height, false, false)
}

fn paginate_with_layout(
    sections: &[Section],
    content_width: usize,
    content_height: usize,
    natural_breaks: bool,
    page_overlap: bool,
) -> Vec<Page> {
    let mut pages = Vec::new();
    let effective_overlap = page_overlap && content_height >= 4;

    for (section_index, section) in sections.iter().enumerate() {
        let wrapped = wrap_text_with_layout(&section.text, content_width, natural_breaks);
        if wrapped.is_empty() {
            continue;
        }

        let mut start_idx = 0;
        let mut is_first_page = true;

        while start_idx < wrapped.len() {
            let (chunk, overlap_lines, new_start_char) = if is_first_page || !effective_overlap {
                let end_idx = (start_idx + content_height).min(wrapped.len());
                let slice = &wrapped[start_idx..end_idx];
                let first_char = slice.first().map(|line| line.start_char).unwrap_or(0);
                start_idx = end_idx;
                is_first_page = false;
                (slice.to_vec(), 0, first_char)
            } else {
                let prev_line = &wrapped[start_idx - 1];
                let new_content_count = content_height.saturating_sub(1);
                let end_idx = (start_idx + new_content_count).min(wrapped.len());
                let new_slice = &wrapped[start_idx..end_idx];
                let first_new_char = new_slice
                    .first()
                    .map(|line| line.start_char)
                    .unwrap_or(prev_line.end_char);

                let mut combined = Vec::with_capacity(1 + new_slice.len());
                combined.push(prev_line.clone());
                combined.extend_from_slice(new_slice);

                start_idx = end_idx;
                (combined, 1, first_new_char)
            };

            let end_char = chunk
                .last()
                .map(|line| line.end_char)
                .unwrap_or(new_start_char);
            pages.push(Page {
                index: pages.len(),
                section_index,
                source_start: TextPosition::new(section_index, new_start_char),
                source_end: TextPosition::new(section_index, end_char),
                lines: chunk.iter().map(|line| line.text.clone()).collect(),
                anchored_lines: chunk,
                overlap_lines,
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
            anchored_lines: vec![AnchoredLine::default()],
            overlap_lines: 0,
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
    wrap_text_with_layout(text, width, false)
}

fn wrap_text_with_layout(text: &str, width: usize, natural_breaks: bool) -> Vec<AnchoredLine> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut raw_start_char = 0;

    for raw_line in text.split('\n') {
        let raw_char_count = raw_line.chars().count();
        if raw_line.is_empty() {
            lines.push(AnchoredLine {
                text: String::new(),
                start_char: raw_start_char,
                end_char: raw_start_char,
                char_offsets: Vec::new(),
            });
            raw_start_char += 1;
            continue;
        }

        if raw_line.trim().is_empty() {
            lines.push(AnchoredLine {
                text: String::new(),
                start_char: raw_start_char,
                end_char: raw_start_char + raw_char_count,
                char_offsets: Vec::new(),
            });
            raw_start_char += raw_char_count + 1;
            continue;
        }

        let char_positions: Vec<(usize, char)> = raw_line.char_indices().collect();
        let mut prev_byte = 0;
        let mut char_pos_idx = 0;

        struct Segment<'a> {
            text: &'a str,
            char_offsets: Vec<usize>,
            width: usize,
            start_char: usize,
            end_char: usize,
        }

        let mut segments = Vec::new();
        for (break_byte, _) in linebreaks(raw_line) {
            if break_byte <= prev_byte {
                continue;
            }
            let seg_str = &raw_line[prev_byte..break_byte];
            let start_char_in_line = char_pos_idx;
            let mut seg_offsets = Vec::new();
            while char_pos_idx < char_positions.len() && char_positions[char_pos_idx].0 < break_byte
            {
                seg_offsets.push(raw_start_char + char_pos_idx);
                char_pos_idx += 1;
            }
            let end_char_in_line = char_pos_idx;
            let seg_start_char = raw_start_char + start_char_in_line;
            let seg_end_char = raw_start_char + end_char_in_line;

            segments.push(Segment {
                text: seg_str,
                char_offsets: seg_offsets,
                width: seg_str.width(),
                start_char: seg_start_char,
                end_char: seg_end_char,
            });
            prev_byte = break_byte;
        }

        let mut current_text = String::new();
        let mut current_offsets: Vec<usize> = Vec::new();
        let mut current_width = 0;
        let mut current_start_char = 0;
        let mut current_end_char = 0;

        for seg in segments {
            let seg_trimmed = seg.text.trim_end_matches(' ');
            let seg_trimmed_width = seg_trimmed.width();

            // 如果连带空格都能完整放入当前行
            if current_width + seg.width <= width {
                if current_text.is_empty() {
                    current_start_char = seg.start_char;
                }
                current_text.push_str(seg.text);
                current_offsets.extend(&seg.char_offsets);
                current_width += seg.width;
                current_end_char = seg.end_char;
                // Only prefer legal UAX #14 boundaries near the right edge.
                // Small windows keep the greedy layout to avoid very short lines.
                if natural_breaks
                    && width >= 24
                    && current_width >= width - width / 8
                    && ends_at_pause(&current_text)
                {
                    push_anchored_line(
                        &mut lines,
                        &mut current_text,
                        &mut current_offsets,
                        &mut current_width,
                        current_start_char,
                        current_end_char,
                    );
                }
                continue;
            }

            // 如果去掉行末空格后恰好能放入当前行，则在行末悬挂折行
            if current_width + seg_trimmed_width <= width {
                if current_text.is_empty() {
                    current_start_char = seg.start_char;
                }
                current_text.push_str(seg.text);
                current_offsets.extend(&seg.char_offsets);
                current_width += seg.width;
                current_end_char = seg.end_char;
                push_anchored_line(
                    &mut lines,
                    &mut current_text,
                    &mut current_offsets,
                    &mut current_width,
                    current_start_char,
                    current_end_char,
                );
                continue;
            }

            // 当前行放不下，如果当前行已有内容，先提交当前行
            if !current_text.is_empty() {
                push_anchored_line(
                    &mut lines,
                    &mut current_text,
                    &mut current_offsets,
                    &mut current_width,
                    current_start_char,
                    current_end_char,
                );
            }

            // 新行检查：整段（含空格）放得下
            if seg.width <= width {
                current_start_char = seg.start_char;
                current_text.push_str(seg.text);
                current_offsets.extend(&seg.char_offsets);
                current_width = seg.width;
                current_end_char = seg.end_char;
                continue;
            }

            // 新行检查：去掉末尾空格后放得下
            if seg_trimmed_width <= width {
                current_start_char = seg.start_char;
                current_text.push_str(seg.text);
                current_offsets.extend(&seg.char_offsets);
                current_width = seg.width;
                current_end_char = seg.end_char;
                push_anchored_line(
                    &mut lines,
                    &mut current_text,
                    &mut current_offsets,
                    &mut current_width,
                    current_start_char,
                    current_end_char,
                );
                continue;
            }

            // seg.width > width: 应急折行，按字形簇（Extended Grapheme Cluster）拆分
            let mut g_char_cursor = 0;
            for g_str in seg.text.graphemes(true) {
                let g_chars_count = g_str.chars().count();
                let g_offsets = &seg.char_offsets[g_char_cursor..g_char_cursor + g_chars_count];
                g_char_cursor += g_chars_count;
                let g_width = g_str.width();
                let g_start = g_offsets.first().copied().unwrap_or(seg.start_char);
                let g_end = g_offsets.last().map(|&x| x + 1).unwrap_or(g_start);

                if current_width + g_width <= width {
                    if current_text.is_empty() {
                        current_start_char = g_start;
                    }
                    current_text.push_str(g_str);
                    current_offsets.extend_from_slice(g_offsets);
                    current_width += g_width;
                    current_end_char = g_end;
                } else {
                    if !current_text.is_empty() {
                        push_anchored_line(
                            &mut lines,
                            &mut current_text,
                            &mut current_offsets,
                            &mut current_width,
                            current_start_char,
                            current_end_char,
                        );
                    }
                    current_start_char = g_start;
                    current_text.push_str(g_str);
                    current_offsets.extend_from_slice(g_offsets);
                    current_width = g_width;
                    current_end_char = g_end;

                    if g_width >= width {
                        push_anchored_line(
                            &mut lines,
                            &mut current_text,
                            &mut current_offsets,
                            &mut current_width,
                            current_start_char,
                            current_end_char,
                        );
                    }
                }
            }
        }

        if !current_text.is_empty() {
            push_anchored_line(
                &mut lines,
                &mut current_text,
                &mut current_offsets,
                &mut current_width,
                current_start_char,
                current_end_char,
            );
        }

        raw_start_char += raw_char_count + 1;
    }

    lines
}

fn ends_at_pause(text: &str) -> bool {
    let ending = text
        .trim_end()
        .trim_end_matches(['”', '’', '」', '』', '）', '"', '\'']);
    ending.ends_with(['，', '。', '；', '：', '！', '？', '!', '?', ';'])
}

fn push_anchored_line(
    lines: &mut Vec<AnchoredLine>,
    text: &mut String,
    char_offsets: &mut Vec<usize>,
    width: &mut usize,
    start_char: usize,
    end_char: usize,
) {
    if text.is_empty() && char_offsets.is_empty() {
        return;
    }
    let trimmed_len = text.trim_end_matches(' ').len();
    let final_text = if trimmed_len > 0 && trimmed_len < text.len() {
        let chars_to_keep = text[..trimmed_len].chars().count();
        char_offsets.truncate(chars_to_keep);
        text[..trimmed_len].to_string()
    } else {
        std::mem::take(text)
    };
    text.clear();
    let offsets = std::mem::take(char_offsets);
    *width = 0;

    lines.push(AnchoredLine {
        text: final_text,
        start_char,
        end_char,
        char_offsets: offsets,
    });
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

    #[test]
    fn multiple_short_paragraphs_share_single_screen_within_section() {
        let text = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let sections = [Section {
            title: "Chapter 1".into(),
            text: text.into(),
        }];
        // width 40, content_height 10: 3个短段落应该在同一个 Page 内（Page 0）
        let pages = paginate_with_exact_content_dimensions(&sections, 40, 10);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].lines.iter().any(|l| l.contains("Paragraph one")));
        assert!(pages[0].lines.iter().any(|l| l.contains("Paragraph two")));
        assert!(pages[0].lines.iter().any(|l| l.contains("Paragraph three")));
    }

    #[test]
    fn chapter_boundary_starts_new_page() {
        let sections = [
            Section {
                title: "Chapter 1".into(),
                text: "Short text in chapter 1".into(),
            },
            Section {
                title: "Chapter 2".into(),
                text: "Short text in chapter 2".into(),
            },
        ];
        // 即使高度是 20，每一章都必须从新页开启
        let pages = paginate_with_exact_content_dimensions(&sections, 40, 20);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].section_index, 0);
        assert_eq!(pages[1].section_index, 1);
    }

    #[test]
    fn long_paragraph_across_screens_has_no_omission_or_duplicate() {
        let long_text = (0..20)
            .map(|i| format!("Line {i} of content."))
            .collect::<Vec<_>>()
            .join("\n");
        let sections = [Section {
            title: "Long Chapter".into(),
            text: long_text,
        }];
        // 每页 5 行
        let pages = paginate_with_exact_content_dimensions(&sections, 40, 5);
        assert_eq!(pages.len(), 4);
        let mut reconstructed_lines = Vec::new();
        for page in &pages {
            reconstructed_lines.extend(page.lines.clone());
        }
        for i in 0..20 {
            let needle = format!("Line {i} of content.");
            assert_eq!(
                reconstructed_lines.iter().filter(|l| l == &&needle).count(),
                1
            );
        }
    }

    #[test]
    fn cjk_punctuation_avoids_line_start_and_line_end() {
        // 常规宽度下（能容纳最少 3 个全角字符 w >= 6），避头标点（逗号、句号、叹号、问号、右括号、右书名号）绝不出现在行首
        let text = "你好，世界！真的吗？这是（测试）与《书名》。";
        for w in 6..=40 {
            let lines = wrap_text(text, w);
            for line in &lines {
                assert!(
                    line.width() <= w,
                    "Line '{}' width {} exceeds max {}",
                    line,
                    line.width(),
                    w
                );
                let first_char = line.chars().next().unwrap_or(' ');
                assert!(
                    !['，', '。', '！', '？', '）', '》', '、'].contains(&first_char),
                    "Line '{}' starts with prohibited head punctuation in width {}",
                    line,
                    w
                );
            }
        }

        // 避尾标点：开括号、开书名号不单独留在行末；闭书名号不单独留在行首
        let text_tail = "书名《红楼梦》";
        let lines_tail = wrap_text(text_tail, 4);
        assert_eq!(lines_tail, ["书名", "《红", "楼", "梦》"]);
        for line in &lines_tail {
            let last_char = line.chars().last().unwrap_or(' ');
            assert_ne!(
                last_char, '《',
                "Opening mark shouldn't end line if content wraps"
            );
            let first_char = line.chars().next().unwrap_or(' ');
            assert_ne!(
                first_char, '》',
                "Closing mark shouldn't start line if content wraps"
            );
        }
    }

    #[test]
    fn grapheme_clusters_and_composite_emoji_not_split() {
        // 组合字符：e + combining acute accent
        let accented = "Caf\u{0065}\u{0301} bar";
        let lines = wrap_text_anchored(accented, 4);
        // "Cafe\u{0301}" 作为一个词，宽度为 4，不能拆断组合音标
        assert!(lines.iter().any(|l| l.text.contains("Caf\u{0065}\u{0301}")));

        // 复合 Emoji 序列（家庭 Emoji）与国旗
        let emoji_text = "嗨👨‍👩‍👧‍👦中国🇨🇳";
        let emoji_lines = wrap_text_anchored(emoji_text, 4);
        assert!(emoji_lines.iter().all(|l| l.width() <= 4));
        // 验证复合 emoji 完整保留在同一行中，未被拆碎
        assert!(emoji_lines.iter().any(|l| l.text.contains("👨‍👩‍👧‍👦")));
        assert!(emoji_lines.iter().any(|l| l.text.contains("🇨🇳")));
    }

    #[test]
    fn extremely_narrow_window_does_not_hang_or_panic() {
        let text = "Supercalifragilisticexpialidocious 你好 👨‍👩‍👧‍👦 https://example.org/test";
        for narrow_width in [1, 2, 3, 5] {
            let lines = wrap_text_anchored(text, narrow_width);
            assert!(!lines.is_empty());
            for line in &lines {
                assert_eq!(line.text.chars().count(), line.char_offsets.len());
            }
        }
    }

    #[test]
    fn bidirectional_char_mapping_accurate() {
        let text = "Hello 世界！ Welcome to 阅读器。";
        let lines = wrap_text_anchored(text, 12);
        let raw_chars: Vec<char> = text.chars().collect();

        for line in &lines {
            assert_eq!(line.text.chars().count(), line.char_offsets.len());
            // 每一个显示字符都能准确在原文中映射到同一个字符
            for (display_idx, &source_offset) in line.char_offsets.iter().enumerate() {
                let display_char = line.text.chars().nth(display_idx).unwrap();
                let source_char = raw_chars[source_offset];
                assert_eq!(display_char, source_char);
                assert_eq!(line.display_to_source(display_idx), Some(source_offset));
            }
        }

        // 测试区间映射 source_range_to_display
        // 查找 "世界" 的原文偏移
        let world_start = text.chars().position(|c| c == '世').unwrap();
        let world_end = world_start + 2;
        let mut matched = false;
        for line in &lines {
            if let Some((disp_start, disp_end)) =
                line.source_range_to_display(world_start, world_end)
            {
                let slice: String = line
                    .text
                    .chars()
                    .skip(disp_start)
                    .take(disp_end - disp_start)
                    .collect();
                assert_eq!(slice, "世界");
                matched = true;
                break;
            }
        }
        assert!(
            matched,
            "Should find '世界' display range in one of the lines"
        );
    }

    #[test]
    fn discreet_breaks_at_a_nearby_pause_and_preserves_source_mapping() {
        let text = "甲".repeat(20) + "，乙丙丁戊己庚辛壬癸。";
        let lines = wrap_text_with_layout(&text, 48, true);
        assert!(lines[0].text.ends_with('，'));
        assert_eq!(lines[0].width(), 42);
        assert!(!wrap_text_anchored(&text, 48)[0].text.ends_with('，'));
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
            text
        );
        let source: Vec<char> = text.chars().collect();
        for line in &lines {
            assert!(line.width() <= 48);
            for (display, ch) in line.text.chars().enumerate() {
                assert_eq!(source[line.display_to_source(display).unwrap()], ch);
            }
        }
    }

    #[test]
    fn discreet_keeps_early_pauses_dense_and_narrow_windows_greedy() {
        let text = "雨停了。".to_owned() + &"甲".repeat(35);
        assert_eq!(
            wrap_text_with_layout(&text, 48, true),
            wrap_text_anchored(&text, 48)
        );
        for width in 1..24 {
            let mixed = "你好，e\u{301} 👨‍👩‍👧‍👦 world！再见。";
            assert_eq!(
                wrap_text_with_layout(mixed, width, true),
                wrap_text_anchored(mixed, width)
            );
        }
    }

    #[test]
    fn discreet_pagination_preserves_all_anchors_across_pages() {
        let text = ("甲".repeat(20) + "，乙丙丁戊己庚辛壬癸。\n\n").repeat(5);
        let pages = paginate_discreet(
            &[Section {
                title: "chapter".into(),
                text: text.clone(),
            }],
            48,
            2,
        );
        let expected = wrap_text_with_layout(&text, 48, true);
        assert_eq!(
            pages
                .iter()
                .flat_map(|p| p.anchored_lines.clone())
                .collect::<Vec<_>>(),
            expected
        );
        for page in &pages {
            assert_eq!(page_for_position(&pages, page.source_start), page.index);
        }
    }

    #[test]
    fn page_overlap_keeps_last_line_as_context_and_preserves_monotonic_anchors() {
        let sections = vec![
            Section {
                title: "Chapter 1".into(),
                text: "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7".into(),
            },
            Section {
                title: "Chapter 2".into(),
                text: "Chap2 Line 1\nChap2 Line 2\nChap2 Line 3\nChap2 Line 4".into(),
            },
        ];

        // 窗口高度为 4 时开启重叠：每页最多 4 行
        // Chapter 1:
        // Page 0: Line 1, Line 2, Line 3, Line 4 (4 行新内容, overlap=0)
        // Page 1: Line 4 (重叠), Line 5, Line 6, Line 7 (1行重叠 + 3行新内容, overlap=1)
        // Chapter 2:
        // Page 2: Chap2 Line 1, Chap2 Line 2, Chap2 Line 3, Chap2 Line 4 (新章节首页, overlap=0)
        let pages = paginate_with_options(&sections, 40, 4, None, false, true);
        assert_eq!(pages.len(), 3);

        // Page 0 检查
        assert_eq!(pages[0].section_index, 0);
        assert_eq!(pages[0].overlap_lines, 0);
        assert_eq!(pages[0].lines, vec!["Line 1", "Line 2", "Line 3", "Line 4"]);
        assert_eq!(pages[0].source_start, TextPosition::new(0, 0));

        // Page 1 检查 (重叠行是 Line 4)
        assert_eq!(pages[1].section_index, 0);
        assert_eq!(pages[1].overlap_lines, 1);
        assert_eq!(pages[1].lines, vec!["Line 4", "Line 5", "Line 6", "Line 7"]);
        // source_start 必须锚定在 Line 5 的起始字符（新内容首行）
        let line5_start = "Line 1\nLine 2\nLine 3\nLine 4\n".chars().count();
        assert_eq!(pages[1].source_start, TextPosition::new(0, line5_start));
        // 进度单调递增
        assert!(pages[1].source_start.char_offset > pages[0].source_start.char_offset);

        // Page 2 检查 (跨章节首页不重叠)
        assert_eq!(pages[2].section_index, 1);
        assert_eq!(pages[2].overlap_lines, 0);
        assert_eq!(pages[2].lines[0], "Chap2 Line 1");
        assert_eq!(pages[2].source_start, TextPosition::new(1, 0));

        // 页面定位 page_for_position 回归验证
        for page in &pages {
            assert_eq!(page_for_position(&pages, page.source_start), page.index);
        }

        // 极矮窗口降级：height < 4 时自动关闭重叠
        let dwarf_pages = paginate_with_options(&sections, 40, 3, None, false, true);
        assert!(dwarf_pages.iter().all(|p| p.overlap_lines == 0));

        // 手动关闭重叠时：所有页 overlap_lines 均为 0
        let disabled_pages = paginate_with_options(&sections, 40, 4, None, false, false);
        assert!(disabled_pages.iter().all(|p| p.overlap_lines == 0));
        assert_eq!(disabled_pages[1].lines, vec!["Line 5", "Line 6", "Line 7"]);
    }

    #[test]
    fn records_unicode_version() {
        assert_eq!(UNICODE_VERSION.0, 15);
        assert!(!unicode_version_str().is_empty());
    }
}
