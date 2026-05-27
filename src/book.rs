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

#[derive(Debug, Clone)]
pub struct BookDocument {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub format: BookFormat,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub section_index: usize,
    pub byte_offset: usize,
    pub preview: String,
}

impl BookDocument {
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
                let haystack = section.text.to_lowercase();
                let mut offset = 0;
                let mut results = Vec::new();
                while let Some(found) = haystack[offset..].find(&query) {
                    let byte_offset = offset + found;
                    results.push(SearchResult {
                        section_index,
                        byte_offset,
                        preview: preview(&section.text, byte_offset),
                    });
                    offset = byte_offset + query.len();
                }
                results
            })
            .collect()
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
        other => bail!("Unsupported book format {other:?}. v1 supports EPUB and TXT."),
    }
}

pub fn load_txt(path: &Path) -> Result<BookDocument> {
    let bytes = std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let text = decode_text(&bytes);
    let title = fallback_title(path);
    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = stable_book_id(&stable_path, &title);
    let sections = split_txt_sections(&text);

    Ok(BookDocument {
        id,
        title,
        path: stable_path,
        format: BookFormat::Txt,
        sections,
    })
}

pub fn load_epub(path: &Path) -> Result<BookDocument> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("Invalid EPUB zip container")?;
    let rootfile = epub_rootfile(&mut archive)?;
    let opf = read_zip_string(&mut archive, &rootfile)?;
    let opf_doc = Document::parse(&opf).context("Invalid EPUB package document")?;
    let title = opf_doc
        .descendants()
        .find(|node| node.tag_name().name() == "title")
        .and_then(|node| node.text())
        .map(clean_inline_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_title(path));

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
            sections.push(Section {
                title: format!("Chapter {}", sections.len() + 1),
                text,
            });
        }
    }

    if sections.is_empty() {
        bail!("EPUB contains no readable spine content");
    }

    let stable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let id = stable_book_id(&stable_path, &title);
    Ok(BookDocument {
        id,
        title,
        path: stable_path,
        format: BookFormat::Epub,
        sections,
    })
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
            sections: vec![Section {
                title: "A".into(),
                text: "Hello Rust reader".into(),
            }],
        };
        let results = document.search("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].section_index, 0);
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
            sections: vec![Section {
                title: "A".into(),
                text: "Hello Rust reader".into(),
            }],
        };

        assert!(document.search("   ").is_empty());
    }
}
