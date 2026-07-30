use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use percent_encoding::percent_decode_str;
use roxmltree::Document;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BookFormat {
    Epub,
    Txt,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentsEntry {
    pub title: String,
    pub depth: usize,
    pub target: TextPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentsSource {
    Document,
    Generated,
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
    pub contents: Vec<ContentsEntry>,
    pub contents_source: ContentsSource,
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
            BookFormat::Pdf => "PDF",
        }
    }

    pub fn section_label(self) -> &'static str {
        match self {
            BookFormat::Epub => "Chapter",
            BookFormat::Txt => "Section",
            BookFormat::Pdf => "Page",
        }
    }

    pub fn section_label_plural(self) -> &'static str {
        match self {
            BookFormat::Epub => "Chapters",
            BookFormat::Txt => "Sections",
            BookFormat::Pdf => "Pages",
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
        "pdf" => load_pdf(path),
        other => bail!("Unsupported book format {other:?}. Reader supports EPUB, TXT, and PDF."),
    }
}

pub(crate) fn is_supported_book_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "epub" | "txt" | "pdf"
    )
}

pub fn load_txt(path: &Path) -> Result<BookDocument> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let text = decode_text(&bytes);
    let title = fallback_title(path);
    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = content_book_id(&bytes);
    let sections = split_txt_sections(&text);
    let contents = generated_contents(&sections);
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
        contents,
        contents_source: ContentsSource::Generated,
    })
}

#[derive(Debug, Clone)]
struct ManifestItem {
    path: String,
    media_type: String,
    properties: Vec<String>,
}

#[derive(Debug)]
struct ExtractedXhtml {
    text: String,
    anchors: HashMap<String, usize>,
}

#[derive(Debug)]
struct EpubSpineTarget {
    section_index: usize,
    anchors: HashMap<String, usize>,
}

#[derive(Debug)]
struct RawContentsEntry {
    title: String,
    depth: usize,
    href: String,
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

    let mut manifest = HashMap::new();
    for item in opf_doc
        .descendants()
        .filter(|node| node.tag_name().name() == "item")
    {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            let Some((item_path, _)) = resolve_archive_href(&rootfile, href) else {
                continue;
            };
            manifest.insert(
                id.to_string(),
                ManifestItem {
                    path: item_path,
                    media_type: item.attribute("media-type").unwrap_or_default().to_string(),
                    properties: item
                        .attribute("properties")
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                },
            );
        }
    }

    let mut sections = Vec::new();
    let mut spine_targets = HashMap::new();
    for itemref in opf_doc
        .descendants()
        .filter(|node| node.tag_name().name() == "itemref")
    {
        let Some(idref) = itemref.attribute("idref") else {
            continue;
        };
        let Some(chapter) = manifest.get(idref) else {
            continue;
        };
        let html = read_zip_string(&mut archive, &chapter.path)
            .with_context(|| format!("Failed to read EPUB chapter {}", chapter.path))?;
        let extracted = extract_xhtml(&html);
        if !extracted.text.trim().is_empty() {
            let title = chapter_title_from_html(&html)
                .unwrap_or_else(|| format!("Chapter {}", sections.len() + 1));
            let section_index = sections.len();
            sections.push(Section {
                title,
                text: extracted.text,
            });
            spine_targets.insert(
                chapter.path.clone(),
                EpubSpineTarget {
                    section_index,
                    anchors: extracted.anchors,
                },
            );
        }
    }

    if sections.is_empty() {
        bail!("EPUB contains no readable spine content");
    }

    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = content_book_id_for_path(path)?;
    let section_count = sections.len();
    let document_contents =
        load_epub_document_contents(&mut archive, &opf_doc, &manifest, &spine_targets);
    let (contents, contents_source) = if document_contents.is_empty() {
        (generated_contents(&sections), ContentsSource::Generated)
    } else {
        (document_contents, ContentsSource::Document)
    };
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
        contents,
        contents_source,
    })
}

pub fn load_pdf(path: &Path) -> Result<BookDocument> {
    let pdf_metadata = pdf_extract::Document::load_metadata(path)
        .with_context(|| format!("Failed to read PDF metadata from {}", path.display()))?;
    let page_texts = pdf_extract::extract_text_by_pages(path)
        .with_context(|| format!("Failed to extract text from PDF {}", path.display()))?;

    if page_texts.is_empty() || page_texts.iter().all(|text| text.trim().is_empty()) {
        bail!(
            "PDF contains no extractable text: {}. Scanned or image-only PDFs require OCR, which is not supported.",
            path.display()
        );
    }

    let title = optional_inline_text(pdf_metadata.title).unwrap_or_else(|| fallback_title(path));
    let author = optional_inline_text(pdf_metadata.author);
    let sections = page_texts
        .into_iter()
        .enumerate()
        .map(|(index, text)| Section {
            title: format!("Page {}", index + 1),
            text: normalize_pdf_text(&text),
        })
        .collect::<Vec<_>>();
    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = content_book_id_for_path(path)?;
    let section_count = sections.len();
    let document_contents = pdf_extract::Document::load(path)
        .ok()
        .and_then(|document| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| document.get_toc()))
                .ok()
                .and_then(|result| result.ok())
        })
        .map(|toc| {
            toc.toc
                .into_iter()
                .filter_map(|entry| {
                    let title = clean_inline_text(&entry.title);
                    (!title.is_empty() && (1..=section_count).contains(&entry.page)).then(|| {
                        ContentsEntry {
                            title,
                            depth: entry.level.saturating_sub(1),
                            target: TextPosition::new(entry.page - 1, 0),
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let (contents, contents_source) = if document_contents.is_empty() {
        (generated_contents(&sections), ContentsSource::Generated)
    } else {
        (document_contents, ContentsSource::Document)
    };

    Ok(BookDocument {
        id,
        title: title.clone(),
        path: stable_path.clone(),
        format: BookFormat::Pdf,
        metadata: BookMetadata {
            title,
            author,
            language: None,
            publisher: None,
            format: BookFormat::Pdf,
            path: stable_path,
            section_count,
        },
        sections,
        contents,
        contents_source,
    })
}

pub(crate) fn generated_contents(sections: &[Section]) -> Vec<ContentsEntry> {
    sections
        .iter()
        .enumerate()
        .map(|(section_index, section)| ContentsEntry {
            title: section.title.clone(),
            depth: 0,
            target: TextPosition::new(section_index, 0),
        })
        .collect()
}

fn load_epub_document_contents<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    opf_doc: &Document<'_>,
    manifest: &HashMap<String, ManifestItem>,
    spine_targets: &HashMap<String, EpubSpineTarget>,
) -> Vec<ContentsEntry> {
    if let Some(nav_item) = manifest
        .values()
        .find(|item| item.properties.iter().any(|property| property == "nav"))
    {
        if let Ok(nav) = read_zip_string(archive, &nav_item.path) {
            let contents =
                resolve_epub_contents(parse_epub3_nav(&nav), &nav_item.path, spine_targets);
            if !contents.is_empty() {
                return contents;
            }
        }
    }

    let ncx_id = opf_doc
        .descendants()
        .find(|node| node.tag_name().name() == "spine")
        .and_then(|node| node.attribute("toc"));
    let ncx_item = ncx_id.and_then(|id| manifest.get(id)).or_else(|| {
        manifest
            .values()
            .find(|item| item.media_type == "application/x-dtbncx+xml")
    });
    if let Some(ncx_item) = ncx_item {
        if let Ok(ncx) = read_zip_string(archive, &ncx_item.path) {
            let contents =
                resolve_epub_contents(parse_epub2_ncx(&ncx), &ncx_item.path, spine_targets);
            if !contents.is_empty() {
                return contents;
            }
        }
    }
    Vec::new()
}

fn parse_epub3_nav(xml: &str) -> Vec<RawContentsEntry> {
    let Ok(document) = Document::parse(xml) else {
        return Vec::new();
    };
    let Some(nav) = document.descendants().find(|node| {
        node.is_element()
            && node.tag_name().name() == "nav"
            && node.attributes().any(|attribute| {
                (attribute.name() == "type"
                    && attribute
                        .value()
                        .split_whitespace()
                        .any(|value| value == "toc"))
                    || (attribute.name() == "role"
                        && attribute
                            .value()
                            .split_whitespace()
                            .any(|value| value == "doc-toc"))
            })
    }) else {
        return Vec::new();
    };
    let Some(list) = nav
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "ol")
    else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    collect_epub3_nav_entries(list, 0, &mut entries);
    entries
}

fn collect_epub3_nav_entries(
    list: roxmltree::Node<'_, '_>,
    depth: usize,
    entries: &mut Vec<RawContentsEntry>,
) {
    for item in list
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "li")
    {
        if let Some(link) = item
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "a")
        {
            if let Some(href) = link.attribute("href") {
                let title = node_inline_text(link);
                if !title.is_empty() {
                    entries.push(RawContentsEntry {
                        title,
                        depth,
                        href: href.to_string(),
                    });
                }
            }
        }
        if let Some(child_list) = item
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "ol")
        {
            collect_epub3_nav_entries(child_list, depth + 1, entries);
        }
    }
}

fn parse_epub2_ncx(xml: &str) -> Vec<RawContentsEntry> {
    let Ok(document) = Document::parse(xml) else {
        return Vec::new();
    };
    let Some(nav_map) = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "navMap")
    else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    collect_epub2_nav_entries(nav_map, 0, &mut entries);
    entries
}

fn collect_epub2_nav_entries(
    parent: roxmltree::Node<'_, '_>,
    depth: usize,
    entries: &mut Vec<RawContentsEntry>,
) {
    for nav_point in parent
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "navPoint")
    {
        let title = nav_point
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "navLabel")
            .map(node_inline_text)
            .unwrap_or_default();
        let href = nav_point
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "content")
            .and_then(|node| node.attribute("src"));
        if let Some(href) = href {
            if !title.is_empty() {
                entries.push(RawContentsEntry {
                    title,
                    depth,
                    href: href.to_string(),
                });
            }
        }
        collect_epub2_nav_entries(nav_point, depth + 1, entries);
    }
}

fn node_inline_text(node: roxmltree::Node<'_, '_>) -> String {
    clean_inline_text(
        &node
            .descendants()
            .filter(|descendant| descendant.is_text())
            .filter_map(|descendant| descendant.text())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn resolve_epub_contents(
    raw_entries: Vec<RawContentsEntry>,
    contents_path: &str,
    spine_targets: &HashMap<String, EpubSpineTarget>,
) -> Vec<ContentsEntry> {
    let mut entries = raw_entries
        .into_iter()
        .filter_map(|entry| {
            let (target_path, fragment) = resolve_archive_href(contents_path, &entry.href)?;
            let spine_target = spine_targets.get(&target_path)?;
            let char_offset = match fragment {
                Some(fragment) => *spine_target.anchors.get(&fragment)?,
                None => 0,
            };
            Some(ContentsEntry {
                title: entry.title,
                depth: entry.depth,
                target: TextPosition::new(spine_target.section_index, char_offset),
            })
        })
        .collect::<Vec<_>>();
    normalize_contents_depths(&mut entries);
    entries
}

fn normalize_contents_depths(entries: &mut [ContentsEntry]) {
    let Some(first_depth) = entries.first().map(|entry| entry.depth) else {
        return;
    };
    let mut previous_depth = 0;
    for (index, entry) in entries.iter_mut().enumerate() {
        let depth = entry.depth.saturating_sub(first_depth);
        entry.depth = if index == 0 {
            0
        } else {
            depth.min(previous_depth + 1)
        };
        previous_depth = entry.depth;
    }
}

fn resolve_archive_href(base_document: &str, href: &str) -> Option<(String, Option<String>)> {
    let (path_and_query, fragment) = href
        .split_once('#')
        .map_or((href, None), |(path, fragment)| (path, Some(fragment)));
    let encoded_path = path_and_query
        .split_once('?')
        .map_or(path_and_query, |(path, _)| path);
    if encoded_path.contains("://") || encoded_path.contains(':') {
        return None;
    }
    let decoded_path = percent_decode_str(encoded_path)
        .decode_utf8_lossy()
        .replace('\\', "/");
    if decoded_path.contains("://") || decoded_path.contains(':') {
        return None;
    }
    let mut components = Vec::new();
    if !decoded_path.starts_with('/') {
        if let Some((base, _)) = base_document.rsplit_once('/') {
            components.extend(base.split('/').filter(|part| !part.is_empty()));
        }
    }
    if decoded_path.is_empty() {
        components.clear();
        components.extend(base_document.split('/').filter(|part| !part.is_empty()));
    } else {
        for component in decoded_path.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    components.pop()?;
                }
                component => components.push(component),
            }
        }
    }
    let path = components.join("/");
    if path.is_empty() {
        return None;
    }
    let fragment = fragment
        .map(|fragment| {
            percent_decode_str(fragment)
                .decode_utf8_lossy()
                .into_owned()
        })
        .filter(|fragment| !fragment.is_empty());
    Some((path, fragment))
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

fn content_book_id(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    format!("{hash:x}")
}

fn content_book_id_for_path(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to hash {}", path.display()))?;
    Ok(content_book_id(&bytes))
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

fn extract_xhtml(html: &str) -> ExtractedXhtml {
    let mut raw = Vec::with_capacity(html.chars().count());
    let mut in_tag = false;
    let mut entity = String::new();
    let mut in_entity = false;
    let mut entity_source = 0;

    for (source, ch) in html.char_indices() {
        match ch {
            '<' => {
                in_tag = true;
                if raw.last().is_none_or(|(ch, _)| *ch != '\n') {
                    raw.push(('\n', source));
                }
            }
            '>' => in_tag = false,
            '&' if !in_tag => {
                in_entity = true;
                entity.clear();
                entity_source = source;
            }
            ';' if in_entity => {
                in_entity = false;
                raw.extend(decode_entity(&entity).chars().map(|ch| (ch, entity_source)));
            }
            _ if in_tag => {}
            _ if in_entity => entity.push(ch),
            _ => raw.push((ch, source)),
        }
    }

    let (text, source_map) = normalize_raw_xhtml_text(&raw);
    let mut anchors = HashMap::new();
    for (id, source) in html_anchor_sources(html) {
        let char_offset = source_map
            .iter()
            .position(|mapped_source| {
                mapped_source.is_some_and(|mapped_source| mapped_source >= source)
            })
            .unwrap_or(source_map.len());
        anchors.entry(id).or_insert(char_offset);
    }

    ExtractedXhtml { text, anchors }
}

fn html_anchor_sources(html: &str) -> Vec<(String, usize)> {
    let mut anchors = Vec::new();
    let mut search_start = 0;
    while search_start < html.len() {
        let Some(relative_start) = html[search_start..].find('<') else {
            break;
        };
        let tag_start = search_start + relative_start;
        let mut quote = None;
        let mut tag_end = None;
        for (relative, ch) in html[tag_start + 1..].char_indices() {
            match (quote, ch) {
                (Some(active), ch) if active == ch => quote = None,
                (None, '\'' | '"') => quote = Some(ch),
                (None, '>') => {
                    tag_end = Some(tag_start + 1 + relative);
                    break;
                }
                _ => {}
            }
        }
        let Some(tag_end) = tag_end else {
            break;
        };
        if let Some(id) = tag_id_attribute(&html[tag_start + 1..tag_end]) {
            anchors.push((id, tag_start));
        }
        search_start = tag_end + 1;
    }
    anchors
}

fn tag_id_attribute(tag: &str) -> Option<String> {
    let tag = tag.trim_start();
    if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
        return None;
    }
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() || matches!(bytes[index], b'/' | b'>') {
            break;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'=' | b'/' | b'>')
        {
            index += 1;
        }
        let name = &tag[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        let (value_start, value_end) = if matches!(bytes[index], b'\'' | b'"') {
            let quote = bytes[index];
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let value_end = index;
            index = index.saturating_add(1);
            (value_start, value_end)
        } else {
            let value_start = index;
            while index < bytes.len()
                && !bytes[index].is_ascii_whitespace()
                && !matches!(bytes[index], b'/' | b'>')
            {
                index += 1;
            }
            (value_start, index)
        };
        if name.eq_ignore_ascii_case("id") || name.eq_ignore_ascii_case("xml:id") {
            return Some(decode_attribute_entities(&tag[value_start..value_end]));
        }
    }
    None
}

fn decode_attribute_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn normalize_raw_xhtml_text(raw: &[(char, usize)]) -> (String, Vec<Option<usize>>) {
    let mut text = String::new();
    let mut source_map = Vec::new();
    let mut line = Vec::new();

    for &(ch, source) in raw {
        if ch == '\n' {
            append_clean_xhtml_line(&line, &mut text, &mut source_map);
            line.clear();
        } else {
            line.push((ch, source));
        }
    }
    append_clean_xhtml_line(&line, &mut text, &mut source_map);
    (text, source_map)
}

fn append_clean_xhtml_line(
    line: &[(char, usize)],
    text: &mut String,
    source_map: &mut Vec<Option<usize>>,
) {
    let mut words = Vec::<Vec<(char, usize)>>::new();
    let mut index = 0;
    while index < line.len() {
        while index < line.len() && line[index].0.is_whitespace() {
            index += 1;
        }
        if index == line.len() {
            break;
        }
        let mut word = Vec::new();
        while index < line.len() && !line[index].0.is_whitespace() {
            word.push(line[index]);
            index += 1;
        }
        words.push(word);
    }
    if words.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push('\n');
        source_map.push(None);
    }
    for (word_index, word) in words.iter().enumerate() {
        if word_index > 0 {
            text.push(' ');
            source_map.push(None);
        }
        for &(ch, source) in word {
            text.push(ch);
            source_map.push(Some(source));
        }
    }
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

fn optional_inline_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| clean_inline_text(&value))
        .filter(|value| !value.is_empty())
}

fn normalize_pdf_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
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
            contents: Vec::new(),
            contents_source: ContentsSource::Generated,
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
            contents: Vec::new(),
            contents_source: ContentsSource::Generated,
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
        assert_eq!(
            extract_xhtml("<h1>Hi</h1><p>A &amp; B</p>").text,
            "Hi\nA & B"
        );
    }

    #[test]
    fn tolerant_anchor_scanner_maps_empty_anchor_to_following_text() {
        let extracted = extract_xhtml("<p>Alpha</p><a xml:id='next'></a><h2>Next &unknown;</h2>");

        assert_eq!(extracted.text, "Alpha\nNext");
        assert_eq!(extracted.anchors.get("next"), Some(&6));
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
    fn content_book_id_is_path_independent_and_changes_with_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let second = nested.join("second.txt");
        std::fs::write(&first, "same bytes").unwrap();
        std::fs::write(&second, "same bytes").unwrap();

        let first_document = load_txt(&first).unwrap();
        let second_document = load_txt(&second).unwrap();

        assert_eq!(first_document.id, second_document.id);
        assert_eq!(first_document.id.len(), 64);

        std::fs::write(&second, "different bytes").unwrap();
        assert_ne!(first_document.id, load_txt(&second).unwrap().id);
    }

    #[test]
    fn load_book_rejects_unsupported_formats() {
        let err = load_book(Path::new("book.mobi")).unwrap_err();
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
            contents: Vec::new(),
            contents_source: ContentsSource::Generated,
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
