use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use roxmltree::Document;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BookFormat {
    Epub,
    Txt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextPosition {
    pub section_index: usize,
    pub char_offset: usize,
}

impl TextPosition {
    pub const fn new(section_index: usize, char_offset: usize) -> Self {
        Self {
            section_index,
            char_offset,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContentMetrics {
    pub section_char_counts: Vec<usize>,
}

impl ContentMetrics {
    pub fn from_sections(sections: &[Section]) -> Self {
        Self {
            section_char_counts: sections
                .iter()
                .map(|section| section.text.chars().count())
                .collect(),
        }
    }

    pub fn total_chars(&self) -> usize {
        self.section_char_counts.iter().sum()
    }

    pub fn clamp(&self, position: TextPosition) -> TextPosition {
        if self.section_char_counts.is_empty() {
            return TextPosition::default();
        }
        let section_index = position
            .section_index
            .min(self.section_char_counts.len().saturating_sub(1));
        TextPosition::new(
            section_index,
            position
                .char_offset
                .min(self.section_char_counts[section_index]),
        )
    }

    pub fn global_char_offset(&self, position: TextPosition) -> usize {
        let position = self.clamp(position);
        self.section_char_counts[..position.section_index]
            .iter()
            .sum::<usize>()
            .saturating_add(position.char_offset)
    }

    pub fn progress_tenths(&self, position: TextPosition) -> u16 {
        let total = self.total_chars();
        if total == 0 {
            return 0;
        }
        let offset = self.global_char_offset(position).min(total);
        (((offset as u128 * 1000) + (total as u128 / 2)) / total as u128) as u16
    }

    pub fn position_at_tenths(&self, tenths: u16) -> TextPosition {
        if self.section_char_counts.is_empty() {
            return TextPosition::default();
        }
        let tenths = tenths.min(1000);
        if tenths == 0 {
            return TextPosition::default();
        }
        let total = self.total_chars();
        if total == 0 {
            return TextPosition::new(self.section_char_counts.len().saturating_sub(1), 0);
        }

        let target = ((total as u128 * tenths as u128) + 500) / 1000;
        let target = usize::try_from(target).unwrap_or(total).min(total);
        if target == total {
            let section_index = self.section_char_counts.len() - 1;
            return TextPosition::new(section_index, self.section_char_counts[section_index]);
        }

        let mut remaining = target;
        for (section_index, &length) in self.section_char_counts.iter().enumerate() {
            if remaining < length || (remaining == 0 && length > 0) {
                return TextPosition::new(section_index, remaining);
            }
            remaining = remaining.saturating_sub(length);
        }

        let section_index = self.section_char_counts.len() - 1;
        TextPosition::new(section_index, self.section_char_counts[section_index])
    }
}

#[derive(Debug, Clone)]
pub struct BookDocument {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub format: BookFormat,
    pub metadata: BookMetadata,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookMetadata {
    pub title: String,
    pub author: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub format: BookFormat,
    pub path: PathBuf,
    pub section_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub position: TextPosition,
    pub preview: String,
}

impl BookDocument {
    pub fn content_metrics(&self) -> ContentMetrics {
        ContentMetrics::from_sections(&self.sections)
    }

    pub fn full_text(&self) -> String {
        self.sections
            .iter()
            .map(|section| section.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }

        self.sections
            .iter()
            .enumerate()
            .flat_map(|(section_index, section)| {
                let (haystack, source_chars) = lowercase_with_source_map(&section.text);
                let mut offset = 0;
                let mut results = Vec::new();
                while let Some(found) = haystack[offset..].find(&query) {
                    let byte_offset = offset + found;
                    let lowercase_char_offset = haystack[..byte_offset].chars().count();
                    let char_offset = source_chars
                        .get(lowercase_char_offset)
                        .copied()
                        .unwrap_or_else(|| section.text.chars().count());
                    let source_byte_offset = section
                        .text
                        .char_indices()
                        .nth(char_offset)
                        .map(|(byte_offset, _)| byte_offset)
                        .unwrap_or(section.text.len());
                    results.push(SearchResult {
                        position: TextPosition::new(section_index, char_offset),
                        preview: preview(&section.text, source_byte_offset),
                    });
                    offset = byte_offset + query.len();
                }
                results
            })
            .collect()
    }
}

fn lowercase_with_source_map(text: &str) -> (String, Vec<usize>) {
    let mut lowercase = String::new();
    let mut source_chars = Vec::new();
    for (source_char, ch) in text.chars().enumerate() {
        for lowercase_char in ch.to_lowercase() {
            lowercase.push(lowercase_char);
            source_chars.push(source_char);
        }
    }
    (lowercase, source_chars)
}

impl BookFormat {
    pub fn label(self) -> &'static str {
        match self {
            BookFormat::Epub => "EPUB",
            BookFormat::Txt => "TXT",
        }
    }
}

pub fn load_book(path: impl AsRef<Path>) -> Result<BookDocument> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();

    match extension.as_str() {
        "epub" => load_epub(path),
        "txt" => load_txt(path),
        other => bail!("Unsupported book format {other:?}. Reader supports EPUB and TXT."),
    }
}

pub fn load_txt(path: &Path) -> Result<BookDocument> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let text = decode_text(&bytes);
    let title = fallback_title(path);
    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = stable_book_id(&stable_path, &title);
    let sections = split_txt_sections(&text);
    let metadata = BookMetadata {
        title: title.clone(),
        author: None,
        language: None,
        publisher: None,
        format: BookFormat::Txt,
        path: stable_path.clone(),
        section_count: sections.len(),
    };

    Ok(BookDocument {
        id,
        title: title.clone(),
        path: stable_path,
        format: BookFormat::Txt,
        metadata,
        sections,
    })
}

pub fn load_epub(path: &Path) -> Result<BookDocument> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("Invalid EPUB zip container")?;
    let rootfile = epub_rootfile(&mut archive)?;
    let opf = read_zip_string(&mut archive, &rootfile)?;
    let opf_doc = Document::parse(&opf).context("Invalid EPUB package document")?;
    let title = metadata_text(&opf_doc, "title").unwrap_or_else(|| fallback_title(path));
    let author = metadata_text(&opf_doc, "creator");
    let language = metadata_text(&opf_doc, "language");
    let publisher = metadata_text(&opf_doc, "publisher");

    let base = Path::new(&rootfile)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let mut manifest = std::collections::HashMap::new();
    for item in opf_doc
        .descendants()
        .filter(|node| node.tag_name().name() == "item")
    {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            manifest.insert(id.to_string(), normalize_zip_path(base.join(href)));
        }
    }

    let mut sections = Vec::new();
    for itemref in opf_doc
        .descendants()
        .filter(|node| node.tag_name().name() == "itemref")
    {
        let Some(idref) = itemref.attribute("idref") else {
            continue;
        };
        let Some(chapter_path) = manifest.get(idref) else {
            continue;
        };
        let html = read_zip_string(&mut archive, chapter_path)
            .with_context(|| format!("Failed to read EPUB chapter {chapter_path}"))?;
        let text = html_to_text(&html);
        if !text.trim().is_empty() {
            let title = chapter_title_from_html(&html)
                .unwrap_or_else(|| format!("Chapter {}", sections.len() + 1));
            sections.push(Section { title, text });
        }
    }

    if sections.is_empty() {
        bail!("EPUB contains no readable spine content");
    }

    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = stable_book_id(&stable_path, &title);
    let section_count = sections.len();
    Ok(BookDocument {
        id,
        title: title.clone(),
        path: stable_path.clone(),
        format: BookFormat::Epub,
        metadata: BookMetadata {
            title,
            author,
            language,
            publisher,
            format: BookFormat::Epub,
            path: stable_path,
            section_count,
        },
        sections,
    })
}

fn metadata_text(document: &Document<'_>, tag: &str) -> Option<String> {
    document
        .descendants()
        .find(|node| node.tag_name().name() == tag)
        .and_then(|node| node.text())
        .map(clean_inline_text)
        .filter(|value| !value.is_empty())
}

fn epub_rootfile<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<String> {
    let container = read_zip_string(archive, "META-INF/container.xml")
        .context("EPUB is missing META-INF/container.xml")?;
    let document = Document::parse(&container).context("Invalid EPUB container.xml")?;
    document
        .descendants()
        .find(|node| node.tag_name().name() == "rootfile")
        .and_then(|node| node.attribute("full-path"))
        .map(str::to_string)
        .with_context(|| "EPUB container does not declare a rootfile")
}

fn read_zip_string<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Result<String> {
    let mut file = archive
        .by_name(path)
        .with_context(|| format!("Missing zip entry {path}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(decode_text(&bytes))
}

fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.trim_start_matches('\u{feff}').to_string(),
        Err(_) => String::from_utf8_lossy(bytes)
            .trim_start_matches('\u{feff}')
            .to_string(),
    }
}

fn split_txt_sections(text: &str) -> Vec<Section> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let chunks: Vec<String> = normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();

    if chunks.is_empty() {
        return vec![Section {
            title: "Text".to_string(),
            text: normalized,
        }];
    }

    chunks
        .into_iter()
        .enumerate()
        .map(|(index, text)| Section {
            title: format!("Section {}", index + 1),
            text,
        })
        .collect()
}

fn fallback_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled")
        .to_string()
}

pub fn author_from_txt_path(path: &Path) -> Option<String> {
    let stem = path.file_stem().and_then(|value| value.to_str())?;

    if let Some((_, tail)) = stem.rsplit_once(" - ") {
        let trimmed = tail.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(open) = stem.rfind('(') {
        let after = &stem[open + 1..];
        if let Some(close) = after.rfind(')') {
            let inside = after[..close].trim();
            if !inside.is_empty() {
                return Some(inside.to_string());
            }
        }
    }

    None
}

fn stable_book_id(path: &Path, title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(title.as_bytes());
    let hash = hasher.finalize();
    format!("{:x}", hash)[..12].to_string()
}

fn normalize_zip_path(path: PathBuf) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn chapter_title_from_html(html: &str) -> Option<String> {
    let document = Document::parse(html).ok()?;
    document
        .descendants()
        .find(|node| matches!(node.tag_name().name(), "h1" | "h2" | "h3" | "title"))
        .and_then(|node| node.text())
        .map(clean_inline_text)
        .filter(|value| !value.is_empty())
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut entity = String::new();
    let mut in_entity = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            '>' => in_tag = false,
            '&' if !in_tag => {
                in_entity = true;
                entity.clear();
            }
            ';' if in_entity => {
                in_entity = false;
                out.push_str(decode_entity(&entity));
            }
            _ if in_tag => {}
            _ if in_entity => entity.push(ch),
            _ => out.push(ch),
        }
    }

    out.lines()
        .map(clean_inline_text)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entity(entity: &str) -> &str {
    match entity {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        _ => "",
    }
}

fn clean_inline_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn preview(text: &str, byte_offset: usize) -> String {
    let byte_offset = nearest_char_boundary(text, byte_offset.min(text.len()));
    let start = text[..byte_offset]
        .char_indices()
        .rev()
        .nth(24)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let end = text[byte_offset.min(text.len())..]
        .char_indices()
        .nth(80)
        .map(|(idx, _)| byte_offset + idx)
        .unwrap_or(text.len());
    clean_inline_text(&text[start..end.min(text.len())])
}

fn nearest_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_sections_split_on_blank_lines() {
        let sections = split_txt_sections("one\n\ntwo\n\nthree");
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[1].text, "two");
    }

    #[test]
    fn search_is_case_insensitive() {
        let document = BookDocument {
            id: "id".into(),
            title: "Title".into(),
            path: PathBuf::new(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: "Title".into(),
                author: None,
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path: PathBuf::new(),
                section_count: 1,
            },
            sections: vec![Section {
                title: "A".into(),
                text: "Hello Rust reader".into(),
            }],
        };
        let results = document.search("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].position, TextPosition::new(0, 6));
    }

    #[test]
    fn search_positions_follow_source_chars_when_lowercase_expands() {
        let document = BookDocument {
            id: "id".into(),
            title: "Title".into(),
            path: PathBuf::new(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: "Title".into(),
                author: None,
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path: PathBuf::new(),
                section_count: 1,
            },
            sections: vec![Section {
                title: "A".into(),
                text: "İX".into(),
            }],
        };

        assert_eq!(document.search("x")[0].position, TextPosition::new(0, 1));
    }

    #[test]
    fn zero_percent_is_the_first_chapter_even_when_it_is_empty() {
        let metrics = ContentMetrics {
            section_char_counts: vec![0, 10],
        };

        assert_eq!(metrics.position_at_tenths(0), TextPosition::new(0, 0));
        assert_eq!(metrics.position_at_tenths(1000), TextPosition::new(1, 10));
    }

    #[test]
    fn strips_basic_html() {
        assert_eq!(html_to_text("<h1>Hi</h1><p>A &amp; B</p>"), "Hi\nA & B");
    }

    #[test]
    fn loads_txt_from_disk_with_title_and_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-book.txt");
        std::fs::write(&path, "First part\n\nSecond part").unwrap();

        let document = load_txt(&path).unwrap();

        assert_eq!(document.title, "my-book");
        assert_eq!(document.format, BookFormat::Txt);
        assert_eq!(document.metadata.title, "my-book");
        assert_eq!(document.metadata.section_count, 2);
        assert_eq!(document.sections.len(), 2);
        assert_eq!(document.sections[0].text, "First part");
        assert!(document.path.is_absolute());
    }

    #[test]
    fn load_book_rejects_unsupported_formats() {
        let err = load_book(Path::new("book.pdf")).unwrap_err();
        assert!(err.to_string().contains("Unsupported book format"));
    }

    #[test]
    fn search_ignores_blank_query() {
        let document = BookDocument {
            id: "id".into(),
            title: "Title".into(),
            path: PathBuf::new(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: "Title".into(),
                author: None,
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path: PathBuf::new(),
                section_count: 1,
            },
            sections: vec![Section {
                title: "A".into(),
                text: "Hello Rust reader".into(),
            }],
        };

        assert!(document.search("   ").is_empty());
    }

    #[test]
    fn extracts_epub_metadata_fields() {
        let opf = r#"
            <package xmlns:dc="http://purl.org/dc/elements/1.1/">
              <metadata>
                <dc:title>Example Book</dc:title>
                <dc:creator>Jane Writer</dc:creator>
                <dc:language>en</dc:language>
                <dc:publisher>Small Press</dc:publisher>
              </metadata>
            </package>
        "#;
        let document = Document::parse(opf).unwrap();

        assert_eq!(
            metadata_text(&document, "title").as_deref(),
            Some("Example Book")
        );
        assert_eq!(
            metadata_text(&document, "creator").as_deref(),
            Some("Jane Writer")
        );
        assert_eq!(metadata_text(&document, "language").as_deref(), Some("en"));
        assert_eq!(
            metadata_text(&document, "publisher").as_deref(),
            Some("Small Press")
        );
    }

    #[test]
    fn extracts_chapter_title_from_html_heading() {
        let html = r#"<html><body><h2> The Real Start </h2><p>Body</p></body></html>"#;

        assert_eq!(
            chapter_title_from_html(html).as_deref(),
            Some("The Real Start")
        );
    }

    #[test]
    fn ignores_blank_chapter_title() {
        let html = r#"<html><head><title>   </title></head><body><p>Body</p></body></html>"#;

        assert_eq!(chapter_title_from_html(html), None);
    }

    #[test]
    fn txt_author_dash_pattern_takes_last_segment() {
        let path = Path::new("A Grammar of the English - William Cobbett.txt");
        assert_eq!(
            author_from_txt_path(path).as_deref(),
            Some("William Cobbett")
        );
    }

    #[test]
    fn txt_author_dash_pattern_handles_inner_dashes() {
        let path = Path::new("Letters - Jean-Paul Sartre.txt");
        assert_eq!(
            author_from_txt_path(path).as_deref(),
            Some("Jean-Paul Sartre")
        );
    }

    #[test]
    fn txt_author_parenthetical_pattern() {
        let path = Path::new("Pride and Prejudice (Jane Austen).txt");
        assert_eq!(author_from_txt_path(path).as_deref(), Some("Jane Austen"));
    }

    #[test]
    fn txt_author_returns_none_for_plain_title() {
        let path = Path::new("Untitled.txt");
        assert_eq!(author_from_txt_path(path), None);
    }

    #[test]
    fn txt_author_dash_pattern_trims_whitespace() {
        let path = Path::new("Notes -   Ada Lovelace   .txt");
        assert_eq!(author_from_txt_path(path).as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn txt_author_empty_after_dash_is_ignored() {
        let path = Path::new("Title - .txt");
        assert_eq!(author_from_txt_path(path), None);
    }
}
