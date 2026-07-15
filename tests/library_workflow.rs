use reader::book::load_book;
use reader::storage::{Progress, Store};
use reader::TextPosition;

#[test]
fn txt_book_library_progress_and_bookmarks_survive_reload() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("journey.txt");
    let store_path = dir.path().join("library.json");
    std::fs::write(&book_path, "Opening page\n\nSecond page with a needle").unwrap();

    let document = load_book(&book_path).unwrap();
    let mut store = Store::open(store_path.clone()).unwrap();
    let book_id = store.add_book(&document, &book_path).unwrap().id.clone();
    let progress = Progress::new(TextPosition::new(1, 0));
    store.set_progress(&book_id, progress);
    store
        .add_bookmark(&book_id, "Needle section".to_string(), progress)
        .unwrap();
    store.save().unwrap();

    let loaded = Store::open(store_path).unwrap();
    let loaded_book = loaded.resolve_book("journey").unwrap();

    assert_eq!(loaded_book.id, book_id);
    assert_eq!(loaded_book.progress, progress);
    assert_eq!(loaded.bookmarks_for(&book_id).len(), 1);
    assert_eq!(loaded.bookmarks_for(&book_id)[0].label, "Needle section");
}

#[test]
fn loaded_txt_books_are_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let book_path = dir.path().join("searchable.txt");
    std::fs::write(&book_path, "Alpha\n\nBeta needle Gamma").unwrap();

    let document = load_book(&book_path).unwrap();
    let results = document.search("needle");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].position.section_index, 1);
    assert!(results[0].preview.contains("needle"));
}
