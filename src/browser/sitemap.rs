//! Sitemap XML parsing (sitemapindex + urlset `<loc>` discovery).

/// Parse URLs from sitemap XML content.
/// Extracts all `<loc>` tag values from sitemap.xml format.
pub fn parse_sitemap_urls(xml: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut pos = 0;
    while pos < xml.len() {
        if let Some(start) = xml[pos..].find("<loc>") {
            let start = pos + start + 5;
            if let Some(end) = xml[start..].find("</loc>") {
                let url = xml[start..start + end].trim().to_string();
                if !url.is_empty() {
                    urls.push(url);
                }
                pos = start + end + 6;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    urls
}
