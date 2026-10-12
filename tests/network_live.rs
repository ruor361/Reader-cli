use reader::book::BookFormat;
use reader::config::NetworkLibraryConfig;
use reader::network::NetworkClient;

/// Opt-in smoke check: uses .env, validates a temporary EPUB, and never opens
/// the local library or changes remote books. Set READER_NETWORK_URL explicitly.
#[test]
#[ignore = "requires the user's network library and .env credentials"]
fn authenticated_catalog_and_epub_download() {
    let config = NetworkLibraryConfig {
        url: Some(std::env::var("READER_NETWORK_URL").expect("Set READER_NETWORK_URL")),
        ..Default::default()
    };
    let client = NetworkClient::connect(&config).unwrap();
    let books = client.books().unwrap();
    let book = books
        .iter()
        .find(|book| book.format == Some(BookFormat::Epub))
        .expect("No EPUB is available for the download check");
    let download = client.download(book).unwrap();
    println!("Authenticated OPDS: {} books; one EPUB download validated. Temporary download will be removed.", books.len());
    drop(download);
}
