//! Read-only Calibre-Web OPDS access. Credentials stay in memory.
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::{StatusCode, Url};

use crate::book::BookFormat;
use crate::config::NetworkLibraryConfig;

const ATOM: &str = "http://www.w3.org/2005/Atom";
const MAX_FEED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BOOK_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RemoteBook {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub format: Option<BookFormat>,
    pub download_url: Option<Url>,
}

#[derive(Debug, Clone)]
pub struct CatalogPage {
    pub books: Vec<RemoteBook>,
    pub next: Option<Url>,
}

#[derive(Clone)]
pub struct NetworkClient {
    client: Client,
    base: Url,
    credentials: Option<(String, String)>,
    login_username: Option<String>,
    login_password: Option<String>,
}

pub struct DownloadedBook {
    // Keeps the temporary directory alive until managed import completes.
    pub directory: tempfile::TempDir,
    pub path: PathBuf,
}

// Intentionally no Debug/Serialize: this structure can contain a password.
#[derive(Default)]
struct ConnectionSettings {
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    ca_certificate: Option<String>,
}

impl ConnectionSettings {
    fn load(configured: Option<&Path>) -> Result<Self> {
        if let Some(path) = std::env::var_os("READER_ENV_FILE") {
            return Self::from_path(Path::new(&path));
        }
        if let Some(path) = configured {
            return Self::from_path(path);
        }
        let local = std::env::current_dir()?.join(".env");
        if local.is_file() {
            return Self::from_path(&local);
        }
        if let Ok(executable) = std::env::current_exe() {
            if let Some(parent) = executable.parent() {
                let adjacent = parent.join(".env");
                if adjacent.is_file() {
                    return Self::from_path(&adjacent);
                }
            }
        }
        Ok(Self::default())
    }

    fn from_path(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).context("Cannot read the local .env connection file")?;
        let text = text.trim_start_matches('\u{feff}').trim();
        let mut settings = Self::default();
        if text.starts_with('{') {
            let value: serde_json::Value = serde_json::from_str(text)
                .or_else(|_| serde_json::from_str(&without_trailing_commas(text)))
                .map_err(|_| {
                    anyhow::anyhow!(
                        "The .env JSON is invalid. Use a JSON object with quoted keys and values."
                    )
                })?;
            let object = value
                .as_object()
                .context("The .env JSON must be an object")?;
            for (key, value) in object {
                if is_connection_key(key) {
                    let value = value
                        .as_str()
                        .context("Connection values in .env must be strings")?;
                    settings.set(key, value.to_string());
                }
            }
        } else {
            // Parse standard KEY=VALUE files too, without exporting secrets into the process environment.
            let entries = dotenvy::from_read_iter(text.as_bytes());
            for entry in entries {
                let (key, value) = entry
                    .map_err(|_| anyhow::anyhow!("The .env file has an invalid KEY=VALUE line"))?;
                settings.set(&key, value);
            }
        }
        Ok(settings)
    }

    fn set(&mut self, key: &str, value: String) {
        match key {
            "url" | "READER_NETWORK_URL" => self.url = Some(value),
            "username" | "READER_NETWORK_USERNAME" => self.username = Some(value),
            "password" | "READER_NETWORK_PASSWORD" => self.password = Some(value),
            "ca_certificate" | "READER_NETWORK_CA_CERT" => self.ca_certificate = Some(value),
            _ => {}
        }
    }
}

pub fn validate_env_file(path: &Path) -> Result<()> {
    ConnectionSettings::from_path(path).map(|_| ())
}

fn is_connection_key(key: &str) -> bool {
    matches!(
        key,
        "url"
            | "username"
            | "password"
            | "ca_certificate"
            | "READER_NETWORK_URL"
            | "READER_NETWORK_USERNAME"
            | "READER_NETWORK_PASSWORD"
            | "READER_NETWORK_CA_CERT"
    )
}

// Accept the user's JSON-style .env example, including its final trailing comma.
// Commas within passwords/strings are never changed.
fn without_trailing_commas(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
            output.push(ch);
        } else if ch == ',' {
            let mut remaining = chars.clone();
            let next = remaining.find(|ch| !ch.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                output.push(ch);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

impl NetworkClient {
    pub fn new(config: &NetworkLibraryConfig) -> Result<Self> {
        let settings = ConnectionSettings::load(config.env_file.as_deref())?;
        let address = std::env::var("READER_NETWORK_URL")
            .ok()
            .or(settings.url)
            .or_else(|| config.url.clone())
            .context(
                "Configure the network library with `reader network configure <URL>` first.",
            )?;
        let base = server_url(&address)?;
        let mut builder = Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Reader-cli/", env!("CARGO_PKG_VERSION")));
        let certificate_path = std::env::var_os("READER_NETWORK_CA_CERT")
            .map(PathBuf::from)
            .or(settings.ca_certificate.map(PathBuf::from))
            .or_else(|| config.ca_certificate.clone());
        if let Some(path) = certificate_path {
            let pem = std::fs::read(&path)
                .with_context(|| format!("Cannot read CA certificate {}", path.display()))?;
            let certificates = reqwest::Certificate::from_pem_bundle(&pem)
                .context("The CA certificate must be a PEM certificate bundle")?;
            if certificates.is_empty() {
                bail!("The CA file contains no PEM certificates.");
            }
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        Ok(Self {
            client: builder.build()?,
            base,
            credentials: None,
            login_username: std::env::var("READER_NETWORK_USERNAME")
                .ok()
                .or(settings.username)
                .or_else(|| config.username.clone()),
            login_password: std::env::var("READER_NETWORK_PASSWORD")
                .ok()
                .or(settings.password),
        })
    }

    /// Probe without credentials first. Ask for credentials only when the server requires them.
    /// Called before entering terminal raw mode.
    pub fn connect(config: &NetworkLibraryConfig) -> Result<Self> {
        let mut client = Self::new(config)?;
        let probe = client.get(client.base.clone())?;
        if probe.status() == StatusCode::UNAUTHORIZED {
            if client.base.scheme() != "https" {
                bail!("Use HTTPS before signing in to the network library.");
            }
            let username = client
                .login_username
                .take()
                .filter(|name| !name.trim().is_empty());
            let username = match username {
                Some(name) => name,
                None => {
                    print!("Network library username: ");
                    std::io::stdout().flush()?;
                    let mut name = String::new();
                    std::io::stdin().read_line(&mut name)?;
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        bail!("Network login cancelled: username is empty.");
                    }
                    name
                }
            };
            let password = match client.login_password.take() {
                Some(password) => password,
                None => rpassword::prompt_password("Network library password (hidden): ")?,
            };
            client.credentials = Some((username, password));
            check_status(client.get(client.base.clone())?)?;
        } else {
            check_status(probe)?;
        }
        Ok(client)
    }

    pub fn catalog_url(&self, query: &str) -> Result<Url> {
        let mut url = self.base.clone();
        let prefix = self.base.path().trim_end_matches('/');
        if query.trim().is_empty() {
            // Calibre-Web's all-books acquisition feed, independent of sidebar visibility.
            url.set_path(&format!("{prefix}/books/letter/00"));
        } else {
            url.set_path(&format!("{prefix}/search"));
            url.query_pairs_mut().append_pair("query", query.trim());
        }
        Ok(url)
    }

    pub fn page(&self, url: Url) -> Result<CatalogPage> {
        ensure_same_origin(&self.base, &url)?;
        let response = check_status(self.get(url.clone())?)?;
        let final_url = response.url().clone();
        let data = read_bounded(response, MAX_FEED_BYTES)?;
        let xml = std::str::from_utf8(&data).context("OPDS feed is not UTF-8")?;
        parse_feed(xml, &final_url)
    }

    /// Fetch all pages for the script-friendly `network list` command.
    pub fn books(&self) -> Result<Vec<RemoteBook>> {
        let mut next = Some(self.catalog_url("")?);
        let mut visited = HashSet::new();
        let mut books = Vec::new();
        let mut ids = HashSet::new();
        while let Some(url) = next {
            if !visited.insert(url.to_string()) || visited.len() > 2000 {
                bail!("OPDS pagination contains a loop or exceeds 2000 pages.");
            }
            let page = self.page(url)?;
            for book in page.books {
                if ids.insert(book.id.clone()) {
                    books.push(book);
                }
            }
            next = page.next;
        }
        Ok(books)
    }

    pub fn download(&self, book: &RemoteBook) -> Result<DownloadedBook> {
        let url = book
            .download_url
            .clone()
            .context("This book has no EPUB, TXT or PDF download.")?;
        let format = book.format.context("This book has no supported format.")?;
        let extension = match format {
            BookFormat::Epub => "epub",
            BookFormat::Txt => "txt",
            BookFormat::Pdf => "pdf",
            BookFormat::Mobi => {
                bail!("MOBI requires conversion to EPUB, TXT or PDF before reading.")
            }
        };
        let response = check_status(self.get(url)?)?;
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.starts_with("text/html") || value.starts_with("application/xhtml+xml")
            })
        {
            bail!("The download endpoint returned a web/login page instead of a book.");
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_BOOK_BYTES)
        {
            bail!("Book download exceeds the 256 MiB limit.");
        }
        let directory = tempfile::tempdir()?;
        let path = directory
            .path()
            .join(format!("{}.{}", safe_filename(&book.title), extension));
        let mut file = std::fs::File::create(&path)?;
        let copied = std::io::copy(&mut response.take(MAX_BOOK_BYTES + 1), &mut file)?;
        if copied > MAX_BOOK_BYTES {
            bail!("Book download exceeds the 256 MiB limit.");
        }
        file.sync_all()?;
        // Validate content before the caller imports it into the local library.
        crate::book::load_book(&path).context("The downloaded file is not a readable book")?;
        Ok(DownloadedBook { directory, path })
    }

    fn get(&self, mut url: Url) -> Result<Response> {
        for _ in 0..8 {
            ensure_same_origin(&self.base, &url)?;
            let mut request = self.client.get(url.clone());
            if let Some((username, password)) = &self.credentials {
                request = request.basic_auth(username, Some(password));
            }
            let response = request.send().map_err(|err| anyhow::anyhow!(
                "Cannot connect to the network library: {}. For a private CA, configure --ca-certificate <PEM>.",
                err.without_url()
            ))?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .context("Server redirect has no Location header")?
                    .to_str()?;
                url = url.join(location)?;
                ensure_same_origin(&self.base, &url)?;
                continue;
            }
            return Ok(response);
        }
        bail!("Too many network library redirects.")
    }
}

pub fn server_url(address: &str) -> Result<Url> {
    let mut url =
        Url::parse(address.trim()).context("Enter a complete http:// or https:// server URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("Network library URL must use HTTP or HTTPS.");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Keep credentials out of the server URL; use --username and the password prompt.");
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/opds") {
        url.set_path(&format!("{path}/opds"));
    }
    Ok(url)
}

fn ensure_same_origin(base: &Url, url: &Url) -> Result<()> {
    if base.origin() != url.origin() || !url.username().is_empty() || url.password().is_some() {
        bail!("The OPDS link or redirect leaves the configured server; request blocked.");
    }
    Ok(())
}

fn check_status(response: Response) -> Result<Response> {
    match response.status() {
        StatusCode::UNAUTHORIZED => {
            bail!("Network login failed (401). Check your OPDS username and password.")
        }
        StatusCode::FORBIDDEN => {
            bail!("Network access denied (403). Check the account's OPDS/download permissions.")
        }
        status if !status.is_success() => {
            bail!("Network library returned HTTP {}.", status.as_u16())
        }
        _ => Ok(response),
    }
}

fn read_bounded(response: Response, limit: u64) -> Result<Vec<u8>> {
    if response.content_length().is_some_and(|len| len > limit) {
        bail!("OPDS feed is too large.");
    }
    let mut bytes = Vec::new();
    response.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("OPDS feed is too large.");
    }
    Ok(bytes)
}

pub fn parse_feed(xml: &str, base: &Url) -> Result<CatalogPage> {
    let document =
        roxmltree::Document::parse(xml).context("Server did not return a valid OPDS XML feed")?;
    let root = document.root_element();
    if !root.has_tag_name((ATOM, "feed")) {
        bail!("Server returned a login page or a non-OPDS response.");
    }
    let mut books = Vec::new();
    for entry in root
        .children()
        .filter(|node| node.has_tag_name((ATOM, "entry")))
    {
        let mut downloads = Vec::new();
        let mut has_acquisition = false;
        for link in entry
            .children()
            .filter(|node| node.has_tag_name((ATOM, "link")))
        {
            let rel = link.attribute("rel").unwrap_or("");
            if rel != "http://opds-spec.org/acquisition"
                && !rel.starts_with("http://opds-spec.org/acquisition/")
            {
                continue;
            }
            has_acquisition = true;
            let format = match link
                .attribute("type")
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
            {
                "application/epub+zip" => Some((0, BookFormat::Epub)),
                "text/plain" => Some((1, BookFormat::Txt)),
                "application/pdf" => Some((2, BookFormat::Pdf)),
                _ => None,
            };
            if let (Some((priority, format)), Some(href)) = (format, link.attribute("href")) {
                let url = base.join(href)?;
                ensure_same_origin(base, &url)?;
                downloads.push((priority, format, url));
            }
        }
        if !has_acquisition {
            continue;
        }
        downloads.sort_by_key(|(priority, _, _)| *priority);
        let selected = downloads.into_iter().next();
        let title = child_text(entry, "title").unwrap_or("Untitled").to_string();
        let id = child_text(entry, "id").unwrap_or(&title).to_string();
        let authors: Vec<_> = entry
            .children()
            .filter(|node| node.has_tag_name((ATOM, "author")))
            .filter_map(|author| child_text(author, "name"))
            .collect();
        books.push(RemoteBook {
            id,
            title,
            author: (!authors.is_empty()).then(|| authors.join(", ")),
            format: selected.as_ref().map(|(_, format, _)| *format),
            download_url: selected.map(|(_, _, url)| url),
        });
    }
    let next = root
        .children()
        .find(|node| node.has_tag_name((ATOM, "link")) && node.attribute("rel") == Some("next"))
        .and_then(|node| node.attribute("href"))
        .map(|href| base.join(href))
        .transpose()?;
    if let Some(next) = &next {
        ensure_same_origin(base, next)?;
    }
    Ok(CatalogPage { books, next })
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.has_tag_name((ATOM, tag)))
        .and_then(|child| child.text())
}

fn safe_filename(title: &str) -> String {
    let title: String = title
        .chars()
        .take(100)
        .map(|ch| {
            if ch.is_control() || "<>:\"/\\|?*".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let title = title.trim().trim_matches('.').trim();
    // A fixed prefix avoids Windows reserved device names too.
    format!(
        "Book - {}",
        if title.is_empty() { "Untitled" } else { title }
    )
}

/// Import locally; never upload metadata, journals or changes to the remote server.
pub fn import_download(
    store: &mut crate::storage::Store,
    config: &crate::config::ReaderConfig,
    download: &DownloadedBook,
) -> Result<crate::storage::LibraryBook> {
    if config.bookshelf_dir.is_none() {
        bail!("Set a local download bookshelf with `reader refresh <DIR>` first.");
    }
    let outcome = crate::bookshelf::import_managed_book(store, config, Path::new(&download.path))?;
    Ok(outcome.book)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn feed(entries: &str, next: &str) -> String {
        format!(r#"<feed xmlns="{ATOM}">{next}{entries}</feed>"#)
    }

    fn entry(id: &str, title: &str, links: &str) -> String {
        format!(
            r#"<entry><id>{id}</id><title>{title}</title><author><name>甲</name></author><author><name>乙</name></author>{links}</entry>"#
        )
    }

    fn acquisition(path: &str, mime: &str) -> String {
        format!(r#"<link rel="http://opds-spec.org/acquisition" href="{path}" type="{mime}"/>"#)
    }

    // Bounded local HTTP fixture. No live library or persistent user data is involved.
    fn server(
        count: usize,
        handler: impl Fn(&str) -> (u16, String, String) + Send + 'static,
    ) -> (Url, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/opds", listener.local_addr().unwrap())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let worker = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            for _ in 0..count {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "fixture timed out");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(err) => panic!("{err}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let (status, headers, body) = handler(&request);
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).unwrap();
            }
        });
        (base, worker)
    }

    fn client(base: Url) -> NetworkClient {
        NetworkClient {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            base,
            credentials: None,
            login_username: None,
            login_password: None,
        }
    }

    #[test]
    fn loads_json_env_with_trailing_comma_without_modifying_password_strings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, r#"{ "username": "reader", "password": "p,}\\\"word", "READER_NETWORK_URL": "https://books.example", }"#).unwrap();
        let settings = ConnectionSettings::from_path(&path).unwrap();
        assert_eq!(settings.username.as_deref(), Some("reader"));
        assert_eq!(settings.password.as_deref(), Some("p,}\\\"word"));
        assert_eq!(settings.url.as_deref(), Some("https://books.example"));
    }

    #[test]
    fn env_syntax_errors_do_not_expose_secret_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            "READER_NETWORK_USERNAME=reader\nREADER_NETWORK_PASSWORD='example, password'\n",
        )
        .unwrap();
        let settings = ConnectionSettings::from_path(&path).unwrap();
        assert_eq!(settings.password.as_deref(), Some("example, password"));
        std::fs::write(&path, "READER_NETWORK_PASSWORD='secret-example\n").unwrap();
        let error = ConnectionSettings::from_path(&path)
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("secret-example"));
        std::fs::write(&path, r#"{"password": "secret-example" invalid}"#).unwrap();
        let error = ConnectionSettings::from_path(&path)
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("secret-example"));
    }

    #[test]
    fn parses_cwa_formats_unicode_authors_and_relative_pagination() {
        let links = format!(
            "{}{}",
            acquisition("/opds/download/7/pdf", "application/pdf"),
            acquisition("/opds/download/7/epub", "application/epub+zip")
        );
        let xml = feed(&format!("{}{}<entry><title>Navigation</title><link rel=\"subsection\" href=\"/opds/books\"/></entry>",
            entry("uuid:7", "中文 &amp; 书", &links),
            entry("uuid:8", "MOBI book", &acquisition("/opds/download/8/mobi", "application/x-mobipocket-ebook"))),
            r#"<link rel="next" href="?offset=20"/>"#);
        let page = parse_feed(
            &xml,
            &Url::parse("https://books.example/opds/books/letter/00").unwrap(),
        )
        .unwrap();
        assert_eq!(page.books.len(), 2);
        assert_eq!(page.books[0].title, "中文 & 书");
        assert_eq!(page.books[0].author.as_deref(), Some("甲, 乙"));
        assert_eq!(page.books[0].format, Some(BookFormat::Epub));
        assert_eq!(
            page.books[0].download_url.as_ref().unwrap().path(),
            "/opds/download/7/epub"
        );
        assert!(page.books[1].format.is_none());
        assert_eq!(
            page.next.unwrap().as_str(),
            "https://books.example/opds/books/letter/00?offset=20"
        );
    }

    #[test]
    fn rejects_login_html_external_links_and_embedded_credentials() {
        let base = Url::parse("https://books.example/opds").unwrap();
        assert!(parse_feed("<html><body>Login</body></html>", &base).is_err());
        let xml = feed(
            &entry(
                "1",
                "book",
                &acquisition("https://other.example/book.epub", "application/epub+zip"),
            ),
            "",
        );
        assert!(parse_feed(&xml, &base).is_err());
        let xml = feed(
            "",
            r#"<link rel="next" href="http://books.example/opds?offset=1"/>"#,
        );
        assert!(parse_feed(&xml, &base).is_err());
        assert!(server_url("https://user:secret@books.example").is_err());
        assert!(server_url("file:///books").is_err());
        assert_eq!(
            server_url("https://books.example/calibre/")
                .unwrap()
                .as_str(),
            "https://books.example/calibre/opds"
        );
    }

    #[test]
    fn loads_all_pages_and_deduplicates_remote_ids() {
        let (base, worker) = server(2, |request| {
            let link = acquisition("/opds/download/1/txt", "text/plain");
            let body = if request.starts_with("GET /opds/books/letter/00?offset=1 ") {
                feed(
                    &format!("{}{}", entry("1", "One", &link), entry("2", "Two", &link)),
                    "",
                )
            } else {
                assert!(request.starts_with("GET /opds/books/letter/00 "));
                feed(
                    &entry("1", "One", &link),
                    r#"<link rel="next" href="?offset=1"/>"#,
                )
            };
            (200, "Content-Type: application/atom+xml\r\n".into(), body)
        });
        let client = client(base);
        let books = client.books().unwrap();
        assert_eq!(
            books
                .iter()
                .map(|book| book.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "2"]
        );
        let search = client.catalog_url("中文 & one").unwrap();
        assert_eq!(search.path(), "/opds/search");
        assert_eq!(
            search.query_pairs().collect::<Vec<_>>(),
            [("query".into(), "中文 & one".into())]
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_redirect_to_another_server_without_following_it() {
        let (base, worker) = server(1, |_| {
            (
                302,
                "Location: https://other.example/steal\r\n".into(),
                String::new(),
            )
        });
        let client = client(base);
        let error = client
            .page(client.catalog_url("").unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("leaves the configured server"));
        worker.join().unwrap();
    }

    #[test]
    fn downloads_and_imports_validated_content_without_remote_writes() {
        let (base, worker) = server(2, |request| {
            assert!(request.starts_with("GET /opds/download/1/txt "));
            (
                200,
                "Content-Type: text/plain\r\n".into(),
                "第一章\n\n你好，世界。".into(),
            )
        });
        let client = client(base.clone());
        let remote = RemoteBook {
            id: "uuid:1".into(),
            title: "../CON:中文".into(),
            author: None,
            format: Some(BookFormat::Txt),
            download_url: Some(base.join("/opds/download/1/txt").unwrap()),
        };
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::storage::Store::open(dir.path().join("library.json")).unwrap();
        let mut config = crate::config::ReaderConfig::default();
        config.bookshelf_dir = Some(dir.path().join("shelf"));
        std::fs::create_dir(config.bookshelf_dir.as_ref().unwrap()).unwrap();
        let download = client.download(&remote).unwrap();
        assert_eq!(download.path.parent(), Some(download.directory.path()));
        let book = import_download(&mut store, &config, &download).unwrap();
        store.set_progress(
            &book.id,
            crate::storage::Progress::new(crate::book::TextPosition::new(0, 2)),
        );
        let again = client.download(&remote).unwrap();
        let book_again = import_download(&mut store, &config, &again).unwrap();
        assert_eq!(book.id, book_again.id);
        assert_eq!(store.library().books().len(), 1);
        assert_eq!(
            store.progress_for(&book.id).unwrap().position.char_offset,
            2
        );
        assert!(book.path.canonicalize().unwrap().starts_with(
            config
                .bookshelf_dir
                .as_ref()
                .unwrap()
                .canonicalize()
                .unwrap()
        ));
        assert!(crate::bookshelf::sidecar_path(&book.path).exists());
        worker.join().unwrap();
    }

    #[test]
    fn http_auth_failures_and_invalid_downloads_do_not_create_library_entries() {
        let (base, worker) = server(2, |request| {
            if request.starts_with("GET /opds/download/1/epub ") {
                (
                    200,
                    "Content-Type: text/html\r\n".into(),
                    "<html>Login page</html>".into(),
                )
            } else {
                (401, String::new(), "Unauthorized".into())
            }
        });
        let client = client(base.clone());
        let error = client.page(base.clone()).unwrap_err().to_string();
        assert!(error.contains("401"));
        let remote = RemoteBook {
            id: "1".into(),
            title: "Book".into(),
            author: None,
            format: Some(BookFormat::Epub),
            download_url: Some(base.join("/opds/download/1/epub").unwrap()),
        };
        let err = client.download(&remote).err().unwrap().to_string();
        assert!(err.contains("web/login page"));
        worker.join().unwrap();
    }
}
