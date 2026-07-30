use std::path::Path;

use pdf_extract::content::{Content, Operation};
use pdf_extract::{dictionary, Document as PdfDocument, Object as PdfObject, Stream};
use reader::book::{load_book, BookFormat, ContentsSource};
use reader::bookshelf;
use reader::storage::{Progress, Store};
use reader::TextPosition;

#[derive(Clone, Copy)]
enum OutlineFixture {
    None,
    Valid,
    Malformed,
}

fn write_pdf(
    path: &Path,
    page_texts: &[Option<&[u8]>],
    title: Option<&str>,
    author: Option<&str>,
    outline: OutlineFixture,
) {
    let mut document = PdfDocument::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });

    let page_ids = page_texts
        .iter()
        .map(|page_text| {
            let operations = page_text
                .map(|text| {
                    vec![
                        Operation::new("BT", vec![]),
                        Operation::new("Tf", vec!["F1".into(), 12.into()]),
                        Operation::new("Td", vec![72.into(), 720.into()]),
                        Operation::new(
                            "Tj",
                            vec![PdfObject::String(
                                text.to_vec(),
                                pdf_extract::StringFormat::Literal,
                            )],
                        ),
                        Operation::new("ET", vec![]),
                    ]
                })
                .unwrap_or_default();
            let content = Content { operations };
            let content_id = document.add_object(Stream::new(
                dictionary! {},
                content.encode().expect("test PDF content should encode"),
            ));
            document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            })
        })
        .collect::<Vec<_>>();

    document.objects.insert(
        pages_id,
        PdfObject::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids
                .iter()
                .copied()
                .map(PdfObject::from)
                .collect::<Vec<_>>(),
            "Count" => page_ids.len() as i64,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let mut catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    match outline {
        OutlineFixture::None => {}
        OutlineFixture::Valid if page_ids.len() >= 3 => {
            let outlines_id = document.new_object_id();
            let chapter_id = document.new_object_id();
            let topic_id = document.new_object_id();
            document.objects.insert(
                outlines_id,
                PdfObject::Dictionary(dictionary! {
                    "Type" => "Outlines",
                    "First" => chapter_id,
                    "Last" => chapter_id,
                    "Count" => 2,
                }),
            );
            document.objects.insert(
                chapter_id,
                PdfObject::Dictionary(dictionary! {
                    "Title" => PdfObject::string_literal("Chapter 1"),
                    "Parent" => outlines_id,
                    "Dest" => vec![page_ids[0].into(), PdfObject::Name(b"Fit".to_vec())],
                    "First" => topic_id,
                    "Last" => topic_id,
                    "Count" => 1,
                }),
            );
            document.objects.insert(
                topic_id,
                PdfObject::Dictionary(dictionary! {
                    "Title" => PdfObject::string_literal("Named topic"),
                    "Parent" => chapter_id,
                    "A" => dictionary! {
                        "S" => "GoTo",
                        "D" => PdfObject::string_literal("named-topic"),
                    },
                }),
            );
            let destinations_id = document.add_object(dictionary! {
                "Names" => vec![
                    PdfObject::string_literal("named-topic"),
                    PdfObject::Dictionary(dictionary! {
                        "D" => vec![page_ids[2].into(), PdfObject::Name(b"Fit".to_vec())],
                    }),
                ],
            });
            catalog.set("Outlines", outlines_id);
            catalog.set("Dests", destinations_id);
        }
        OutlineFixture::Valid => {}
        OutlineFixture::Malformed => {
            let outlines_id = document.new_object_id();
            let broken_id = document.new_object_id();
            document.objects.insert(
                outlines_id,
                PdfObject::Dictionary(dictionary! {
                    "Type" => "Outlines",
                    "First" => broken_id,
                    "Last" => broken_id,
                    "Count" => 1,
                }),
            );
            document.objects.insert(
                broken_id,
                PdfObject::Dictionary(dictionary! {
                    "Title" => PdfObject::string_literal("Broken destination"),
                    "Parent" => outlines_id,
                    "Dest" => Vec::<PdfObject>::new(),
                }),
            );
            catalog.set("Outlines", outlines_id);
        }
    }
    let catalog_id = document.add_object(catalog);
    document.trailer.set("Root", catalog_id);

    if title.is_some() || author.is_some() {
        let mut info = pdf_extract::Dictionary::new();
        if let Some(title) = title {
            info.set("Title", PdfObject::string_literal(title));
        }
        if let Some(author) = author {
            info.set("Author", PdfObject::string_literal(author));
        }
        let info_id = document.add_object(info);
        document.trailer.set("Info", info_id);
    }

    document.save(path).expect("test PDF should save");
}

#[test]
fn pdf_pages_metadata_search_and_reading_state_survive_reload() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("fixture.pdf");
    let store_path = dir.path().join("library.json");
    write_pdf(
        &book_path,
        &[
            Some(b"First PDF page"),
            None,
            Some(b"Caf\xe9 reader needle"),
        ],
        Some("Fixture PDF"),
        Some("Test Author"),
        OutlineFixture::None,
    );

    let document = load_book(&book_path).unwrap();

    assert_eq!(document.format, BookFormat::Pdf);
    assert_eq!(document.title, "Fixture PDF");
    assert_eq!(document.metadata.author.as_deref(), Some("Test Author"));
    assert_eq!(document.metadata.section_count, 3);
    assert_eq!(document.sections[0].title, "Page 1");
    assert!(document.sections[0].text.contains("First PDF page"));
    assert_eq!(document.sections[1].title, "Page 2");
    assert!(document.sections[1].text.is_empty());
    assert!(document.sections[2].text.contains("Café reader needle"));
    assert_eq!(document.contents_source, ContentsSource::Generated);
    assert_eq!(document.contents.len(), 3);
    assert_eq!(document.contents[2].target, TextPosition::new(2, 0));
    let result = document.search("needle");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].position.section_index, 2);

    let mut store = Store::open(store_path.clone()).unwrap();
    let book_id = store.add_book(&document, &book_path).unwrap().id.clone();
    let progress = Progress::new(TextPosition::new(2, 5));
    store.set_progress(&book_id, progress);
    store
        .add_bookmark(&book_id, "Page 3 · 80.0%".to_string(), progress)
        .unwrap();
    store.save().unwrap();

    let loaded = Store::open(store_path).unwrap();
    let loaded_book = loaded.resolve_book("Fixture PDF").unwrap();
    assert_eq!(loaded_book.format, BookFormat::Pdf);
    assert_eq!(loaded_book.progress, progress);
    assert_eq!(
        loaded.bookmarks_for(&book_id)[0].position,
        progress.position
    );
}

#[test]
fn pdf_outline_becomes_hierarchical_contents_with_page_targets() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("outline.pdf");
    write_pdf(
        &book_path,
        &[
            Some(b"First outline page"),
            Some(b"Second outline page"),
            Some(b"Third outline page"),
        ],
        Some("Outline PDF"),
        None,
        OutlineFixture::Valid,
    );

    let document = load_book(&book_path).unwrap();

    assert_eq!(document.contents_source, ContentsSource::Document);
    assert_eq!(document.contents.len(), 2);
    assert_eq!(document.contents[0].title, "Chapter 1");
    assert_eq!(document.contents[0].depth, 0);
    assert_eq!(document.contents[0].target, TextPosition::new(0, 0));
    assert_eq!(document.contents[1].title, "Named topic");
    assert_eq!(document.contents[1].depth, 1);
    assert_eq!(document.contents[1].target, TextPosition::new(2, 0));
}

#[test]
fn malformed_pdf_outline_falls_back_without_blocking_text() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("malformed-outline.pdf");
    write_pdf(
        &book_path,
        &[Some(b"Readable text")],
        Some("Malformed Outline"),
        None,
        OutlineFixture::Malformed,
    );

    let document = load_book(&book_path).unwrap();

    assert_eq!(document.sections.len(), 1);
    assert!(document.sections[0].text.contains("Readable text"));
    assert_eq!(document.contents_source, ContentsSource::Generated);
    assert_eq!(document.contents[0].title, "Page 1");
}

#[test]
fn pdf_without_extractable_text_reports_ocr_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("scan.pdf");
    write_pdf(
        &book_path,
        &[None, None],
        Some("Scan"),
        None,
        OutlineFixture::None,
    );

    let error = load_book(&book_path).unwrap_err().to_string();

    assert!(error.contains("no extractable text"));
    assert!(error.contains("OCR"));
    assert!(error.contains("scan.pdf"));
}

#[test]
fn malformed_pdf_error_includes_path_and_parser_reason() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("broken.pdf");
    std::fs::write(&book_path, b"not a PDF").unwrap();

    let error = format!("{:#}", load_book(&book_path).unwrap_err());

    assert!(error.contains("broken.pdf"));
    assert!(error.contains("PDF"));
    assert_ne!(
        error,
        format!("Failed to read PDF metadata from {}", book_path.display())
    );
}

#[test]
fn bookshelf_refresh_and_directory_scan_recognize_pdf_and_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let shelf = dir.path().join("shelf");
    std::fs::create_dir(&shelf).unwrap();
    let book_path = shelf.join("UPPER.PDF");
    write_pdf(
        &book_path,
        &[Some(b"Shelf PDF text")],
        None,
        None,
        OutlineFixture::None,
    );

    let mut store = Store::open(dir.path().join("library.json")).unwrap();
    let report = bookshelf::refresh(&mut store, &shelf).unwrap();
    assert_eq!(report.books_added, 1);
    assert_eq!(report.sidecars_written, 1);

    let book = store.library().books().first().unwrap();
    assert_eq!(book.format, BookFormat::Pdf);
    assert_eq!(book.title, "UPPER");
    let book_id = book.id.clone();
    let progress = Progress::new(TextPosition::new(0, 4));
    store.set_progress(&book_id, progress);
    store
        .add_bookmark(&book_id, "Page 1".to_string(), progress)
        .unwrap();
    bookshelf::sync_sidecar_for_book(&store, &book_id, &shelf).unwrap();
    let sidecar = bookshelf::sidecar_path(&book_path);
    assert!(sidecar.exists());
    assert!(std::fs::read_to_string(&sidecar)
        .unwrap()
        .contains("\"format\": \"Pdf\""));

    let mut restored = Store::open(dir.path().join("restored.json")).unwrap();
    let report = bookshelf::refresh(&mut restored, &shelf).unwrap();
    assert_eq!(report.books_added, 1);
    let restored_book = restored.library().books().first().unwrap();
    assert_eq!(restored_book.progress, progress);
    assert_eq!(restored_book.bookmarks.len(), 1);

    let scan_dir = dir.path().join("scan");
    std::fs::create_dir(&scan_dir).unwrap();
    let scanned_path = scan_dir.join("scan.PdF");
    write_pdf(
        &scanned_path,
        &[Some(b"Directory scan text")],
        None,
        None,
        OutlineFixture::None,
    );
    let mut scanned = Store::open(dir.path().join("scanned.json")).unwrap();
    let result = scanned.scan_lib_dir(&scan_dir);
    assert_eq!(result.imported, 1);
    assert!(result.errors.is_empty());
    assert_eq!(scanned.library().books()[0].format, BookFormat::Pdf);
}

#[test]
fn pdf_format_exposes_page_labels() {
    assert_eq!(BookFormat::Pdf.label(), "PDF");
    assert_eq!(BookFormat::Pdf.section_label(), "Page");
    assert_eq!(BookFormat::Pdf.section_label_plural(), "Pages");
}
