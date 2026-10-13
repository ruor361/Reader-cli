use time::OffsetDateTime;

use reader::book::{load_book, CURRENT_PARSER_VERSION};
use reader::bookshelf::{read_sidecar, refresh, sidecar_path};
use reader::reading_data::{ReadingDataStore, ReadingEvent, SessionCheckpoint};
use reader::storage::Store;
use reader::TextPosition;

#[test]
fn txt_legacy_position_and_bookmarks_migrate_to_new_chapters_on_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("story.txt");
    let store_path = dir.path().join("library.json");

    let text = "第一章 开始\n\n第一段短文。\n\n第二段短文。\n\n第二章 推进\n\n第三段短文。";
    std::fs::write(&book_path, text).unwrap();

    // 模拟 v1 版本的 library.json 记录
    // 旧切分：
    // 段 0: "第一章 开始"
    // 段 1: "第一段短文。"
    // 段 2: "第二段短文。"
    // 段 3: "第二章 推进"
    // 段 4: "第三段短文。"
    let legacy_json = serde_json::json!({
        "schema_version": 3,
        "books": [{
            "id": "story-book-id",
            "title": "story",
            "path": book_path,
            "format": "Txt",
            "content_metrics": {
                "section_char_counts": [6, 6, 6, 6, 6]
            },
            "progress": {
                "position": {
                    "section_index": 2,
                    "char_offset": 2
                }
            },
            "last_opened": null,
            "missing": false,
            "bookmarks": [{
                "id": "bm-1",
                "label": "Mark in chapter 2",
                "position": {
                    "section_index": 4,
                    "char_offset": 1
                }
            }],
            "parser_version": 1
        }],
        "last_book_id": "story-book-id"
    });
    std::fs::write(
        &store_path,
        serde_json::to_string_pretty(&legacy_json).unwrap(),
    )
    .unwrap();

    let mut store = Store::open(store_path.clone()).unwrap();
    let book = store.book_by_id("story-book-id").unwrap();
    assert_eq!(book.parser_version, 1);

    // 升级触发：打开或更新图书 metrics（add_book）
    let document = load_book(&book_path).unwrap();
    assert_eq!(document.sections.len(), 2);
    store.add_book(&document, &book_path).unwrap();

    let upgraded = store.book_by_id(&document.id).unwrap();
    assert_eq!(upgraded.parser_version, CURRENT_PARSER_VERSION);
    // 旧段落 2 属于第一章，新章节索引必须是 0，不能被 clamp 到越界位置
    assert_eq!(upgraded.progress.position.section_index, 0);
    assert!(upgraded.progress.position.char_offset > 0);

    assert_eq!(upgraded.bookmarks[0].position.char_offset, 9);
    assert_eq!(
        document.sections[1]
            .text
            .chars()
            .nth(upgraded.bookmarks[0].position.char_offset),
        Some('三')
    );

    // 验证幂等性：再次保存与打开，不会重复转换或变动位置
    store.save().unwrap();
    let reloaded = Store::open(store_path).unwrap();
    let reloaded_book = reloaded.book_by_id(&document.id).unwrap();
    assert_eq!(reloaded_book.parser_version, CURRENT_PARSER_VERSION);
    assert_eq!(reloaded_book.progress.position, upgraded.progress.position);
    assert_eq!(
        reloaded_book.bookmarks[0].position,
        upgraded.bookmarks[0].position
    );
}

#[test]
fn sidecar_migration_and_refresh_preserves_reading_positions() {
    let dir = tempfile::tempdir().unwrap();
    let shelf_dir = dir.path().join("shelf");
    std::fs::create_dir(&shelf_dir).unwrap();
    let book_path = shelf_dir.join("novel.txt");
    let text = "第一章 序幕\n\n这是引言。\n\n这是正文。\n\n第二章 发展\n\n下回分解。";
    std::fs::write(&book_path, text).unwrap();

    // 写入 v1 版本的便携 sidecar
    let sidecar_file = sidecar_path(&book_path);
    let legacy_sidecar = serde_json::json!({
        "schema_version": 3,
        "book": {
            "id": "novel-id",
            "title": "novel",
            "author": null,
            "format": "Txt",
            "content_metrics": { "section_char_counts": [6, 5, 5, 6, 5] },
            "progress": {
                "position": { "section_index": 2, "char_offset": 1 }
            },
            "last_opened": null,
            "bookmarks": [{
                "id": "bm-sidecar",
                "label": "Mark",
                "position": { "section_index": 4, "char_offset": 2 }
            }],
            "parser_version": 1
        }
    });
    std::fs::write(
        &sidecar_file,
        serde_json::to_string_pretty(&legacy_sidecar).unwrap(),
    )
    .unwrap();

    let mut store = Store::open(dir.path().join("library.json")).unwrap();
    let report = refresh(&mut store, &shelf_dir).unwrap();
    assert_eq!(report.books_added, 1);

    let book = store.resolve_book("novel").unwrap();
    assert_eq!(book.parser_version, CURRENT_PARSER_VERSION);
    assert_eq!(book.progress.position.section_index, 0);
    assert_eq!(book.bookmarks[0].position.section_index, 1);

    // 验证新写回的 sidecar 已升级为 CURRENT_PARSER_VERSION
    let reloaded_sidecar = read_sidecar(&book_path).unwrap().unwrap();
    assert_eq!(reloaded_sidecar.parser_version, CURRENT_PARSER_VERSION);
}

#[test]
fn sync_replay_does_not_corrupt_migrated_positions_with_legacy_events() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("book.txt");
    let text = "第一章 开始\n\n第一段短文。\n\n第二段短文。\n\n第二章 推进\n\n第三段短文。";
    std::fs::write(&book_path, text).unwrap();

    let store_path = dir.path().join("library.json");
    let document = load_book(&book_path).unwrap();
    let mut store = Store::open(store_path).unwrap();
    let book_id = store.add_book(&document, &book_path).unwrap().id.clone();

    let sync_dir = dir.path().join("reading_data");
    let data_store = ReadingDataStore::new(sync_dir, None);

    // 模拟来自旧客户端的同步事件（带有旧版坐标 section_index: 2, char_offset: 2，且 parser_version: None）
    let base = OffsetDateTime::UNIX_EPOCH + time::Duration::days(10);
    data_store
        .upsert_event(&SessionCheckpoint {
            id: "legacy-event".into(),
            event: ReadingEvent {
                book_id: book_id.clone(),
                started_at: base,
                ended_at: base + time::Duration::minutes(5),
                start_position: TextPosition::new(0, 0),
                end_position: TextPosition::new(2, 2),
                parser_version: None,
            },
        })
        .unwrap();

    // 重放同步
    data_store.synchronize_store(&mut store).unwrap();

    let book = store.book_by_id(&book_id).unwrap();
    // 进度应被转换为新坐标，落在新章节 0 内，而不是被 clamp 到末尾或越界
    assert_eq!(book.progress.position.section_index, 0);
    assert!(book.progress.position.char_offset > 0);
}

#[test]
fn sync_seeds_unmigrated_bookmarks_with_actual_version_and_migrates_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("book.txt");
    let text = "alpha\n\n\nbeta";
    std::fs::write(&book_path, text).unwrap();

    let store_path = dir.path().join("library.json");
    let legacy_json = serde_json::json!({
        "schema_version": 3,
        "books": [{
            "id": "txt-book-id",
            "title": "book",
            "path": book_path,
            "format": "Txt",
            "content_metrics": { "section_char_counts": [5, 4] },
            "progress": {
                "position": { "section_index": 0, "char_offset": 0 }
            },
            "last_opened": null,
            "missing": false,
            "bookmarks": [{
                "id": "bm-legacy",
                "label": "Mark at beta",
                "position": { "section_index": 1, "char_offset": 1 }
            }],
            "parser_version": 1
        }],
        "last_book_id": "txt-book-id"
    });
    std::fs::write(
        &store_path,
        serde_json::to_string_pretty(&legacy_json).unwrap(),
    )
    .unwrap();

    let mut store = Store::open(store_path).unwrap();
    let sync_dir = dir.path().join("reading_data");
    let data_store = ReadingDataStore::new(sync_dir, None);

    let report = data_store.synchronize_store(&mut store).unwrap();
    assert!(report.state_changed);

    let book = store.book_by_id("txt-book-id").unwrap();
    assert_eq!(book.parser_version, CURRENT_PARSER_VERSION);
    assert_eq!(book.bookmarks[0].position, TextPosition::new(0, 8));
}

#[test]
fn epub_with_image_cover_skipped_preserves_readable_section_index_migration() {
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("cover_book.epub");

    let file = File::create(&book_path).unwrap();
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let container = r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test EPUB</dc:title>
  </metadata>
  <manifest>
    <item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="cover"/>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    let cover_html = r#"<?xml version="1.0" encoding="utf-8"?>
<html><body><img src="cover.jpg" alt="" /></body></html>"#;

    let ch1_html = r#"<?xml version="1.0" encoding="utf-8"?>
<html><body><p>这是第一章的正文内容</p></body></html>"#;

    for (name, content) in &[
        ("META-INF/container.xml", container),
        ("package.opf", opf),
        ("cover.xhtml", cover_html),
        ("ch1.xhtml", ch1_html),
    ] {
        archive.start_file(*name, options).unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    }
    archive.finish().unwrap();

    let old_pos = TextPosition::new(0, 6);
    let migrated = reader::book::migrate_position_for_book(
        reader::book::BookFormat::Epub,
        &book_path,
        old_pos,
    )
    .unwrap();

    assert_eq!(migrated, TextPosition::new(0, 6));
}
