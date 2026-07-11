use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::html_meta::{
    collect_meta_property_values, extract_attr, extract_html_lang, extract_json_ld_field,
    extract_json_ld_string_list, extract_link_rel, extract_meta_content, iter_json_ld_blocks,
};
use crate::html_util::{find_ci, strip_html_tags};
use crate::{Browser, BrowserOptions, ConvertOptions, PageToMarkdown};

/// MCP tool request schema
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub url: String,
    #[serde(default)]
    pub include_images: bool,
    #[serde(default)]
    pub keep_header: bool,
    #[serde(default)]
    pub main_content: bool,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub favor_precision: bool,
    #[serde(default)]
    pub favor_recall: bool,
    /// Include forum comments when detected (default true).
    #[serde(default = "default_true")]
    pub include_comments: bool,
    #[serde(default)]
    pub only_with_metadata: bool,
}

fn default_true() -> bool {
    true
}

impl Default for McpRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            include_images: false,
            keep_header: false,
            main_content: false,
            max_length: None,
            favor_precision: false,
            favor_recall: false,
            include_comments: true,
            only_with_metadata: false,
        }
    }
}

/// MCP tool response schema
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub url: String,
    pub markdown: String,
    #[serde(flatten)]
    pub meta: PageMetadata,
}

/// Metadata extracted from an HTML page.
/// Used by both the MCP server and the CLI `--format json` output.
#[derive(Debug, Default, Serialize, Clone)]
pub struct PageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Extraction confidence in `0.0..=1.0` (set when Markdown is available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_quality: Option<f64>,
    /// Coarse page type: `article`, `forum`, `product`, or `page`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    /// 64-bit simhash fingerprint of extracted text (hex), for near-duplicate detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl PageMetadata {
    /// Generate a YAML frontmatter block from the metadata.
    /// Returns `None` if no metadata fields are present.
    /// The block is wrapped in `---` delimiters and includes a `url` field if provided.
    pub fn to_frontmatter(&self, url: Option<&str>) -> Option<String> {
        let mut lines = Vec::new();

        if let Some(u) = url {
            lines.push(format!("url: \"{}\"", escape_yaml_string(u)));
        }
        if let Some(ref title) = self.title {
            lines.push(format!("title: \"{}\"", escape_yaml_string(title)));
        }
        if let Some(ref desc) = self.description {
            lines.push(format!("description: \"{}\"", escape_yaml_string(desc)));
        }
        if let Some(ref author) = self.author {
            lines.push(format!("author: \"{}\"", escape_yaml_string(author)));
        }
        if let Some(ref date) = self.published_date {
            lines.push(format!("published_date: \"{}\"", escape_yaml_string(date)));
        }
        if let Some(ref image) = self.image {
            lines.push(format!("image: \"{}\"", escape_yaml_string(image)));
        }
        if let Some(ref headline) = self.headline {
            lines.push(format!("headline: \"{}\"", escape_yaml_string(headline)));
        }
        if let Some(ref site) = self.site_name {
            lines.push(format!("site_name: \"{}\"", escape_yaml_string(site)));
        }
        if let Some(ref kw) = self.keywords {
            let items: Vec<String> = kw.iter().map(|k| format!("  - \"{}\"", escape_yaml_string(k))).collect();
            lines.push(format!("keywords:\n{}", items.join("\n")));
        }
        if let Some(ref cats) = self.categories {
            let items: Vec<String> = cats.iter().map(|c| format!("  - \"{}\"", escape_yaml_string(c))).collect();
            lines.push(format!("categories:\n{}", items.join("\n")));
        }
        if let Some(ref excerpt) = self.excerpt {
            lines.push(format!("excerpt: \"{}\"", escape_yaml_string(excerpt)));
        }
        if let Some(ref canonical) = self.canonical_url {
            lines.push(format!("canonical_url: \"{}\"", escape_yaml_string(canonical)));
        }
        if let Some(ref language) = self.language {
            lines.push(format!("language: \"{}\"", escape_yaml_string(language)));
        }
        if let Some(quality) = self.extraction_quality {
            lines.push(format!("extraction_quality: {:.2}", quality));
        }
        if let Some(ref page_type) = self.page_type {
            lines.push(format!("page_type: \"{}\"", escape_yaml_string(page_type)));
        }
        if let Some(ref fingerprint) = self.fingerprint {
            lines.push(format!("fingerprint: \"{}\"", escape_yaml_string(fingerprint)));
        }

        if lines.is_empty() {
            None
        } else {
            Some(format!("---\n{}\n---\n\n", lines.join("\n")))
        }
    }

    /// Attach extraction quality, page type, language fallback, and content fingerprint.
    pub fn with_content_signals(mut self, html: &str, markdown: &str) -> Self {
        self.extraction_quality = Some(PageToMarkdown::extraction_quality(html, markdown));
        self.page_type = Some(PageToMarkdown::detect_page_type(html).to_string());
        if self.language.is_none() {
            self.language = detect_content_language(markdown);
        }
        let plain = PageToMarkdown::to_plain_text(markdown);
        self.fingerprint = Some(content_fingerprint(&plain));
        self
    }

    /// Emit a Trafilatura-style CSV document (header + one data row).
    pub fn to_csv(&self, url: &str, text: &str) -> String {
        let mut out = String::from(
            "url,title,author,published_date,language,page_type,extraction_quality,fingerprint,text\n",
        );
        out.push_str(&csv_escape(url));
        out.push(',');
        out.push_str(&csv_escape(self.title.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_escape(self.author.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_escape(self.published_date.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_escape(self.language.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_escape(self.page_type.as_deref().unwrap_or("")));
        out.push(',');
        if let Some(q) = self.extraction_quality {
            out.push_str(&format!("{:.2}", q));
        }
        out.push(',');
        out.push_str(&csv_escape(self.fingerprint.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_escape(text));
        out.push('\n');
        out
    }

    /// Emit a Trafilatura-style TEI XML document for corpus pipelines.
    pub fn to_tei(&self, url: &str, text: &str) -> String {
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <TEI xmlns=\"http://www.tei-c.org/ns/1.0\">\n\
               <teiHeader>\n\
                 <fileDesc>\n\
                   <titleStmt>\n",
        );
        out.push_str("                     <title>");
        out.push_str(&xml_escape(self.title.as_deref().unwrap_or("")));
        out.push_str("</title>\n");
        if let Some(author) = &self.author {
            out.push_str("                     <author>");
            out.push_str(&xml_escape(author));
            out.push_str("</author>\n");
        }
        out.push_str(
            "                   </titleStmt>\n\
                   <publicationStmt>\n",
        );
        if let Some(site) = &self.site_name {
            out.push_str("                     <publisher>");
            out.push_str(&xml_escape(site));
            out.push_str("</publisher>\n");
        }
        if let Some(date) = &self.published_date {
            out.push_str("                     <date when=\"");
            out.push_str(&xml_escape(date));
            out.push_str("\">");
            out.push_str(&xml_escape(date));
            out.push_str("</date>\n");
        }
        out.push_str("                     <idno type=\"URL\">");
        out.push_str(&xml_escape(url));
        out.push_str("</idno>\n");
        if let Some(q) = self.extraction_quality {
            out.push_str("                     <idno type=\"extraction-quality\">");
            out.push_str(&format!("{:.2}", q));
            out.push_str("</idno>\n");
        }
        if let Some(pt) = &self.page_type {
            out.push_str("                     <idno type=\"page-type\">");
            out.push_str(&xml_escape(pt));
            out.push_str("</idno>\n");
        }
        if let Some(fp) = &self.fingerprint {
            out.push_str("                     <idno type=\"fingerprint\">");
            out.push_str(&xml_escape(fp));
            out.push_str("</idno>\n");
        }
        out.push_str(
            "                   </publicationStmt>\n\
                   <sourceDesc>\n\
                     <p>Converted from web page by web2md</p>\n\
                   </sourceDesc>\n\
                 </fileDesc>\n",
        );
        if let Some(lang) = &self.language {
            out.push_str(
                "                 <profileDesc>\n\
                   <langUsage>\n\
                     <language ident=\"",
            );
            out.push_str(&xml_escape(lang));
            out.push_str(
                "\"/>\n\
                   </langUsage>\n\
                 </profileDesc>\n",
            );
        }
        out.push_str(
            "               </teiHeader>\n\
               <text>\n\
                 <body>\n\
                   <div type=\"entry\">\n",
        );
        for para in tei_paragraphs(text) {
            out.push_str("                     <p>");
            out.push_str(&xml_escape(&para));
            out.push_str("</p>\n");
        }
        out.push_str(
            "                   </div>\n\
                 </body>\n\
               </text>\n\
             </TEI>\n",
        );
        out
    }

    /// Emit a Trafilatura-style plain XML document (`<doc>` with metadata + `<main>`).
    pub fn to_xml(&self, url: &str, text: &str) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<doc>\n");
        out.push_str("  <url>");
        out.push_str(&xml_escape(url));
        out.push_str("</url>\n");
        if let Some(title) = &self.title {
            out.push_str("  <title>");
            out.push_str(&xml_escape(title));
            out.push_str("</title>\n");
        }
        if let Some(author) = &self.author {
            out.push_str("  <author>");
            out.push_str(&xml_escape(author));
            out.push_str("</author>\n");
        }
        if let Some(date) = &self.published_date {
            out.push_str("  <date>");
            out.push_str(&xml_escape(date));
            out.push_str("</date>\n");
        }
        if let Some(site) = &self.site_name {
            out.push_str("  <sitename>");
            out.push_str(&xml_escape(site));
            out.push_str("</sitename>\n");
        }
        if let Some(desc) = &self.description {
            out.push_str("  <description>");
            out.push_str(&xml_escape(desc));
            out.push_str("</description>\n");
        }
        if let Some(lang) = &self.language {
            out.push_str("  <language>");
            out.push_str(&xml_escape(lang));
            out.push_str("</language>\n");
        }
        if let Some(pt) = &self.page_type {
            out.push_str("  <page_type>");
            out.push_str(&xml_escape(pt));
            out.push_str("</page_type>\n");
        }
        if let Some(q) = self.extraction_quality {
            out.push_str(&format!("  <extraction_quality>{:.2}</extraction_quality>\n", q));
        }
        if let Some(fp) = &self.fingerprint {
            out.push_str("  <fingerprint>");
            out.push_str(&xml_escape(fp));
            out.push_str("</fingerprint>\n");
        }
        out.push_str("  <main>\n");
        for para in tei_paragraphs(text) {
            out.push_str("    <p>");
            out.push_str(&xml_escape(&para));
            out.push_str("</p>\n");
        }
        out.push_str("  </main>\n</doc>\n");
        out
    }
}

/// Detect language from extracted text when HTML metadata has no language.
/// Returns an ISO 639-3 code when detection is reliable.
pub fn detect_content_language(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.chars().count() < 40 {
        return None;
    }
    let info = whatlang::detect(trimmed)?;
    if !info.is_reliable() {
        return None;
    }
    Some(info.lang().code().to_string())
}

/// 64-bit simhash fingerprint of normalized text (hex). Stable for near-duplicate detection.
pub fn content_fingerprint(text: &str) -> String {
    let tokens: Vec<String> = text
        .split_whitespace()
        .filter(|t| t.chars().count() > 2)
        .map(|t| t.to_lowercase())
        .collect();
    if tokens.is_empty() {
        return "0000000000000000".to_string();
    }
    let mut weights = [0i32; 64];
    for token in &tokens {
        let h = fnv1a64(token.as_bytes());
        for i in 0..64 {
            if (h >> i) & 1 == 1 {
                weights[i] += 1;
            } else {
                weights[i] -= 1;
            }
        }
    }
    let mut fingerprint = 0u64;
    for (i, w) in weights.iter().enumerate() {
        if *w > 0 {
            fingerprint |= 1u64 << i;
        }
    }
    format!("{:016x}", fingerprint)
}

/// True when `actual` language matches `target` (ISO 639-1 or 639-3, with region tags).
pub fn language_matches(actual: Option<&str>, target: &str) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    let a = normalize_lang_code(actual);
    let b = normalize_lang_code(target);
    if a == b {
        return true;
    }
    match (whatlang::Lang::from_code(&a), whatlang::Lang::from_code(&b)) {
        (Some(la), Some(lb)) => la == lb,
        _ => false,
    }
}

fn normalize_lang_code(code: &str) -> String {
    let primary = code
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or(code)
        .to_lowercase();
    match primary.as_str() {
        "en" => "eng".into(),
        "de" => "deu".into(),
        "fr" => "fra".into(),
        "es" => "spa".into(),
        "it" => "ita".into(),
        "pt" => "por".into(),
        "nl" => "nld".into(),
        "ru" => "rus".into(),
        "zh" => "zho".into(),
        "ja" => "jpn".into(),
        "ko" => "kor".into(),
        "ar" => "ara".into(),
        "pl" => "pol".into(),
        "tr" => "tur".into(),
        "vi" => "vie".into(),
        "sv" => "swe".into(),
        "da" => "dan".into(),
        "fi" => "fin".into(),
        "no" | "nb" | "nn" => "nor".into(),
        "cs" => "ces".into(),
        "uk" => "ukr".into(),
        "el" => "ell".into(),
        "he" => "heb".into(),
        "hi" => "hin".into(),
        "id" => "ind".into(),
        "th" => "tha".into(),
        "ro" => "ron".into(),
        "hu" => "hun".into(),
        other => other.to_string(),
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn csv_escape(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Split plain text into TEI `<p>` chunks (blank-line separated; fallback to whole text).
fn tei_paragraphs(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let paras: Vec<String> = trimmed
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect();
    if paras.is_empty() {
        vec![trimmed.to_string()]
    } else {
        paras
    }
}

/// Truncate `text` to `max` bytes and append a `[truncated]` marker.
pub fn truncate_with_marker(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}\n\n[truncated]", &text[..max])
    }
}

/// Extract metadata from HTML, then attach extraction quality and page type using Markdown.
pub fn extract_page_metadata(html: &str, markdown: &str) -> PageMetadata {
    extract_metadata(html).with_content_signals(html, markdown)
}

/// Escape double quotes and backslashes in a YAML string value.
fn escape_yaml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Extract metadata (title, description, author, publication date, image, headline, site name, keywords, categories, excerpt, canonical URL, language) from HTML.
pub fn extract_metadata(html: &str) -> PageMetadata {
    let title = extract_title(html).or_else(|| extract_dublin_core(html, "title"));
    let description = extract_meta_content(html, "name", "description")
        .or_else(|| extract_meta_content(html, "property", "og:description"))
        .or_else(|| extract_dublin_core(html, "description"));
    let author = extract_meta_content(html, "name", "author")
        .or_else(|| extract_json_ld_author(html))
        .or_else(|| extract_dublin_core(html, "creator"));
    let published_date = extract_published_date(html);
    let image = extract_meta_content(html, "property", "og:image")
        .or_else(|| extract_json_ld_image(html));
    let headline = extract_json_ld_field(html, "headline");
    let site_name = extract_meta_content(html, "property", "og:site_name");
    let keywords = extract_keywords(html);
    let categories = extract_categories(html);
    let excerpt = extract_excerpt(html);
    let canonical_url = extract_meta_content(html, "property", "og:url")
        .or_else(|| extract_link_rel(html, "canonical"));
    let language = extract_language(html);
    PageMetadata {
        title,
        description,
        author,
        published_date,
        image,
        headline,
        site_name,
        keywords,
        categories,
        excerpt,
        canonical_url,
        language,
        extraction_quality: None,
        page_type: None,
        fingerprint: None,
    }
}

/// MCP server wrapping the Browser
pub struct McpServer {
    browser: Browser,
}

impl McpServer {
    /// Create a new MCP server instance
    pub fn new() -> Result<Self> {
        let browser = Browser::new(BrowserOptions::default())?;
        Ok(Self { browser })
    }

    /// Handle a single MCP request: fetch URL and return Markdown
    pub async fn handle(&self, req: McpRequest) -> Result<McpResponse> {
        let html = self.browser.fetch(&req.url).await?;
        let html = self.browser.prepare_html(&html, &req.url).await?;
        let opts = ConvertOptions {
            include_images: req.include_images,
            keep_header: req.keep_header,
            main_content: req.main_content,
            favor_precision: req.favor_precision,
            favor_recall: req.favor_recall,
            include_comments: req.include_comments,
        };
        let mut markdown = PageToMarkdown::convert_with(&html, &opts, &[])?;
        markdown = PageToMarkdown::absolutize_links(&markdown, &req.url);

        if let Some(max) = req.max_length {
            markdown = truncate_with_marker(&markdown, max);
        }

        let meta = extract_page_metadata(&html, &markdown);
        if req.only_with_metadata && (meta.title.is_none() || meta.published_date.is_none()) {
            anyhow::bail!(
                "only_with_metadata requires title and published_date; found title={:?} published_date={:?}",
                meta.title.as_deref().unwrap_or("(missing)"),
                meta.published_date.as_deref().unwrap_or("(missing)")
            );
        }

        Ok(McpResponse {
            url: req.url,
            markdown,
            meta,
        })
    }
}

/// Extract publication date from HTML.
/// Checks in order: `<meta property="article:published_time">`, `<meta name="article:published_time">`,
/// `<time datetime="...">`, JSON-LD `"datePublished"`, then Dublin Core `DC.date` / `dcterms.date`.
fn extract_published_date(html: &str) -> Option<String> {
    extract_meta_content(html, "property", "article:published_time")
        .or_else(|| extract_meta_content(html, "name", "article:published_time"))
        .or_else(|| extract_time_datetime(html))
        .or_else(|| extract_json_ld_field(html, "datePublished"))
        .or_else(|| extract_dublin_core(html, "date"))
}

/// Dublin Core / DCTERMS meta fallback (`DC.{field}` or `dcterms.{field}`).
fn extract_dublin_core(html: &str, field: &str) -> Option<String> {
    extract_meta_content(html, "name", &format!("DC.{}", field))
        .or_else(|| extract_meta_content(html, "name", &format!("dcterms.{}", field)))
}

/// Extract `datetime` attribute from the first `<time>` tag.
fn extract_time_datetime(html: &str) -> Option<String> {
    let pos = find_ci(html, "<time")?;
    let tag_end = html[pos..].find('>').map(|e| pos + e)?;
    let tag = &html[pos..=tag_end];
    extract_attr(tag, "datetime")
}

/// Extract `author` from JSON-LD `<script type="application/ld+json">` blocks.
/// Handles both string authors and `{"@type":"Person","name":"..."}` object authors.
fn extract_json_ld_author(html: &str) -> Option<String> {
    for json in iter_json_ld_blocks(html) {
        if let Some(author) = json.get("author") {
            if let Some(name) = author.as_str() {
                return Some(name.to_string());
            }
            if let Some(name) = author.get("name").and_then(|v| v.as_str()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Extract `image` from JSON-LD `<script type="application/ld+json">` blocks.
/// Handles string URLs, `{"url":"..."}` objects, and arrays of either (first item used).
fn extract_json_ld_image(html: &str) -> Option<String> {
    for json in iter_json_ld_blocks(html) {
        if let Some(image) = json.get("image") {
            if let Some(url) = image.as_str() {
                return Some(url.to_string());
            }
            if let Some(url) = image.get("url").and_then(|v| v.as_str()) {
                return Some(url.to_string());
            }
            if let Some(arr) = image.as_array() {
                if let Some(first) = arr.first() {
                    if let Some(url) = first.as_str() {
                        return Some(url.to_string());
                    }
                    if let Some(url) = first.get("url").and_then(|v| v.as_str()) {
                        return Some(url.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Extract keywords/tags from HTML.
/// Checks in order: multiple `<meta property="article:tag">` tags, `<meta name="keywords">`,
/// and JSON-LD `keywords` (string, array, or comma-separated).
fn extract_keywords(html: &str) -> Option<Vec<String>> {
    let keywords = collect_meta_property_values(html, "article:tag");
    if !keywords.is_empty() {
        return Some(keywords);
    }

    if let Some(kw_str) = extract_meta_content(html, "name", "keywords") {
        let tags: Vec<String> = kw_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !tags.is_empty() {
            return Some(tags);
        }
    }

    extract_json_ld_string_list(html, "keywords", true)
}

/// Extract article categories/sections from HTML.
/// Checks in order: multiple `<meta property="article:section">` tags,
/// then JSON-LD `articleSection` (string or array).
fn extract_categories(html: &str) -> Option<Vec<String>> {
    let categories = collect_meta_property_values(html, "article:section");
    if !categories.is_empty() {
        return Some(categories);
    }
    extract_json_ld_string_list(html, "articleSection", false)
}

/// Extract `<title>` text from HTML.
fn extract_title(html: &str) -> Option<String> {
    find_ci(html, "<title>").and_then(|start| {
        let rest = &html[start + 7..];
        find_ci(rest, "</title>").map(|end| rest[..end].trim().to_string())
    })
}

const EXCERPT_MAX_LEN: usize = 160;
const MIN_EXCERPT_PARAGRAPH_LEN: usize = 40;

/// Build a short excerpt from the first substantive paragraph in the page body.
fn extract_excerpt(html: &str) -> Option<String> {
    first_paragraph_text(html).map(|text| truncate_excerpt(&text))
}

fn first_paragraph_text(html: &str) -> Option<String> {
    let mut i = 0;
    while i < html.len() {
        if let Some(pos) = find_ci(&html[i..], "<p") {
            let pos = i + pos;
            let tag_end = html[pos..].find('>').map(|e| pos + e)?;
            let close = find_ci(&html[tag_end + 1..], "</p>").map(|c| tag_end + 1 + c)?;
            let inner = &html[tag_end + 1..close];
            let text = strip_html_tags(inner);
            let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if trimmed.len() >= MIN_EXCERPT_PARAGRAPH_LEN {
                return Some(trimmed);
            }
            i = close + 4;
        } else {
            break;
        }
    }
    None
}

fn truncate_excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= EXCERPT_MAX_LEN {
        return trimmed.to_string();
    }

    let mut end = EXCERPT_MAX_LEN;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    while end > EXCERPT_MAX_LEN / 2 && !trimmed[..end].ends_with(' ') {
        end -= 1;
    }
    if end <= EXCERPT_MAX_LEN / 2 {
        end = EXCERPT_MAX_LEN;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
    }
    format!("{}…", trimmed[..end].trim_end())
}

fn extract_language(html: &str) -> Option<String> {
    extract_html_lang(html)
        .or_else(|| extract_meta_content(html, "property", "og:locale"))
        .or_else(|| extract_json_ld_language(html))
        .map(normalize_language_tag)
}

fn normalize_language_tag(tag: String) -> String {
    tag.replace('_', "-")
}

fn extract_json_ld_language(html: &str) -> Option<String> {
    for json in iter_json_ld_blocks(html) {
        if let Some(lang) = json.get("inLanguage") {
            if let Some(s) = lang.as_str() {
                return Some(s.to_string());
            }
            if let Some(s) = lang.get("name").and_then(|v| v.as_str()) {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_server_fetch_and_convert() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/doc")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><head><title>Test Page</title></head><body><h1>Hello</h1><p>World</p></body></html>")
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/doc", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.title, Some("Test Page".to_string()));
        assert!(resp.markdown.contains("Hello"));
        assert!(resp.markdown.contains("World"));
        assert_eq!(resp.meta.published_date, None);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_extracts_metadata() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/meta")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head>
                <title>Article Title</title>
                <meta name="description" content="A test article about Rust">
                <meta name="author" content="Jane Doe">
                <meta property="og:description" content="OG description override">
                <meta property="article:published_time" content="2025-01-15T08:30:00Z">
            </head><body><p>Content</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/meta", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.title, Some("Article Title".to_string()));
        assert_eq!(resp.meta.description, Some("A test article about Rust".to_string()));
        assert_eq!(resp.meta.author, Some("Jane Doe".to_string()));
        assert_eq!(resp.meta.published_date, Some("2025-01-15T08:30:00Z".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_og_description_fallback() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/og")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head>
                <title>OG Page</title>
                <meta property="og:description" content="OG only description">
            </head><body><p>Body</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/og", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.description, Some("OG only description".to_string()));
        assert_eq!(resp.meta.author, None);
        assert_eq!(resp.meta.published_date, None);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_no_metadata_returns_none() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/bare")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body><p>No metadata here</p></body></html>")
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/bare", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.title, None);
        assert_eq!(resp.meta.description, None);
        assert_eq!(resp.meta.author, None);
        assert_eq!(resp.meta.published_date, None);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_extracts_date_from_time_tag() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/time")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head><title>Time Article</title></head><body><article><time datetime="2024-06-01">June 1, 2024</time><p>Article body.</p></article></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/time", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.published_date, Some("2024-06-01".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_extracts_date_from_json_ld() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/jsonld")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head><title>JSON-LD Article</title><script type="application/ld+json">{"@type":"NewsArticle","headline":"Test","datePublished":"2023-12-25T10:00:00+00:00","author":{"@type":"Person","name":"John"}}</script></head><body><p>Content.</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/jsonld", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.published_date, Some("2023-12-25T10:00:00+00:00".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mcp_server_meta_date_takes_priority() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/priority")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<html><head><meta property="article:published_time" content="2025-03-20T12:00:00Z"></head><body><time datetime="2024-01-01">Old date</time><p>Content.</p></body></html>"#)
            .create_async()
            .await;

        let mcp = McpServer::new().unwrap();
        let resp = mcp
            .handle(McpRequest {
                url: format!("{}/priority", server.url()),
                include_images: false,
                keep_header: false,
                main_content: false,
                max_length: None,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(resp.meta.published_date, Some("2025-03-20T12:00:00Z".to_string()));
        mock.assert_async().await;
    }

    #[test]
    fn extract_metadata_all_fields() {
        let html = r#"<html><head>
            <title>Test Article</title>
            <meta name="description" content="A description">
            <meta name="author" content="Jane Doe">
            <meta property="article:published_time" content="2025-01-15T08:30:00Z">
        </head><body><p>Content</p></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.title, Some("Test Article".to_string()));
        assert_eq!(meta.description, Some("A description".to_string()));
        assert_eq!(meta.author, Some("Jane Doe".to_string()));
        assert_eq!(meta.published_date, Some("2025-01-15T08:30:00Z".to_string()));
    }

    #[test]
    fn extract_metadata_no_fields_returns_none() {
        let html = "<html><body><p>No metadata</p></body></html>";
        let meta = extract_metadata(html);
        assert_eq!(meta.title, None);
        assert_eq!(meta.description, None);
        assert_eq!(meta.author, None);
        assert_eq!(meta.published_date, None);
    }

    #[test]
    fn extract_metadata_json_ld_author_string() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","author":"John Smith","datePublished":"2025-01-01"}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.author, Some("John Smith".to_string()));
    }

    #[test]
    fn extract_metadata_json_ld_author_object() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","author":{"@type":"Person","name":"Alice Jones"}}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.author, Some("Alice Jones".to_string()));
    }

    #[test]
    fn extract_metadata_meta_author_takes_priority_over_json_ld() {
        let html = r#"<html><head>
            <meta name="author" content="Meta Author">
            <script type="application/ld+json">{"author":"JSON-LD Author"}</script>
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.author, Some("Meta Author".to_string()));
    }

    #[test]
    fn extract_metadata_json_ld_author_fallback_when_no_meta() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"NewsArticle","author":{"@type":"Person","name":"From JSON-LD"}}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.author, Some("From JSON-LD".to_string()));
    }

    #[test]
    fn extract_metadata_og_image_meta_tag() {
        let html = r#"<html><head>
            <meta property="og:image" content="https://example.com/cover.jpg">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.image, Some("https://example.com/cover.jpg".to_string()));
    }

    #[test]
    fn extract_metadata_json_ld_image_string() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","image":"https://example.com/img.png"}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.image, Some("https://example.com/img.png".to_string()));
    }

    #[test]
    fn extract_metadata_json_ld_image_object() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","image":{"@type":"ImageObject","url":"https://example.com/photo.jpg"}}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.image, Some("https://example.com/photo.jpg".to_string()));
    }

    #[test]
    fn extract_metadata_json_ld_image_array() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","image":["https://example.com/first.jpg","https://example.com/second.jpg"]}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.image, Some("https://example.com/first.jpg".to_string()));
    }

    #[test]
    fn extract_metadata_og_image_takes_priority_over_json_ld() {
        let html = r#"<html><head>
            <meta property="og:image" content="https://example.com/og.jpg">
            <script type="application/ld+json">{"image":"https://example.com/jsonld.jpg"}</script>
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.image, Some("https://example.com/og.jpg".to_string()));
    }

    #[test]
    fn extract_metadata_json_ld_headline() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"NewsArticle","headline":"Breaking News Story"}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.headline, Some("Breaking News Story".to_string()));
    }

    #[test]
    fn extract_metadata_no_image_or_headline_returns_none() {
        let html = "<html><body><p>No metadata</p></body></html>";
        let meta = extract_metadata(html);
        assert_eq!(meta.image, None);
        assert_eq!(meta.headline, None);
    }

    #[test]
    fn extract_metadata_og_site_name() {
        let html = r#"<html><head><meta property="og:site_name" content="Tech Blog"></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.site_name, Some("Tech Blog".to_string()));
    }

    #[test]
    fn extract_metadata_no_site_name_returns_none() {
        let html = "<html><body><p>No metadata</p></body></html>";
        let meta = extract_metadata(html);
        assert_eq!(meta.site_name, None);
    }

    #[test]
    fn extract_metadata_article_tags() {
        let html = r#"<html><head>
            <meta property="article:tag" content="Rust">
            <meta property="article:tag" content="Web Scraping">
            <meta property="article:tag" content="Markdown">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.keywords, Some(vec![
            "Rust".to_string(),
            "Web Scraping".to_string(),
            "Markdown".to_string(),
        ]));
    }

    #[test]
    fn extract_metadata_meta_keywords_fallback() {
        let html = r#"<html><head><meta name="keywords" content="python, web scraping, automation"></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.keywords, Some(vec![
            "python".to_string(),
            "web scraping".to_string(),
            "automation".to_string(),
        ]));
    }

    #[test]
    fn extract_metadata_json_ld_keywords_string() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","keywords":"rust, async, tokio"}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.keywords, Some(vec![
            "rust".to_string(),
            "async".to_string(),
            "tokio".to_string(),
        ]));
    }

    #[test]
    fn extract_metadata_json_ld_keywords_array() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","keywords":["rust","async","tokio"]}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.keywords, Some(vec![
            "rust".to_string(),
            "async".to_string(),
            "tokio".to_string(),
        ]));
    }

    #[test]
    fn extract_metadata_article_tag_takes_priority_over_meta_keywords() {
        let html = r#"<html><head>
            <meta property="article:tag" content="Primary Tag">
            <meta name="keywords" content="fallback, tags">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.keywords, Some(vec!["Primary Tag".to_string()]));
    }

    #[test]
    fn extract_metadata_no_keywords_returns_none() {
        let html = "<html><body><p>No metadata</p></body></html>";
        let meta = extract_metadata(html);
        assert_eq!(meta.keywords, None);
    }

    #[test]
    fn extract_metadata_article_section() {
        let html = r#"<html><head>
            <meta property="article:section" content="Technology">
            <meta property="article:section" content="Open Source">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(
            meta.categories,
            Some(vec!["Technology".to_string(), "Open Source".to_string()])
        );
    }

    #[test]
    fn extract_metadata_json_ld_article_section_string() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"Article","articleSection":"Science"}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.categories, Some(vec!["Science".to_string()]));
    }

    #[test]
    fn extract_metadata_json_ld_article_section_array() {
        let html = r#"<html><head><script type="application/ld+json">{"@type":"NewsArticle","articleSection":["Politics","World"]}</script></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(
            meta.categories,
            Some(vec!["Politics".to_string(), "World".to_string()])
        );
    }

    #[test]
    fn extract_metadata_article_section_takes_priority_over_json_ld() {
        let html = r#"<html><head>
            <meta property="article:section" content="From Meta">
            <script type="application/ld+json">{"articleSection":"From JSON-LD"}</script>
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.categories, Some(vec!["From Meta".to_string()]));
    }

    #[test]
    fn extract_metadata_no_categories_returns_none() {
        let html = "<html><body><p>No metadata</p></body></html>";
        let meta = extract_metadata(html);
        assert_eq!(meta.categories, None);
    }

    #[test]
    fn extract_metadata_excerpt_from_first_paragraph() {
        let html = r#"<html><body><p>This is the opening paragraph with enough words to qualify as a page excerpt for agents and citations.</p><p>Second paragraph.</p></body></html>"#;
        let meta = extract_metadata(html);
        assert!(meta
            .excerpt
            .unwrap()
            .starts_with("This is the opening paragraph"));
    }

    #[test]
    fn extract_metadata_canonical_url_prefers_og_url() {
        let html = r#"<html><head>
            <meta property="og:url" content="https://example.com/og">
            <link rel="canonical" href="https://example.com/canonical">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.canonical_url, Some("https://example.com/og".to_string()));
    }

    #[test]
    fn extract_metadata_canonical_url_falls_back_to_link_rel() {
        let html = r#"<html><head><link rel="canonical" href="https://example.com/article"></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(
            meta.canonical_url,
            Some("https://example.com/article".to_string())
        );
    }

    #[test]
    fn extract_metadata_language_from_html_lang() {
        let html = r#"<html lang="en"><body><p>Hello world with enough text to become an excerpt field here today.</p></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.language, Some("en".to_string()));
    }

    #[test]
    fn extract_metadata_language_from_og_locale() {
        let html = r#"<html><head><meta property="og:locale" content="en_US"></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.language, Some("en-US".to_string()));
    }

    #[test]
    fn extract_metadata_dublin_core_fallbacks() {
        let html = r#"<html><head>
            <meta name="DC.title" content="DC Title">
            <meta name="DC.creator" content="DC Author">
            <meta name="DC.date" content="2026-07-11">
            <meta name="DC.description" content="DC Description">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.title, Some("DC Title".to_string()));
        assert_eq!(meta.author, Some("DC Author".to_string()));
        assert_eq!(meta.published_date, Some("2026-07-11".to_string()));
        assert_eq!(meta.description, Some("DC Description".to_string()));
    }

    #[test]
    fn extract_metadata_dcterms_creator_fallback() {
        let html = r#"<html><head><meta name="dcterms.creator" content="Terms Author"></head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.author, Some("Terms Author".to_string()));
    }

    #[test]
    fn extract_metadata_meta_author_takes_priority_over_dublin_core() {
        let html = r#"<html><head>
            <meta name="author" content="Meta Author">
            <meta name="DC.creator" content="DC Author">
        </head><body></body></html>"#;
        let meta = extract_metadata(html);
        assert_eq!(meta.author, Some("Meta Author".to_string()));
    }

    #[test]
    fn extract_page_metadata_includes_quality_and_type() {
        let html = r#"<html><head><title>Story</title>
            <meta name="author" content="Jane">
            <meta property="article:published_time" content="2026-07-11">
        </head><body><article><h1>Story</h1>
        <p>This is a substantial article body with enough text for a confident extraction score that agents can trust when deciding whether to fall back to an LLM.</p>
        </article></body></html>"#;
        let md = PageToMarkdown::convert(html, false, false, true, &[]).unwrap();
        let meta = extract_page_metadata(html, &md);
        assert_eq!(meta.page_type.as_deref(), Some("article"));
        let quality = meta.extraction_quality.unwrap();
        assert!(quality >= 0.5, "expected meaningful quality, got {quality}");
        assert!(quality <= 1.0);
        let fp = meta.fingerprint.expect("fingerprint");
        assert_eq!(fp.len(), 16);
    }

    #[test]
    fn detect_content_language_english() {
        let text = "This is a longer English paragraph used to verify that language detection \
            can identify the language of extracted article text when HTML metadata is missing.";
        let lang = detect_content_language(text).expect("should detect English");
        assert_eq!(lang, "eng");
    }

    #[test]
    fn detect_content_language_skips_short_text() {
        assert_eq!(detect_content_language("Hi"), None);
    }

    #[test]
    fn to_csv_emits_header_and_escaped_row() {
        let meta = PageMetadata {
            title: Some("Hello, \"World\"".to_string()),
            author: Some("Ada".to_string()),
            language: Some("eng".to_string()),
            page_type: Some("article".to_string()),
            extraction_quality: Some(0.9),
            ..Default::default()
        };
        let csv = meta.to_csv("https://example.com/a", "Line one\nLine two");
        assert!(csv.starts_with(
            "url,title,author,published_date,language,page_type,extraction_quality,fingerprint,text\n"
        ));
        assert!(csv.contains("\"Hello, \"\"World\"\"\""));
        assert!(csv.contains("Ada"));
        assert!(csv.contains("0.90"));
        assert!(csv.contains("\"Line one\nLine two\""));
    }

    #[test]
    fn content_fingerprint_stable_and_differs() {
        let a = content_fingerprint(
            "The quick brown fox jumps over the lazy dog near the river bank today.",
        );
        let b = content_fingerprint(
            "The quick brown fox jumps over the lazy dog near the river bank today.",
        );
        let c = content_fingerprint(
            "Completely different prose about quantum computing and satellite networks worldwide.",
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert_ne!(a, c);
    }

    #[test]
    fn language_matches_iso6391_and_6393() {
        assert!(language_matches(Some("en"), "eng"));
        assert!(language_matches(Some("eng"), "en"));
        assert!(language_matches(Some("en-US"), "en"));
        assert!(!language_matches(Some("fra"), "eng"));
        assert!(!language_matches(None, "eng"));
    }

    #[test]
    fn to_xml_emits_doc_with_main() {
        let meta = PageMetadata {
            title: Some("Hello <World>".to_string()),
            author: Some("Ada".to_string()),
            language: Some("en".to_string()),
            fingerprint: Some("abcd1234abcd1234".to_string()),
            ..Default::default()
        };
        let xml = meta.to_xml("https://example.com/a", "First paragraph.\n\nSecond paragraph.");
        assert!(xml.contains("<doc>"));
        assert!(xml.contains("<title>Hello &lt;World&gt;</title>"));
        assert!(xml.contains("<author>Ada</author>"));
        assert!(xml.contains("<language>en</language>"));
        assert!(xml.contains("<fingerprint>abcd1234abcd1234</fingerprint>"));
        assert!(xml.contains("<main>"));
        assert!(xml.contains("<p>First paragraph.</p>"));
        assert!(xml.contains("<p>Second paragraph.</p>"));
    }

    #[test]
    fn to_tei_emits_header_and_escaped_paragraphs() {
        let meta = PageMetadata {
            title: Some("Hello <World>".to_string()),
            author: Some("Ada & Bob".to_string()),
            site_name: Some("Example Site".to_string()),
            published_date: Some("2026-01-15".to_string()),
            language: Some("en".to_string()),
            page_type: Some("article".to_string()),
            extraction_quality: Some(0.85),
            ..Default::default()
        };
        let tei = meta.to_tei(
            "https://example.com/a?q=1&x=2",
            "First paragraph.\n\nSecond <para> with & ampersand.",
        );
        assert!(tei.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(tei.contains("<TEI xmlns=\"http://www.tei-c.org/ns/1.0\">"));
        assert!(tei.contains("<title>Hello &lt;World&gt;</title>"));
        assert!(tei.contains("<author>Ada &amp; Bob</author>"));
        assert!(tei.contains("<publisher>Example Site</publisher>"));
        assert!(tei.contains("when=\"2026-01-15\""));
        assert!(tei.contains("<idno type=\"URL\">https://example.com/a?q=1&amp;x=2</idno>"));
        assert!(tei.contains("<idno type=\"extraction-quality\">0.85</idno>"));
        assert!(tei.contains("<idno type=\"page-type\">article</idno>"));
        assert!(tei.contains("<language ident=\"en\"/>"));
        assert!(tei.contains("<div type=\"entry\">"));
        assert!(tei.contains("<p>First paragraph.</p>"));
        assert!(tei.contains("<p>Second &lt;para&gt; with &amp; ampersand.</p>"));
    }

    #[test]
    fn frontmatter_includes_all_fields() {
        let meta = PageMetadata {
            title: Some("Test Title".to_string()),
            description: Some("A description".to_string()),
            author: Some("Author Name".to_string()),
            published_date: Some("2025-07-04T12:00:00Z".to_string()),
            image: Some("https://example.com/img.jpg".to_string()),
            headline: Some("Breaking News".to_string()),
            site_name: Some("Tech Blog".to_string()),
            keywords: Some(vec!["Rust".to_string(), "Markdown".to_string()]),
            categories: Some(vec!["Technology".to_string(), "Rust".to_string()]),
            excerpt: Some("Short summary".to_string()),
            canonical_url: Some("https://example.com/page".to_string()),
            language: Some("en".to_string()),
            extraction_quality: Some(0.85),
            page_type: Some("article".to_string()),
            fingerprint: Some("deadbeefcafebabe".to_string()),
        };
        let fm = meta.to_frontmatter(Some("https://example.com/page")).unwrap();
        assert!(fm.starts_with("---\n"));
        assert!(fm.contains("url: \"https://example.com/page\""));
        assert!(fm.contains("title: \"Test Title\""));
        assert!(fm.contains("description: \"A description\""));
        assert!(fm.contains("author: \"Author Name\""));
        assert!(fm.contains("published_date: \"2025-07-04T12:00:00Z\""));
        assert!(fm.contains("image: \"https://example.com/img.jpg\""));
        assert!(fm.contains("headline: \"Breaking News\""));
        assert!(fm.contains("site_name: \"Tech Blog\""));
        assert!(fm.contains("keywords:\n  - \"Rust\"\n  - \"Markdown\""));
        assert!(fm.contains("categories:\n  - \"Technology\"\n  - \"Rust\""));
        assert!(fm.contains("excerpt: \"Short summary\""));
        assert!(fm.contains("canonical_url: \"https://example.com/page\""));
        assert!(fm.contains("language: \"en\""));
        assert!(fm.contains("extraction_quality: 0.85"));
        assert!(fm.contains("page_type: \"article\""));
        assert!(fm.contains("fingerprint: \"deadbeefcafebabe\""));
        assert!(fm.ends_with("---\n\n"));
    }

    #[test]
    fn frontmatter_without_url() {
        let meta = PageMetadata {
            title: Some("Title Only".to_string()),
            ..Default::default()
        };
        let fm = meta.to_frontmatter(None).unwrap();
        assert!(fm.contains("title: \"Title Only\""));
        assert!(!fm.contains("url:"));
    }

    #[test]
    fn frontmatter_empty_metadata_returns_none() {
        let meta = PageMetadata::default();
        assert!(meta.to_frontmatter(None).is_none());
    }

    #[test]
    fn frontmatter_escapes_quotes() {
        let meta = PageMetadata {
            title: Some("Title with \"quotes\"".to_string()),
            ..Default::default()
        };
        let fm = meta.to_frontmatter(None).unwrap();
        assert!(fm.contains("title: \"Title with \\\"quotes\\\"\""));
    }

    #[test]
    fn frontmatter_escapes_backslashes() {
        let meta = PageMetadata {
            title: Some("Path\\with\\backslashes".to_string()),
            ..Default::default()
        };
        let fm = meta.to_frontmatter(None).unwrap();
        assert!(fm.contains("title: \"Path\\\\with\\\\backslashes\""));
    }
}
