//! OPDS 1.x Atom XML feed builders.
//!
//! Provides `AtomFeed` and `AtomEntry` types that render OPDS-compliant
//! Atom XML using `quick-xml`.

use std::io::Cursor;

use chrono::{DateTime, Utc};
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};

const ATOM_NS: &str = "http://www.w3.org/2005/Atom";
const OPDS_NS: &str = "http://opds-spec.org/2010/catalog";

/// Link relation constants for OPDS feeds.
pub mod rel {
    pub const SELF: &str = "self";
    pub const START: &str = "start";
    pub const SUBSECTION: &str = "subsection";
    pub const NEXT: &str = "next";
    pub const SEARCH: &str = "search";
    pub const ACQUISITION: &str = "http://opds-spec.org/acquisition";
    pub const IMAGE: &str = "http://opds-spec.org/image";
    pub const THUMBNAIL: &str = "http://opds-spec.org/image/thumbnail";
}

/// OPDS feed profile MIME types.
pub mod mime {
    pub const NAVIGATION: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
    pub const ACQUISITION: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";
    pub const ATOM_XML: &str = "application/atom+xml";
    pub const OPENSEARCH: &str = "application/opensearchdescription+xml";
}

/// A link in an Atom feed or entry.
#[derive(Clone)]
pub struct AtomLink {
    pub rel: String,
    pub href: String,
    pub link_type: Option<String>,
    pub title: Option<String>,
}

impl AtomLink {
    #[must_use]
    pub fn new(rel: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            rel: rel.into(),
            href: href.into(),
            link_type: None,
            title: None,
        }
    }

    #[must_use]
    pub fn with_type(mut self, link_type: impl Into<String>) -> Self {
        self.link_type = Some(link_type.into());
        self
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// An entry in an Atom/OPDS feed.
pub struct AtomEntry {
    pub id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub content: Option<String>,
    pub authors: Vec<String>,
    pub links: Vec<AtomLink>,
}

impl AtomEntry {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>, updated: DateTime<Utc>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            updated,
            content: None,
            authors: Vec::new(),
            links: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    #[must_use]
    pub fn with_author(mut self, name: impl Into<String>) -> Self {
        self.authors.push(name.into());
        self
    }

    #[must_use]
    pub fn with_link(mut self, link: AtomLink) -> Self {
        self.links.push(link);
        self
    }
}

/// An OPDS Atom feed (navigation or acquisition).
pub struct AtomFeed {
    pub id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub links: Vec<AtomLink>,
    pub entries: Vec<AtomEntry>,
}

impl AtomFeed {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>, updated: DateTime<Utc>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            updated,
            links: Vec::new(),
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_link(mut self, link: AtomLink) -> Self {
        self.links.push(link);
        self
    }

    #[must_use]
    pub fn with_entry(mut self, entry: AtomEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Renders the feed as an OPDS 1.x Atom XML document.
    pub fn to_xml(&self) -> Result<String, quick_xml::Error> {
        let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        let mut feed_start = BytesStart::new("feed");
        feed_start.push_attribute(("xmlns", ATOM_NS));
        feed_start.push_attribute(("xmlns:opds", OPDS_NS));
        writer.write_event(Event::Start(feed_start))?;

        write_text_element(&mut writer, "id", &self.id)?;
        write_text_element(&mut writer, "title", &self.title)?;
        write_text_element(&mut writer, "updated", &self.updated.to_rfc3339())?;

        for link in &self.links {
            write_link(&mut writer, link)?;
        }

        for entry in &self.entries {
            write_entry(&mut writer, entry)?;
        }

        writer.write_event(Event::End(BytesEnd::new("feed")))?;

        let buf = writer.into_inner().into_inner();
        Ok(String::from_utf8(buf).expect("XML output is valid UTF-8"))
    }
}

fn write_text_element(w: &mut Writer<Cursor<Vec<u8>>>, tag: &str, text: &str) -> Result<(), quick_xml::Error> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    w.write_event(Event::Text(BytesText::new(text)))?;
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

fn write_link(w: &mut Writer<Cursor<Vec<u8>>>, link: &AtomLink) -> Result<(), quick_xml::Error> {
    let mut el = BytesStart::new("link");
    el.push_attribute(("rel", link.rel.as_str()));
    el.push_attribute(("href", link.href.as_str()));
    if let Some(ref t) = link.link_type {
        el.push_attribute(("type", t.as_str()));
    }
    if let Some(ref title) = link.title {
        el.push_attribute(("title", title.as_str()));
    }
    w.write_event(Event::Empty(el))?;
    Ok(())
}

fn write_entry(w: &mut Writer<Cursor<Vec<u8>>>, entry: &AtomEntry) -> Result<(), quick_xml::Error> {
    w.write_event(Event::Start(BytesStart::new("entry")))?;

    write_text_element(w, "id", &entry.id)?;
    write_text_element(w, "title", &entry.title)?;
    write_text_element(w, "updated", &entry.updated.to_rfc3339())?;

    for author_name in &entry.authors {
        w.write_event(Event::Start(BytesStart::new("author")))?;
        write_text_element(w, "name", author_name)?;
        w.write_event(Event::End(BytesEnd::new("author")))?;
    }

    if let Some(ref content) = entry.content {
        let mut content_start = BytesStart::new("content");
        content_start.push_attribute(("type", "text"));
        w.write_event(Event::Start(content_start))?;
        w.write_event(Event::Text(BytesText::new(content)))?;
        w.write_event(Event::End(BytesEnd::new("content")))?;
    }

    for link in &entry.links {
        write_link(w, link)?;
    }

    w.write_event(Event::End(BytesEnd::new("entry")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn fixed_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
    }

    #[test]
    fn test_empty_navigation_feed() {
        let feed =
            AtomFeed::new("urn:bookboss:root", "BookBoss Catalog", fixed_time()).with_link(AtomLink::new(rel::SELF, "/opds/").with_type(mime::NAVIGATION));

        let xml = feed.to_xml().unwrap();
        assert!(xml.contains("xmlns=\"http://www.w3.org/2005/Atom\""));
        assert!(xml.contains("<title>BookBoss Catalog</title>"));
        assert!(xml.contains("rel=\"self\""));
        assert!(xml.contains("href=\"/opds/\""));
    }

    #[test]
    fn test_feed_with_entry() {
        let entry = AtomEntry::new("urn:bookboss:book:1", "Test Book", fixed_time())
            .with_author("Test Author")
            .with_content("A great book")
            .with_link(AtomLink::new(rel::ACQUISITION, "/opds/download/BK_abc/epub").with_type("application/epub+zip"));

        let feed = AtomFeed::new("urn:bookboss:all", "All Books", fixed_time()).with_entry(entry);

        let xml = feed.to_xml().unwrap();
        assert!(xml.contains("<entry>"));
        assert!(xml.contains("<title>Test Book</title>"));
        assert!(xml.contains("<name>Test Author</name>"));
        assert!(xml.contains("A great book"));
        assert!(xml.contains("application/epub+zip"));
    }

    #[test]
    fn test_xml_escapes_special_chars() {
        let entry = AtomEntry::new("urn:test", "Book & <Title>", fixed_time());
        let feed = AtomFeed::new("urn:feed", "Feed & <Test>", fixed_time()).with_entry(entry);

        let xml = feed.to_xml().unwrap();
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&lt;"));
    }

    #[test]
    fn test_multiple_authors() {
        let entry = AtomEntry::new("urn:test", "Book", fixed_time())
            .with_author("Author One")
            .with_author("Author Two");
        let feed = AtomFeed::new("urn:feed", "Feed", fixed_time()).with_entry(entry);

        let xml = feed.to_xml().unwrap();
        assert!(xml.contains("<name>Author One</name>"));
        assert!(xml.contains("<name>Author Two</name>"));
    }
}
