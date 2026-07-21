use async_trait::async_trait;
use bb_core::{
    Error,
    download::{DownloadCandidate, DownloadProvider, DownloadedFile},
};
use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use serde::Deserialize;

use crate::{MIRROR_DEFAULT, REQUEST_TIMEOUT};

/// Browser-like User-Agent — Anna's Archive rejects obvious bot clients.
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Ebook formats we know how to recognise in a search-result metadata line.
const KNOWN_FORMATS: [&str; 9] = ["epub", "pdf", "mobi", "azw3", "azw", "djvu", "cbz", "cbr", "fb2"];

/// Highest `domain_index` (download server) we will try before giving up.
///
/// The fast-download JSON API serves each file from one of several "Fast
/// Partner Server"s selected by `domain_index` (`0` = server #1). When a
/// server's host is unreachable we walk to the next index, so a single flaky
/// mirror no longer fails the whole download. Anna's Archive only runs a
/// handful of partner servers, so a small bound covers them all.
const MAX_DOMAIN_INDEX: u8 = 3;

/// HTTP adapter for Anna's Archive.
///
/// `search` scrapes the HTML search page (there is no official JSON search
/// API); `fetch` uses the JSON fast-download API, which requires a
/// donation-backed API key.
pub struct AnnasArchiveAdapter {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

/// Response shape of `GET /dyn/api/fast_download.json`.
#[derive(Debug, Deserialize)]
struct FastDownloadResponse {
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Outcome of a single-server (`domain_index`) download attempt that did not
/// yield a file.
enum AttemptError {
    /// The request failed for a reason no other server can fix — an invalid
    /// md5, a rejected key, or an exhausted quota. Surface it immediately.
    Permanent(Error),
    /// This particular server failed (unreachable host or bad status), but
    /// another `domain_index` may still work. Move on to the next server.
    Transient(Error),
}

impl AnnasArchiveAdapter {
    /// Build an adapter for the given mirror (falling back to
    /// [`MIRROR_DEFAULT`]) and API key.
    #[must_use]
    pub fn new(mirror_url: Option<&str>, api_key: impl Into<String>) -> Self {
        let base = mirror_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(|| MIRROR_DEFAULT.to_owned(), normalize_base_url);
        Self::with_base_url(base, api_key)
    }

    /// Build an adapter against an explicit, already-normalised base URL. Used
    /// by tests to point at a mock server.
    #[must_use]
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Resolve and download the file from a single download server, identified
    /// by `domain_index`.
    ///
    /// Returns [`AttemptError::Permanent`] when no other server could help
    /// (the API rejected the md5/key/quota), or [`AttemptError::Transient`]
    /// when this server failed but another `domain_index` might succeed.
    async fn fetch_from_server(&self, external_id: &str, domain_index: u8) -> Result<DownloadedFile, AttemptError> {
        // Step 1: resolve the time-limited download URL via the JSON API.
        let mut api_url = reqwest::Url::parse(&format!("{}/dyn/api/fast_download.json", self.base_url))
            .map_err(|e| AttemptError::Permanent(Error::Infrastructure(format!("Anna's Archive URL construction failed: {e}"))))?;
        api_url
            .query_pairs_mut()
            .append_pair("md5", external_id)
            .append_pair("key", &self.api_key)
            .append_pair("domain_index", &domain_index.to_string());

        let resp = self
            .client
            .get(api_url)
            .header(USER_AGENT, UA)
            .send()
            .await
            .map_err(|e| AttemptError::Transient(Error::Infrastructure(format!("Anna's Archive fast-download request failed: {e}"))))?;

        if !resp.status().is_success() {
            return Err(AttemptError::Transient(Error::Infrastructure(format!(
                "Anna's Archive fast-download returned status {}",
                resp.status()
            ))));
        }

        let payload: FastDownloadResponse = resp
            .json()
            .await
            .map_err(|e| AttemptError::Transient(Error::Infrastructure(format!("Anna's Archive returned invalid JSON: {e}"))))?;

        if let Some(err) = payload.error.filter(|e| !e.trim().is_empty()) {
            return Err(AttemptError::Permanent(Error::Validation(format!(
                "Anna's Archive refused the download: {err}"
            ))));
        }

        let download_url = payload
            .download_url
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| AttemptError::Permanent(Error::Validation("Anna's Archive returned no download URL".into())))?;

        // Step 2: download the actual file bytes.
        let file_resp = self
            .client
            .get(&download_url)
            .header(USER_AGENT, UA)
            .send()
            .await
            .map_err(|e| AttemptError::Transient(Error::Infrastructure(format!("download request failed: {e}"))))?;

        if !file_resp.status().is_success() {
            return Err(AttemptError::Transient(Error::Infrastructure(format!(
                "download returned status {}",
                file_resp.status()
            ))));
        }

        let bytes = file_resp
            .bytes()
            .await
            .map_err(|e| AttemptError::Transient(Error::Infrastructure(format!("reading download body failed: {e}"))))?;

        Ok(DownloadedFile {
            filename: format!("{external_id}.epub"),
            bytes: bytes.to_vec(),
        })
    }
}

#[async_trait]
impl DownloadProvider for AnnasArchiveAdapter {
    fn name(&self) -> &'static str {
        "Anna's Archive"
    }

    async fn search(&self, query: &str, language: Option<&str>) -> Result<Vec<DownloadCandidate>, Error> {
        let mut url = reqwest::Url::parse(&format!("{}/search", self.base_url))
            .map_err(|e| Error::Infrastructure(format!("Anna's Archive URL construction failed: {e}")))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("q", query).append_pair("ext", "epub").append_pair("content", "book_any");
            if let Some(lang) = language.map(str::trim).filter(|l| !l.is_empty()) {
                qp.append_pair("lang", lang);
            }
        }

        let resp = self
            .client
            .get(url)
            .header(USER_AGENT, UA)
            .send()
            .await
            .map_err(|e| Error::Infrastructure(format!("Anna's Archive search request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(Error::Infrastructure(format!("Anna's Archive search returned status {}", resp.status())));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| Error::Infrastructure(format!("reading Anna's Archive search body failed: {e}")))?;

        Ok(parse_search_html(&body))
    }

    async fn fetch(&self, external_id: &str) -> Result<DownloadedFile, Error> {
        // Walk the available download servers (`domain_index`) in order, falling
        // back to the next one whenever a server is unreachable, so a single
        // flaky partner host no longer fails the whole download.
        let mut last_transient: Option<Error> = None;
        for domain_index in 0..=MAX_DOMAIN_INDEX {
            match self.fetch_from_server(external_id, domain_index).await {
                Ok(file) => return Ok(file),
                Err(AttemptError::Permanent(e)) => {
                    // At the first server, an API-level rejection is about the
                    // file, key, or quota — no other server will help. At a
                    // later index it means we have run out of real servers, so
                    // surface the earlier failure that made us start iterating.
                    return match last_transient.take() {
                        Some(prev) if domain_index > 0 => Err(prev),
                        _ => Err(e),
                    };
                }
                Err(AttemptError::Transient(e)) => {
                    tracing::warn!(
                        external_id,
                        domain_index,
                        error = %e,
                        "Anna's Archive download server failed, trying next server"
                    );
                    last_transient = Some(e);
                }
            }
        }

        Err(last_transient.unwrap_or_else(|| Error::Infrastructure("all Anna's Archive download servers failed".into())))
    }
}

/// Normalise a mirror value into a scheme-qualified base URL with no trailing
/// slash. Accepts a full URL or a bare domain.
fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    }
}

/// Parse the Anna's Archive search-results HTML into EPUB download candidates.
///
/// This scrapes the page structure (there is no JSON search API), so it is
/// inherently best-effort and may need updating if the site markup changes.
/// Each result lives in a `div.max-w-full` block containing the title `/md5/`
/// link, a `div.text-gray-800` metadata line, and author/publisher search
/// links tagged with icon spans. Non-EPUB results are dropped.
fn parse_search_html(body: &str) -> Vec<DownloadCandidate> {
    let doc = Html::parse_document(body);
    // Selectors are static and known-valid.
    let block_sel = Selector::parse("div.max-w-full").expect("valid selector");
    let md5_sel = Selector::parse(r#"a[href^="/md5/"]"#).expect("valid selector");
    let meta_sel = Selector::parse("div.text-gray-800").expect("valid selector");
    let search_link_sel = Selector::parse(r#"a[href^="/search"]"#).expect("valid selector");
    let author_icon_sel = Selector::parse(r#"span[class*="mdi--user-edit"]"#).expect("valid selector");

    let mut out = Vec::new();
    for block in doc.select(&block_sel) {
        let Some(anchor) = block.select(&md5_sel).next() else { continue };
        let Some(external_id) = anchor.value().attr("href").and_then(extract_md5) else {
            continue;
        };

        let title = collapse_ws(&anchor.text().collect::<String>());
        if title.is_empty() {
            continue;
        }

        let meta = block.select(&meta_sel).next().map(|m| m.text().collect::<String>()).unwrap_or_default();
        let (language, format, size) = parse_meta_line(&meta);

        // Drop anything that clearly isn't an EPUB. When the format could not be
        // parsed we keep the result, since the query itself filtered to EPUB.
        if !format.is_empty() && format != "epub" {
            continue;
        }
        let format = if format.is_empty() { "epub".to_owned() } else { format };

        let authors = block
            .select(&search_link_sel)
            .filter(|a| a.select(&author_icon_sel).next().is_some())
            .map(|a| collapse_ws(&a.text().collect::<String>()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");

        out.push(DownloadCandidate {
            external_id,
            title,
            authors,
            publisher: None,
            language,
            format,
            size,
        });
    }
    out
}

/// Extract the MD5 hash from an `/md5/{hash}` href.
fn extract_md5(href: &str) -> Option<String> {
    let rest = href.strip_prefix("/md5/")?;
    let hash = rest.split(['/', '?', '#']).next().unwrap_or_default().trim();
    (!hash.is_empty()).then(|| hash.to_owned())
}

/// Parse a metadata line like `"English [en], epub, 1.4MB, 📗 Book (fiction)"`
/// into `(language, format, size)`.
fn parse_meta_line(line: &str) -> (Option<String>, String, Option<String>) {
    let language = line
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(code, _)| code.trim().to_owned())
        .filter(|s| !s.is_empty() && s.len() <= 8);

    let parts: Vec<&str> = line.split(',').map(str::trim).collect();

    let format = parts
        .iter()
        .find_map(|part| {
            let lower = part.to_lowercase();
            KNOWN_FORMATS
                .iter()
                .find(|f| lower == **f || lower.split_whitespace().any(|w| w == **f))
                .map(|f| (*f).to_owned())
        })
        .unwrap_or_default();

    let size = parts.iter().find(|p| is_size_token(p)).map(|p| (*p).to_owned());

    (language, format, size)
}

/// True if `s` looks like a human file size, e.g. `"1.4MB"` or `"820 KB"`.
fn is_size_token(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    ["kb", "mb", "gb", "tb"].iter().any(|unit| {
        lower
            .strip_suffix(unit)
            .map(str::trim)
            .is_some_and(|prefix| prefix.chars().next().is_some_and(|c| c.is_ascii_digit()))
    })
}

/// Collapse runs of whitespace and trim.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::*;

    const SEARCH_HTML: &str = r#"
    <html><body>
      <div class="flex">
        <a href="/md5/aaaa1111" class="custom-a block mr-2"><div><img src="x"></div></a>
        <div class="relative pl-4 grow max-w-full">
          <a href="/md5/aaaa1111" class="custom-a">Dune</a>
          <div class="truncate text-gray-800 text-sm">English [en], epub, 1.4MB, Book (fiction)</div>
          <div class="truncate text-sm"><a href="/search?q=x&termtype_1=author"><span class="icon-[mdi--user-edit]"></span>Frank Herbert</a></div>
          <div class="truncate text-sm"><a href="/search?q=y&termtype_1=publisher"><span class="icon-[mdi--company]"></span>Ace Books</a></div>
        </div>
      </div>
      <div class="flex">
        <a href="/md5/bbbb2222" class="custom-a block mr-2"><div><img src="x"></div></a>
        <div class="relative pl-4 grow max-w-full">
          <a href="/md5/bbbb2222" class="custom-a">Some PDF Book</a>
          <div class="truncate text-gray-800 text-sm">English [en], pdf, 5.0MB, Book (non-fiction)</div>
          <div class="truncate text-sm"><a href="/search?q=z&termtype_1=author"><span class="icon-[mdi--user-edit]"></span>Someone Else</a></div>
        </div>
      </div>
    </body></html>
    "#;

    #[test]
    fn normalize_base_url_adds_scheme_and_trims_slash() {
        assert_eq!(normalize_base_url("annas-archive.org"), "https://annas-archive.org");
        assert_eq!(normalize_base_url("https://example.com/"), "https://example.com");
        assert_eq!(normalize_base_url("http://127.0.0.1:8080"), "http://127.0.0.1:8080");
    }

    #[test]
    fn parse_meta_line_extracts_language_format_size() {
        let (lang, fmt, size) = parse_meta_line("English [en], epub, 1.4MB, Book (fiction)");
        assert_eq!(lang.as_deref(), Some("en"));
        assert_eq!(fmt, "epub");
        assert_eq!(size.as_deref(), Some("1.4MB"));
    }

    #[test]
    fn parse_search_html_keeps_epub_drops_others() {
        let results = parse_search_html(SEARCH_HTML);
        assert_eq!(results.len(), 1, "the PDF result must be filtered out");
        let r = &results[0];
        assert_eq!(r.external_id, "aaaa1111");
        assert_eq!(r.title, "Dune");
        assert_eq!(r.format, "epub");
        assert_eq!(r.language.as_deref(), Some("en"));
        assert_eq!(r.size.as_deref(), Some("1.4MB"));
        assert_eq!(r.authors, "Frank Herbert");
    }

    #[tokio::test]
    async fn search_hits_the_search_endpoint_and_parses_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("ext", "epub"))
            .and(query_param("lang", "en"))
            .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH_HTML))
            .mount(&server)
            .await;

        let adapter = AnnasArchiveAdapter::with_base_url(server.uri(), "testkey");
        let results = adapter.search("dune", Some("en")).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].external_id, "aaaa1111");
    }

    #[tokio::test]
    async fn fetch_resolves_fast_download_then_downloads_bytes() {
        let server = MockServer::start().await;
        let file_url = format!("{}/file.epub", server.uri());

        Mock::given(method("GET"))
            .and(path("/dyn/api/fast_download.json"))
            .and(query_param("md5", "aaaa1111"))
            .and(query_param("key", "testkey"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "download_url": file_url })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/file.epub"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PK\x03\x04epub-bytes".to_vec()))
            .mount(&server)
            .await;

        let adapter = AnnasArchiveAdapter::with_base_url(server.uri(), "testkey");
        let file = adapter.fetch("aaaa1111").await.unwrap();

        assert_eq!(file.filename, "aaaa1111.epub");
        assert_eq!(file.bytes, b"PK\x03\x04epub-bytes");
    }

    #[tokio::test]
    async fn fetch_falls_back_to_next_server_when_first_download_fails() {
        let server = MockServer::start().await;
        let bad_url = format!("{}/bad.epub", server.uri());
        let good_url = format!("{}/good.epub", server.uri());

        // Server #0 hands back a URL whose download fails...
        Mock::given(method("GET"))
            .and(path("/dyn/api/fast_download.json"))
            .and(query_param("domain_index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "download_url": bad_url })))
            .mount(&server)
            .await;
        // ...server #1 hands back a URL that serves the bytes.
        Mock::given(method("GET"))
            .and(path("/dyn/api/fast_download.json"))
            .and(query_param("domain_index", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "download_url": good_url })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/bad.epub"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/good.epub"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PK\x03\x04good".to_vec()))
            .mount(&server)
            .await;

        let adapter = AnnasArchiveAdapter::with_base_url(server.uri(), "testkey");
        let file = adapter.fetch("aaaa1111").await.unwrap();

        assert_eq!(file.filename, "aaaa1111.epub");
        assert_eq!(file.bytes, b"PK\x03\x04good");
    }

    #[tokio::test]
    async fn fetch_surfaces_download_error_after_exhausting_all_servers() {
        let server = MockServer::start().await;
        let bad_url = format!("{}/bad.epub", server.uri());

        // Every server hands back a URL whose download fails.
        Mock::given(method("GET"))
            .and(path("/dyn/api/fast_download.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "download_url": bad_url })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/bad.epub"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let adapter = AnnasArchiveAdapter::with_base_url(server.uri(), "testkey");
        let err = adapter.fetch("aaaa1111").await.unwrap_err();

        assert!(err.to_string().contains("download returned status 500"), "got: {err}");
    }

    #[tokio::test]
    async fn fetch_returns_validation_error_when_api_reports_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/dyn/api/fast_download.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "error": "Daily download limit exceeded" })))
            .mount(&server)
            .await;

        let adapter = AnnasArchiveAdapter::with_base_url(server.uri(), "testkey");
        let err = adapter.fetch("aaaa1111").await.unwrap_err();

        assert_eq!(err.kind(), bb_core::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("Daily download limit exceeded"));
    }
}
