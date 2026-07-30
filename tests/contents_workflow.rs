use std::fs::File;
use std::io::Write;
use std::path::Path;

use reader::book::{load_book, ContentsSource};
use reader::TextPosition;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn write_epub(path: &Path, entries: &[(&str, &str)]) {
    let file = File::create(path).expect("EPUB fixture should be created");
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, contents) in entries {
        archive
            .start_file(*name, options)
            .expect("EPUB entry should start");
        archive
            .write_all(contents.as_bytes())
            .expect("EPUB entry should be written");
    }
    archive.finish().expect("EPUB fixture should finish");
}

fn container_xml() -> &'static str {
    r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
}

fn package_opf() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Contents Fixture</dc:title>
  </metadata>
  <manifest>
    <item id="nav" href="nav/nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="chapter" href="Text/Chapter%201.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="chapter"/>
  </spine>
</package>"#
}

#[test]
fn epub3_navigation_preserves_hierarchy_and_exact_fragment_targets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("epub3.epub");
    let nav = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"
      xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ol>
        <li>
          <a href="../Text/Chapter%201.xhtml#intro">Introduction</a>
          <ol>
            <li><a href="../Text/Chapter%201.xhtml#%E6%B7%B1%E5%85%A5">深入主题</a></li>
            <li><a href="../Text/missing.xhtml">Broken entry</a></li>
          </ol>
        </li>
      </ol>
    </nav>
  </body>
</html>"#;
    let chapter = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body>
    <h1 id="intro">Intro</h1>
    <p>alpha</p>
    <h2 id="深入">深入</h2>
    <p>omega</p>
  </body>
</html>"#;
    let fallback_ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/">
  <navMap>
    <navPoint><navLabel><text>Wrong fallback</text></navLabel><content src="Text/Chapter%201.xhtml"/></navPoint>
  </navMap>
</ncx>"#;
    write_epub(
        &path,
        &[
            ("META-INF/container.xml", container_xml()),
            ("EPUB/package.opf", package_opf()),
            ("EPUB/nav/nav.xhtml", nav),
            ("EPUB/toc.ncx", fallback_ncx),
            ("EPUB/Text/Chapter 1.xhtml", chapter),
        ],
    );

    let document = load_book(&path).unwrap();

    assert_eq!(document.contents_source, ContentsSource::Document);
    assert_eq!(document.contents.len(), 2);
    assert_eq!(document.contents[0].title, "Introduction");
    assert_eq!(document.contents[0].depth, 0);
    assert_eq!(document.contents[0].target, TextPosition::new(0, 0));
    assert_eq!(document.contents[1].title, "深入主题");
    assert_eq!(document.contents[1].depth, 1);
    assert_eq!(document.contents[1].target, TextPosition::new(0, 12));
    assert_eq!(document.sections[0].text, "Intro\nalpha\n深入\nomega");
}

#[test]
fn epub2_ncx_is_used_when_epub3_navigation_has_no_toc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("epub2-fallback.epub");
    let non_toc_nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"
      xmlns:epub="http://www.idpf.org/2007/ops">
  <body><nav epub:type="landmarks"><ol><li><a href="../Text/Chapter%201.xhtml">Cover</a></li></ol></nav></body>
</html>"#;
    let ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/">
  <navMap>
    <navPoint>
      <navLabel><text>Chapter</text></navLabel>
      <content src="Text/Chapter%201.xhtml"/>
      <navPoint>
        <navLabel><text>Detail</text></navLabel>
        <content src="Text/Chapter%201.xhtml#detail"/>
      </navPoint>
      <navPoint>
        <navLabel><text>Missing anchor</text></navLabel>
        <content src="Text/Chapter%201.xhtml#missing"/>
      </navPoint>
    </navPoint>
  </navMap>
</ncx>"#;
    let chapter = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body><h1>Start</h1><p>alpha</p><a id="detail"></a><h2>Detail</h2></body>
</html>"#;
    write_epub(
        &path,
        &[
            ("META-INF/container.xml", container_xml()),
            ("EPUB/package.opf", package_opf()),
            ("EPUB/nav/nav.xhtml", non_toc_nav),
            ("EPUB/toc.ncx", ncx),
            ("EPUB/Text/Chapter 1.xhtml", chapter),
        ],
    );

    let document = load_book(&path).unwrap();

    assert_eq!(document.contents_source, ContentsSource::Document);
    assert_eq!(document.contents.len(), 2);
    assert_eq!(document.contents[0].title, "Chapter");
    assert_eq!(document.contents[0].target, TextPosition::new(0, 0));
    assert_eq!(document.contents[1].title, "Detail");
    assert_eq!(document.contents[1].depth, 1);
    assert_eq!(document.contents[1].target, TextPosition::new(0, 12));
}
