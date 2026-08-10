//! Library documentation fetcher — fetch README + metadata from crates.io, npm, or PyPI.
//!
//! No API key required. Uses public registry APIs:
//! - crates.io: `https://crates.io/api/v1/crates/{name}` (JSON)
//! - npm: `https://registry.npmjs.org/{name}` (JSON)
//! - PyPI: `https://pypi.org/pypi/{name}/json` (JSON)

use serde::Serialize;

/// Which package registry to query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Registry {
    CratesIo,
    Npm,
    Pypi,
}

impl Registry {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "crates" | "crates.io" | "crate" | "rust" => Some(Registry::CratesIo),
            "npm" | "node" => Some(Registry::Npm),
            "pypi" | "python" | "py" => Some(Registry::Pypi),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Registry::CratesIo => "crates.io",
            Registry::Npm => "npm",
            Registry::Pypi => "PyPI",
        }
    }
}

/// Metadata extracted from a package registry.
#[derive(Debug, Serialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub documentation: Option<String>,
    pub license: Option<String>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub author: Option<String>,
    pub readme: Option<String>,
    pub registry: &'static str,
}

/// Build the API URL for a given registry and package name.
pub fn registry_api_url(registry: Registry, name: &str) -> String {
    match registry {
        Registry::CratesIo => format!("https://crates.io/api/v1/crates/{}", name),
        Registry::Npm => format!("https://registry.npmjs.org/{}", name),
        Registry::Pypi => format!("https://pypi.org/pypi/{}/json", name),
    }
}

/// Parse crates.io JSON response into PackageInfo.
fn parse_crates_io(json: &serde_json::Value) -> Option<PackageInfo> {
    let crate_obj = json.get("crate")?;
    let name = crate_obj.get("name")?.as_str()?.to_string();
    let version = crate_obj.get("max_stable_version")
        .or_else(|| crate_obj.get("max_version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let description = crate_obj.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
    let homepage = crate_obj.get("homepage").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let repository = crate_obj.get("repository").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let documentation = crate_obj.get("documentation").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let license = crate_obj.get("license").and_then(|v| v.as_str()).map(|s| s.to_string());
    let keywords = crate_obj.get("keywords")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let categories = crate_obj.get("categories")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let author = crate_obj.get("owners")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|o| o.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(PackageInfo {
        name,
        version,
        description,
        homepage,
        repository,
        documentation,
        license,
        keywords,
        categories,
        author,
        readme: None,
        registry: "crates.io",
    })
}

/// Parse npm registry JSON response into PackageInfo.
fn parse_npm(json: &serde_json::Value) -> Option<PackageInfo> {
    let name = json.get("name")?.as_str()?.to_string();
    let dist_tags = json.get("dist-tags")?;
    let version = dist_tags.get("latest").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let latest_pkg = json.get("versions")
        .and_then(|v| v.get(&version))
        .or_else(|| json.get("versions").and_then(|v| v.as_object().and_then(|o| o.values().next())));

    let description = json.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
    let homepage = json.get("homepage").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let repository = json.get("repository")
        .and_then(|v| v.get("url"))
        .or_else(|| json.get("repository"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let license = latest_pkg.and_then(|p| p.get("license")).and_then(|v| v.as_str()).map(|s| s.to_string());
    let keywords = latest_pkg
        .and_then(|p| p.get("keywords"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let author = json.get("author")
        .and_then(|v| v.get("name"))
        .or_else(|| json.get("author"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let readme = json.get("readme").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());

    Some(PackageInfo {
        name,
        version,
        description,
        homepage,
        repository,
        documentation: None,
        license,
        keywords,
        categories: Vec::new(),
        author,
        readme: Some(readme).flatten(),
        registry: "npm",
    })
}

/// Parse PyPI JSON response into PackageInfo.
fn parse_pypi(json: &serde_json::Value) -> Option<PackageInfo> {
    let info = json.get("info")?;
    let name = info.get("name")?.as_str()?.to_string();
    let version = info.get("version").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let description = info.get("summary").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let homepage = info.get("home_page").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let repository = info.get("project_urls")
        .and_then(|v| v.as_object())
        .and_then(|o| o.iter().find(|(k, _)| k.to_ascii_lowercase().contains("source") || k.to_ascii_lowercase().contains("repository")))
        .and_then(|(_, v)| v.as_str())
        .map(|s| s.to_string());
    let documentation = info.get("project_urls")
        .and_then(|v| v.as_object())
        .and_then(|o| o.iter().find(|(k, _)| k.to_ascii_lowercase().contains("documentation") || k.to_ascii_lowercase().contains("docs")))
        .and_then(|(_, v)| v.as_str())
        .map(|s| s.to_string());
    let license = info.get("license").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let keywords_str = info.get("keywords").and_then(|v| v.as_str()).unwrap_or("");
    let keywords: Vec<String> = if keywords_str.is_empty() {
        Vec::new()
    } else {
        keywords_str.split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    };
    let author = info.get("author").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let readme = info.get("description").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());

    Some(PackageInfo {
        name,
        version,
        description,
        homepage,
        repository,
        documentation,
        license,
        keywords,
        categories: Vec::new(),
        author,
        readme: Some(readme).flatten(),
        registry: "PyPI",
    })
}

/// Parse a registry JSON response into PackageInfo.
pub fn parse_registry_response(registry: Registry, body: &str) -> Option<PackageInfo> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    match registry {
        Registry::CratesIo => parse_crates_io(&json),
        Registry::Npm => parse_npm(&json),
        Registry::Pypi => parse_pypi(&json),
    }
}

/// Render PackageInfo as Markdown.
pub fn package_info_to_markdown(info: &PackageInfo) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} ({})\n\n", info.name, info.registry));
    if let Some(ref desc) = info.description {
        out.push_str(&format!("{}\n\n", desc));
    }
    out.push_str("| Field | Value |\n|---|---|\n");
    out.push_str(&format!("| Version | {} |\n", info.version));
    if let Some(ref lic) = info.license {
        out.push_str(&format!("| License | {} |\n", lic));
    }
    if let Some(ref hp) = info.homepage {
        out.push_str(&format!("| Homepage | {} |\n", hp));
    }
    if let Some(ref repo) = info.repository {
        out.push_str(&format!("| Repository | {} |\n", repo));
    }
    if let Some(ref docs) = info.documentation {
        out.push_str(&format!("| Documentation | {} |\n", docs));
    }
    if let Some(ref author) = info.author {
        out.push_str(&format!("| Author | {} |\n", author));
    }
    if !info.keywords.is_empty() {
        out.push_str(&format!("| Keywords | {} |\n", info.keywords.join(", ")));
    }
    if !info.categories.is_empty() {
        out.push_str(&format!("| Categories | {} |\n", info.categories.join(", ")));
    }
    out.push('\n');
    if let Some(ref readme) = info.readme {
        out.push_str("## README\n\n");
        out.push_str(readme);
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_from_str_works() {
        assert_eq!(Registry::from_str("crates"), Some(Registry::CratesIo));
        assert_eq!(Registry::from_str("crates.io"), Some(Registry::CratesIo));
        assert_eq!(Registry::from_str("rust"), Some(Registry::CratesIo));
        assert_eq!(Registry::from_str("npm"), Some(Registry::Npm));
        assert_eq!(Registry::from_str("node"), Some(Registry::Npm));
        assert_eq!(Registry::from_str("pypi"), Some(Registry::Pypi));
        assert_eq!(Registry::from_str("python"), Some(Registry::Pypi));
        assert_eq!(Registry::from_str("unknown"), None);
    }

    #[test]
    fn registry_api_url_crates_io() {
        let url = registry_api_url(Registry::CratesIo, "serde");
        assert_eq!(url, "https://crates.io/api/v1/crates/serde");
    }

    #[test]
    fn registry_api_url_npm() {
        let url = registry_api_url(Registry::Npm, "express");
        assert_eq!(url, "https://registry.npmjs.org/express");
    }

    #[test]
    fn registry_api_url_pypi() {
        let url = registry_api_url(Registry::Pypi, "requests");
        assert_eq!(url, "https://pypi.org/pypi/requests/json");
    }

    #[test]
    fn parse_crates_io_response() {
        let body = r#"{
            "crate": {
                "name": "serde",
                "max_stable_version": "1.0.197",
                "description": "A serialization/deserialization framework",
                "homepage": "https://serde.rs",
                "repository": "https://github.com/serde-rs/serde",
                "documentation": "https://docs.rs/serde",
                "license": "MIT OR Apache-2.0",
                "keywords": ["serde", "serialization"],
                "categories": ["encoding"]
            }
        }"#;
        let info = parse_registry_response(Registry::CratesIo, body).unwrap();
        assert_eq!(info.name, "serde");
        assert_eq!(info.version, "1.0.197");
        assert_eq!(info.description.as_deref(), Some("A serialization/deserialization framework"));
        assert_eq!(info.homepage.as_deref(), Some("https://serde.rs"));
        assert_eq!(info.license.as_deref(), Some("MIT OR Apache-2.0"));
        assert_eq!(info.keywords, vec!["serde", "serialization"]);
        assert_eq!(info.categories, vec!["encoding"]);
        assert_eq!(info.registry, "crates.io");
    }

    #[test]
    fn parse_npm_response() {
        let body = r##"{
            "name": "express",
            "description": "Fast, unopinionated, minimalist web framework",
            "homepage": "https://expressjs.com",
            "repository": {"url": "git+https://github.com/expressjs/express.git"},
            "author": {"name": "TJ Holowaychuk"},
            "dist-tags": {"latest": "4.18.2"},
            "versions": {
                "4.18.2": {
                    "license": "MIT",
                    "keywords": ["express", "web", "framework"]
                }
            },
            "readme": "# Express\n\nFast web framework."
        }"##;
        let info = parse_registry_response(Registry::Npm, body).unwrap();
        assert_eq!(info.name, "express");
        assert_eq!(info.version, "4.18.2");
        assert_eq!(info.description.as_deref(), Some("Fast, unopinionated, minimalist web framework"));
        assert_eq!(info.homepage.as_deref(), Some("https://expressjs.com"));
        assert_eq!(info.license.as_deref(), Some("MIT"));
        assert_eq!(info.keywords, vec!["express", "web", "framework"]);
        assert_eq!(info.author.as_deref(), Some("TJ Holowaychuk"));
        assert_eq!(info.readme.as_deref(), Some("# Express\n\nFast web framework."));
        assert_eq!(info.registry, "npm");
    }

    #[test]
    fn parse_pypi_response() {
        let body = r##"{
            "info": {
                "name": "requests",
                "version": "2.31.0",
                "summary": "Python HTTP for Humans",
                "home_page": "https://requests.readthedocs.io",
                "license": "Apache 2.0",
                "keywords": "HTTP, client, requests",
                "author": "Kenneth Reitz",
                "description": "# Requests\n\nHTTP library.",
                "project_urls": {
                    "Source": "https://github.com/psf/requests",
                    "Documentation": "https://requests.readthedocs.io"
                }
            }
        }"##;
        let info = parse_registry_response(Registry::Pypi, body).unwrap();
        assert_eq!(info.name, "requests");
        assert_eq!(info.version, "2.31.0");
        assert_eq!(info.description.as_deref(), Some("Python HTTP for Humans"));
        assert_eq!(info.homepage.as_deref(), Some("https://requests.readthedocs.io"));
        assert_eq!(info.license.as_deref(), Some("Apache 2.0"));
        assert!(info.keywords.contains(&"HTTP".to_string()));
        assert_eq!(info.author.as_deref(), Some("Kenneth Reitz"));
        assert_eq!(info.repository.as_deref(), Some("https://github.com/psf/requests"));
        assert_eq!(info.documentation.as_deref(), Some("https://requests.readthedocs.io"));
        assert_eq!(info.registry, "PyPI");
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_registry_response(Registry::CratesIo, "not json").is_none());
    }

    #[test]
    fn package_info_to_markdown_includes_fields() {
        let info = PackageInfo {
            name: "test-pkg".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test package".to_string()),
            homepage: Some("https://example.com".to_string()),
            repository: Some("https://github.com/test/pkg".to_string()),
            documentation: None,
            license: Some("MIT".to_string()),
            keywords: vec!["test".to_string()],
            categories: vec![],
            author: Some("Alice".to_string()),
            readme: Some("## Usage\n\nDo stuff.".to_string()),
            registry: "crates.io",
        };
        let md = package_info_to_markdown(&info);
        assert!(md.contains("# test-pkg (crates.io)"));
        assert!(md.contains("A test package"));
        assert!(md.contains("| Version | 1.0.0 |"));
        assert!(md.contains("| License | MIT |"));
        assert!(md.contains("| Homepage | https://example.com |"));
        assert!(md.contains("## Usage"));
    }
}
