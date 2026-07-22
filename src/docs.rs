//! Library documentation fetcher — a poor-person's Context7 for any registry.
//!
//! Fetches README and metadata from crates.io, docs.rs, npm, and PyPI
//! without any API key. Each registry has a different strategy:
//!
//! - **crates.io**: JSON API → repository URL → fetch README from docs.rs
//! - **docs.rs**: Direct README URL (`/crate/<name>/latest/source/README.md`)
//! - **npm**: JSON API → `readme` field in the response body
//! - **PyPI**: JSON API → `info.description` (long description, usually README)
//!
//! All responses are converted to clean Markdown.

use serde::{Deserialize, Serialize};

use crate::html_to_md::parse_html;

/// Which package registry to fetch from.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Registry {
    /// crates.io (Rust packages)
    Crates,
    /// docs.rs (Rust documentation)
    Docsrs,
    /// npmjs.com (JavaScript packages)
    Npm,
    /// pypi.org (Python packages)
    Pypi,
}

/// Metadata for a library doc result.
#[derive(Debug, Serialize)]
pub struct DocResult {
    pub registry: &'static str,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub readme: String,
}

/// Build the fetch URL for a given registry and package name.
/// Returns (url, is_json) — `is_json` indicates the response is JSON
/// rather than text/HTML.
pub fn registry_url(registry: Registry, name: &str) -> (String, bool) {
    match registry {
        Registry::Crates => (
            format!("https://crates.io/api/v1/crates/{}", name),
            true,
        ),
        Registry::Docsrs => (
            format!(
                "https://docs.rs/crate/{}/latest/source/README.md",
                name
            ),
            false,
        ),
        Registry::Npm => (
            format!("https://registry.npmjs.org/{}", name),
            true,
        ),
        Registry::Pypi => (
            format!("https://pypi.org/pypi/{}/json", name),
            true,
        ),
    }
}

/// Parse a crates.io JSON API response into a `DocResult`.
pub fn parse_crates_io(json: &str, name: &str) -> Result<DocResult, String> {
    #[derive(Deserialize)]
    struct CratesResponse {
        #[serde(rename = "crate")]
        crate_data: CrateData,
    }
    #[derive(Deserialize)]
    struct CrateData {
        max_version: Option<String>,
        description: Option<String>,
        repository: Option<String>,
        homepage: Option<String>,
        documentation: Option<String>,
        license: Option<String>,
    }

    let resp: CratesResponse =
        serde_json::from_str(json).map_err(|e| format!("crates.io JSON parse error: {}", e))?;

    let c = resp.crate_data;
    Ok(DocResult {
        registry: "crates.io",
        name: name.to_string(),
        version: c.max_version,
        description: c.description,
        repository: c.repository,
        homepage: c.homepage,
        documentation: c.documentation,
        license: c.license,
        readme: String::new(),
    })
}

/// Parse an npm JSON API response into a `DocResult`.
pub fn parse_npm(json: &str, name: &str) -> Result<DocResult, String> {
    let val: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("npm JSON parse error: {}", e))?;

    let latest = val
        .get("dist-tags")
        .and_then(|d| d.get("latest"))
        .and_then(|v| v.as_str());

    let info = val
        .get("versions")
        .and_then(|v| v.get(latest.unwrap_or("latest")))
        .or(Some(&val));

    let readme = val
        .get("readme")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let readme_md = if readme.contains('<') {
        parse_html(readme)
    } else {
        readme.to_string()
    };

    Ok(DocResult {
        registry: "npm",
        name: name.to_string(),
        version: latest.map(|s| s.to_string()),
        description: info
            .and_then(|i| i.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        repository: info
            .and_then(|i| i.get("repository"))
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()))
            }),
        homepage: info
            .and_then(|i| i.get("homepage"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        documentation: None,
        license: info
            .and_then(|i| i.get("license"))
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.get("type").and_then(|t| t.as_str()).map(|s| s.to_string()))
            }),
        readme: readme_md,
    })
}

/// Parse a PyPI JSON API response into a `DocResult`.
pub fn parse_pypi(json: &str, name: &str) -> Result<DocResult, String> {
    let val: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("pypi JSON parse error: {}", e))?;

    let info = val
        .get("info")
        .ok_or_else(|| "pypi: missing 'info' field".to_string())?;

    let description = info
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let content_type = info
        .get("description_content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let readme = if content_type.contains("html") || (description.contains('<') && !content_type.contains("markdown")) {
        parse_html(description)
    } else {
        description.to_string()
    };

    Ok(DocResult {
        registry: "pypi",
        name: name.to_string(),
        version: info
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        description: info
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        repository: info
            .get("project_urls")
            .and_then(|u| u.get("Source"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                info.get("home_page")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }),
        homepage: info
            .get("home_page")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        documentation: info
            .get("project_urls")
            .and_then(|u| u.get("Documentation"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        license: info
            .get("license")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        readme,
    })
}

/// Render a `DocResult` as a Markdown document.
pub fn doc_result_to_markdown(result: &DocResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {} ({})\n\n",
        result.name, result.registry
    ));
    if let Some(ref v) = result.version {
        out.push_str(&format!("**Version:** {}\n\n", v));
    }
    if let Some(ref d) = result.description {
        out.push_str(&format!("{}\n\n", d));
    }
    if let Some(ref r) = result.repository {
        out.push_str(&format!("**Repository:** {}\n\n", r));
    }
    if let Some(ref h) = result.homepage {
        out.push_str(&format!("**Homepage:** {}\n\n", h));
    }
    if let Some(ref d) = result.documentation {
        out.push_str(&format!("**Documentation:** {}\n\n", d));
    }
    if let Some(ref l) = result.license {
        out.push_str(&format!("**License:** {}\n\n", l));
    }
    if !result.readme.is_empty() {
        out.push_str("---\n\n");
        out.push_str(&result.readme);
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_url_crates() {
        let (url, is_json) = registry_url(Registry::Crates, "serde");
        assert_eq!(url, "https://crates.io/api/v1/crates/serde");
        assert!(is_json);
    }

    #[test]
    fn registry_url_docsrs() {
        let (url, is_json) = registry_url(Registry::Docsrs, "serde");
        assert_eq!(url, "https://docs.rs/crate/serde/latest/source/README.md");
        assert!(!is_json);
    }

    #[test]
    fn registry_url_npm() {
        let (url, is_json) = registry_url(Registry::Npm, "express");
        assert_eq!(url, "https://registry.npmjs.org/express");
        assert!(is_json);
    }

    #[test]
    fn registry_url_pypi() {
        let (url, is_json) = registry_url(Registry::Pypi, "requests");
        assert_eq!(url, "https://pypi.org/pypi/requests/json");
        assert!(is_json);
    }

    #[test]
    fn parse_crates_io_extracts_metadata() {
        let json = r#"{"crate":{"max_version":"1.0.193","description":"A serialization framework","repository":"https://github.com/serde-rs/serde","homepage":null,"documentation":"https://serde.rs","license":"MIT OR Apache-2.0"}}"#;
        let result = parse_crates_io(json, "serde").unwrap();
        assert_eq!(result.registry, "crates.io");
        assert_eq!(result.name, "serde");
        assert_eq!(result.version.as_deref(), Some("1.0.193"));
        assert_eq!(result.description.as_deref(), Some("A serialization framework"));
        assert_eq!(result.repository.as_deref(), Some("https://github.com/serde-rs/serde"));
        assert_eq!(result.documentation.as_deref(), Some("https://serde.rs"));
        assert_eq!(result.license.as_deref(), Some("MIT OR Apache-2.0"));
        assert!(result.readme.is_empty());
    }

    #[test]
    fn parse_crates_io_handles_missing_fields() {
        let json = r#"{"crate":{"max_version":"0.1.0"}}"#;
        let result = parse_crates_io(json, "mycrate").unwrap();
        assert_eq!(result.version.as_deref(), Some("0.1.0"));
        assert!(result.description.is_none());
        assert!(result.repository.is_none());
    }

    #[test]
    fn parse_npm_extracts_readme_and_metadata() {
        let json = r##"{
            "dist-tags": {"latest": "4.18.2"},
            "versions": {
                "4.18.2": {
                    "description": "Fast web framework",
                    "repository": {"url": "git+https://github.com/expressjs/express.git"},
                    "homepage": "https://expressjs.com",
                    "license": "MIT"
                }
            },
            "readme": "# Express\n\nFast web framework for Node.js"
        }"##;
        let result = parse_npm(json, "express").unwrap();
        assert_eq!(result.registry, "npm");
        assert_eq!(result.name, "express");
        assert_eq!(result.version.as_deref(), Some("4.18.2"));
        assert_eq!(result.description.as_deref(), Some("Fast web framework"));
        assert_eq!(result.license.as_deref(), Some("MIT"));
        assert!(result.readme.contains("# Express"));
    }

    #[test]
    fn parse_npm_converts_html_readme() {
        let json = r#"{
            "dist-tags": {"latest": "1.0.0"},
            "versions": {"1.0.0": {}},
            "readme": "<h1>Title</h1><p>Hello world</p>"
        }"#;
        let result = parse_npm(json, "pkg").unwrap();
        assert!(result.readme.contains("Title"));
        assert!(result.readme.contains("Hello world"));
    }

    #[test]
    fn parse_pypi_extracts_metadata() {
        let json = r##"{
            "info": {
                "version": "2.31.0",
                "summary": "Python HTTP for Humans",
                "home_page": "https://requests.readthedocs.io",
                "license": "Apache 2.0",
                "description": "# Requests\n\nHTTP library for Python",
                "description_content_type": "text/markdown",
                "project_urls": {
                    "Source": "https://github.com/psf/requests",
                    "Documentation": "https://requests.readthedocs.io"
                }
            }
        }"##;
        let result = parse_pypi(json, "requests").unwrap();
        assert_eq!(result.registry, "pypi");
        assert_eq!(result.name, "requests");
        assert_eq!(result.version.as_deref(), Some("2.31.0"));
        assert_eq!(result.description.as_deref(), Some("Python HTTP for Humans"));
        assert_eq!(result.repository.as_deref(), Some("https://github.com/psf/requests"));
        assert_eq!(result.documentation.as_deref(), Some("https://requests.readthedocs.io"));
        assert_eq!(result.license.as_deref(), Some("Apache 2.0"));
        assert!(result.readme.contains("# Requests"));
    }

    #[test]
    fn parse_pypi_converts_html_description() {
        let json = r#"{
            "info": {
                "version": "1.0",
                "description": "<h2>My Package</h2><p>Description here</p>",
                "description_content_type": "text/html"
            }
        }"#;
        let result = parse_pypi(json, "pkg").unwrap();
        assert!(result.readme.contains("My Package"));
        assert!(result.readme.contains("Description here"));
    }

    #[test]
    fn doc_result_to_markdown_includes_all_fields() {
        let result = DocResult {
            registry: "crates.io",
            name: "serde".to_string(),
            version: Some("1.0".to_string()),
            description: Some("Serialization framework".to_string()),
            repository: Some("https://github.com/serde-rs/serde".to_string()),
            homepage: None,
            documentation: Some("https://serde.rs".to_string()),
            license: Some("MIT".to_string()),
            readme: "# Serde\n\nA framework".to_string(),
        };
        let md = doc_result_to_markdown(&result);
        assert!(md.contains("# serde (crates.io)"));
        assert!(md.contains("**Version:** 1.0"));
        assert!(md.contains("Serialization framework"));
        assert!(md.contains("**Repository:**"));
        assert!(md.contains("**Documentation:**"));
        assert!(md.contains("**License:** MIT"));
        assert!(md.contains("---"));
        assert!(md.contains("# Serde"));
    }

    #[test]
    fn doc_result_to_markdown_minimal() {
        let result = DocResult {
            registry: "npm",
            name: "pkg".to_string(),
            version: None,
            description: None,
            repository: None,
            homepage: None,
            documentation: None,
            license: None,
            readme: "Just a readme".to_string(),
        };
        let md = doc_result_to_markdown(&result);
        assert!(md.contains("# pkg (npm)"));
        assert!(md.contains("---"));
        assert!(md.contains("Just a readme"));
    }

    #[test]
    fn parse_crates_io_invalid_json_returns_error() {
        let result = parse_crates_io("not json", "serde");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("crates.io JSON parse error"));
    }

    #[test]
    fn parse_npm_invalid_json_returns_error() {
        let result = parse_npm("not json", "express");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("npm JSON parse error"));
    }

    #[test]
    fn parse_pypi_invalid_json_returns_error() {
        let result = parse_pypi("not json", "requests");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("pypi JSON parse error"));
    }

    #[test]
    fn parse_pypi_missing_info_returns_error() {
        let json = r#"{"releases": {}}"#;
        let result = parse_pypi(json, "pkg");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing 'info'"));
    }

    #[test]
    fn parse_npm_no_versions_falls_back_to_top_level() {
        let json = r#"{
            "dist-tags": {"latest": "1.0.0"},
            "readme": "README text"
        }"#;
        let result = parse_npm(json, "pkg").unwrap();
        assert_eq!(result.name, "pkg");
        assert_eq!(result.version.as_deref(), Some("1.0.0"));
        assert!(result.readme.contains("README text"));
    }

    #[test]
    fn parse_npm_repository_as_string() {
        let json = r#"{
            "dist-tags": {"latest": "1.0.0"},
            "versions": {"1.0.0": {"repository": "https://github.com/foo/bar"}},
            "readme": ""
        }"#;
        let result = parse_npm(json, "pkg").unwrap();
        assert_eq!(
            result.repository.as_deref(),
            Some("https://github.com/foo/bar")
        );
    }

    #[test]
    fn parse_npm_license_as_object() {
        let json = r#"{
            "dist-tags": {"latest": "1.0.0"},
            "versions": {"1.0.0": {"license": {"type": "BSD-2-Clause"}}},
            "readme": ""
        }"#;
        let result = parse_npm(json, "pkg").unwrap();
        assert_eq!(result.license.as_deref(), Some("BSD-2-Clause"));
    }

    #[test]
    fn parse_npm_empty_readme() {
        let json = r#"{
            "dist-tags": {"latest": "1.0.0"},
            "versions": {"1.0.0": {}},
            "readme": ""
        }"#;
        let result = parse_npm(json, "pkg").unwrap();
        assert!(result.readme.is_empty());
    }

    #[test]
    fn parse_pypi_empty_description() {
        let json = r#"{
            "info": {
                "version": "1.0",
                "summary": "A package"
            }
        }"#;
        let result = parse_pypi(json, "pkg").unwrap();
        assert_eq!(result.description.as_deref(), Some("A package"));
        assert!(result.readme.is_empty());
    }

    #[test]
    fn parse_pypi_falls_back_to_home_page_for_repository() {
        let json = r#"{
            "info": {
                "version": "1.0",
                "home_page": "https://example.com/project"
            }
        }"#;
        let result = parse_pypi(json, "pkg").unwrap();
        assert_eq!(
            result.repository.as_deref(),
            Some("https://example.com/project")
        );
        assert_eq!(
            result.homepage.as_deref(),
            Some("https://example.com/project")
        );
    }

    #[test]
    fn parse_pypi_html_description_without_content_type() {
        let json = r#"{
            "info": {
                "version": "1.0",
                "description": "<p>HTML content</p>"
            }
        }"#;
        let result = parse_pypi(json, "pkg").unwrap();
        assert!(result.readme.contains("HTML content"));
    }

    #[test]
    fn doc_result_to_markdown_includes_homepage() {
        let result = DocResult {
            registry: "npm",
            name: "express".to_string(),
            version: Some("4.18.2".to_string()),
            description: Some("Fast framework".to_string()),
            repository: Some("https://github.com/expressjs/express".to_string()),
            homepage: Some("https://expressjs.com".to_string()),
            documentation: None,
            license: Some("MIT".to_string()),
            readme: "# Express".to_string(),
        };
        let md = doc_result_to_markdown(&result);
        assert!(md.contains("**Homepage:** https://expressjs.com"));
    }

    #[test]
    fn doc_result_to_markdown_empty_readme_omits_separator() {
        let result = DocResult {
            registry: "crates.io",
            name: "pkg".to_string(),
            version: Some("1.0".to_string()),
            description: None,
            repository: None,
            homepage: None,
            documentation: None,
            license: None,
            readme: String::new(),
        };
        let md = doc_result_to_markdown(&result);
        assert!(!md.contains("---"));
        assert!(md.contains("**Version:** 1.0"));
    }

    #[test]
    fn registry_url_encodes_special_characters() {
        let (url, _) = registry_url(Registry::Npm, "@scope/pkg");
        assert_eq!(url, "https://registry.npmjs.org/@scope/pkg");
    }
}
