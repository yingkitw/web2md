use std::time::Duration;

use url::Url;

/// Parsed robots.txt rules for a single matching user-agent group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsTxt {
    disallow: Vec<String>,
    crawl_delay: Option<Duration>,
}

impl RobotsTxt {
    /// Allow all paths, no crawl delay.
    pub fn allow_all() -> Self {
        Self {
            disallow: Vec::new(),
            crawl_delay: None,
        }
    }

    /// Parse robots.txt content for the given user-agent string.
    /// Uses the first user-agent group that matches `user_agent` or `*`.
    pub fn parse(content: &str, user_agent: &str) -> Self {
        let groups = parse_groups(content);
        let ua = user_agent.to_ascii_lowercase();
        let product = ua.split('/').next().unwrap_or(&ua).trim();

        for group in groups {
            if group
                .agents
                .iter()
                .any(|agent| agent_matches(agent, product, &ua))
            {
                return RobotsTxt {
                    disallow: group.disallow,
                    crawl_delay: group.crawl_delay,
                };
            }
        }

        Self::allow_all()
    }

    pub fn crawl_delay(&self) -> Option<Duration> {
        self.crawl_delay
    }

    /// Returns whether fetching `url` is permitted by these rules.
    pub fn is_allowed(&self, url: &str) -> bool {
        let path = match Url::parse(url) {
            Ok(parsed) => parsed.path().to_string(),
            Err(_) => return true,
        };
        path_is_allowed(&path, &self.disallow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RobotsGroup {
    agents: Vec<String>,
    disallow: Vec<String>,
    crawl_delay: Option<Duration>,
}

fn agent_matches(agent: &str, product: &str, full_ua: &str) -> bool {
    agent == "*" || agent == product || full_ua.contains(agent)
}

fn parse_groups(content: &str) -> Vec<RobotsGroup> {
    let mut groups = Vec::new();
    let mut current: Option<RobotsGroup> = None;

    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();

        match key.as_str() {
            "user-agent" => {
                let agent = value.to_ascii_lowercase();
                match &mut current {
                    Some(group) if group.disallow.is_empty() && group.crawl_delay.is_none() => {
                        group.agents.push(agent);
                    }
                    Some(group) => {
                        groups.push(group.clone());
                        current = Some(RobotsGroup {
                            agents: vec![agent],
                            disallow: Vec::new(),
                            crawl_delay: None,
                        });
                    }
                    None => {
                        current = Some(RobotsGroup {
                            agents: vec![agent],
                            disallow: Vec::new(),
                            crawl_delay: None,
                        });
                    }
                }
            }
            "disallow" if current.is_some() => {
                current.as_mut().unwrap().disallow.push(value.to_string());
            }
            "crawl-delay" if current.is_some() => {
                if let Ok(secs) = value.parse::<f64>()
                    && secs >= 0.0 {
                        current.as_mut().unwrap().crawl_delay =
                            Some(Duration::from_secs_f64(secs));
                    }
            }
            _ => {}
        }
    }

    if let Some(group) = current {
        groups.push(group);
    }

    groups
}

fn path_is_allowed(path: &str, disallow: &[String]) -> bool {
    if disallow.is_empty() {
        return true;
    }
    for rule in disallow {
        if rule.is_empty() {
            continue;
        }
        if rule == "/" {
            return false;
        }
        if path.starts_with(rule) {
            return false;
        }
    }
    true
}

/// Build the origin key (`scheme://host[:port]`) used for robots.txt lookup.
pub fn robots_origin(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

/// True when the URL path is `/robots.txt`.
pub fn is_robots_txt_url(url: &Url) -> bool {
    url.path().eq_ignore_ascii_case("/robots.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_disallow_rules_for_wildcard_agent() {
        let txt = "User-agent: *\nDisallow: /private/\nDisallow: /admin\n";
        let rules = RobotsTxt::parse(txt, "Web2MD/0.1.2");
        assert!(!rules.is_allowed("https://example.com/private/page"));
        assert!(!rules.is_allowed("https://example.com/admin"));
        assert!(rules.is_allowed("https://example.com/public"));
    }

    #[test]
    fn parse_uses_first_matching_agent_group() {
        let txt = "User-agent: OtherBot\nDisallow: /\nUser-agent: Web2MD\nDisallow: /secret\n";
        let rules = RobotsTxt::parse(txt, "Web2MD/0.1.2");
        assert!(rules.is_allowed("https://example.com/"));
        assert!(!rules.is_allowed("https://example.com/secret/doc"));
    }

    #[test]
    fn parse_crawl_delay_seconds() {
        let txt = "User-agent: *\nCrawl-delay: 2.5\n";
        let rules = RobotsTxt::parse(txt, "Web2MD/0.1.2");
        assert_eq!(rules.crawl_delay(), Some(Duration::from_secs_f64(2.5)));
    }

    #[test]
    fn empty_disallow_rule_allows_path() {
        let txt = "User-agent: *\nDisallow:\n";
        let rules = RobotsTxt::parse(txt, "Web2MD/0.1.2");
        assert!(rules.is_allowed("https://example.com/anything"));
    }

    #[test]
    fn disallow_slash_blocks_all() {
        let txt = "User-agent: *\nDisallow: /\n";
        let rules = RobotsTxt::parse(txt, "Web2MD/0.1.2");
        assert!(!rules.is_allowed("https://example.com/page"));
    }

    #[test]
    fn robots_origin_includes_non_default_port() {
        let url = Url::parse("http://127.0.0.1:8080/page").unwrap();
        assert_eq!(
            robots_origin(&url).as_deref(),
            Some("http://127.0.0.1:8080")
        );
    }

    #[test]
    fn is_robots_txt_url_detects_path() {
        let url = Url::parse("https://example.com/robots.txt").unwrap();
        assert!(is_robots_txt_url(&url));
        let url = Url::parse("https://example.com/page").unwrap();
        assert!(!is_robots_txt_url(&url));
    }
}
