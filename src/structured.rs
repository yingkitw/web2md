//! Domain-specific deterministic extractors for structured JSON-LD types.
//!
//! Each `extract_*` function reads the page's JSON-LD blocks and builds a
//! clean Markdown document. No LLM is involved. Each extractor:
//!
//! - Returns `Ok(None)` if no JSON-LD block of the target `@type` is found.
//! - Returns `Err(StructuredError::Ambiguous)` if multiple distinct blocks are found
//!   and the caller cannot decide which one to render — callers fall back to plain
//!   Markdown conversion instead of guessing.
//! - Returns `Ok(Some(markdown))` with a YAML frontmatter block describing the
//!   structured fields, plus prose that is either an ingredient list / numbered
//!   steps (recipes), Q+A pairs (FAQs), or a labeled field summary (jobs/events).
//!
//! These mirror Firecrawl's `json` extraction but cost zero credits because they
//! only consume schema.org structured data already on the page.

use std::fmt;

use crate::html_meta::iter_json_ld_blocks;

/// Errors a structured extractor can return.
#[derive(Debug, Clone)]
pub enum StructuredError {
    /// Multiple JSON-LD blocks of the same type were found and disagree on
    /// key fields (e.g. two recipes for two different dishes).
    Ambiguous(String),
}

impl fmt::Display for StructuredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructuredError::Ambiguous(s) => write!(f, "ambiguous structured data: {}", s),
        }
    }
}

impl std::error::Error for StructuredError {}

fn json_ld_value_is_type(json: &serde_json::Value, type_name: &str) -> bool {
    match json.get("@type") {
        Some(t) => {
            if let Some(s) = t.as_str() {
                return s.eq_ignore_ascii_case(type_name);
            }
            if let Some(arr) = t.as_array() {
                return arr.iter().any(|v| {
                    v.as_str()
                        .map(|s| s.eq_ignore_ascii_case(type_name))
                        .unwrap_or(false)
                });
            }
            false
        }
        None => false,
    }
}

fn find_blocks_of_type(html: &str, type_name: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for json in iter_json_ld_blocks(html) {
        if let Some(arr) = json.as_array() {
            for node in arr {
                if json_ld_value_is_type(node, type_name) {
                    out.push(node.clone());
                }
            }
        } else if json_ld_value_is_type(&json, type_name) {
            out.push(json.clone());
        }
        if let Some(graph) = json.get("@graph").and_then(|g| g.as_array()) {
            for node in graph {
                if json_ld_value_is_type(node, type_name) {
                    out.push(node.clone());
                }
            }
        }
    }
    out
}

fn first_nonempty_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut cur = value;
    for k in path {
        cur = cur.get(*k)?;
    }
    if let Some(s) = cur.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(arr) = cur.as_array() {
        for v in arr {
            if let Some(s) = v.as_str() {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if let Some(obj) = v.as_object() {
                if let Some(s) = obj.get("name").and_then(|n| n.as_str()) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
                if let Some(s) = obj.get("text").and_then(|n| n.as_str()) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
}

fn duration_to_iso8601(value: Option<&serde_json::Value>) -> Option<String> {
    let v = value?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    None
}

fn duration_minutes(value: &serde_json::Value) -> Option<u32> {
    if let Some(s) = value.as_str() {
        return parse_iso8601_minutes(s);
    }
    if let Some(n) = value.as_u64() {
        return Some(n as u32);
    }
    None
}

/// Parse `PT1H30M`, `PT15M`, `PT45S`, etc. into minutes.
fn parse_iso8601_minutes(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    if !raw.starts_with("PT") && !raw.starts_with("P") {
        return None;
    }
    let mut hours = 0u32;
    let mut minutes = 0u32;
    let mut current = String::new();
    let mut in_time = false;
    for ch in raw.chars() {
        match ch {
            'P' => {}
            'T' => in_time = true,
            '0'..='9' => current.push(ch),
            'H' | 'M' | 'S' | 'D' | 'W' => {
                let n: u32 = current.parse().unwrap_or(0);
                match ch {
                    'H' => hours = n,
                    'M' => minutes = n,
                    'D' | 'W' => hours = hours.saturating_add(n * 24),
                    _ => {}
                }
                current.clear();
                if ch == 'S' || ch == 'M' {
                    in_time = false;
                }
                let _ = in_time;
            }
            _ => {}
        }
    }
    if hours == 0 && minutes == 0 {
        None
    } else {
        Some(hours * 60 + minutes)
    }
}

fn yaml_escape(s: &str) -> String {
    let needs_quote = s.contains(':')
        || s.contains('#')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.contains('\n')
        || s.starts_with('"')
        || s.starts_with('\'');
    if needs_quote {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn frontmatter_block(fields: &[(&str, String)]) -> String {
    let mut out = String::from("---\n");
    for (k, v) in fields {
        if v.is_empty() {
            continue;
        }
        out.push_str(&format!("{}: {}\n", k, yaml_escape(v)));
    }
    out.push_str("---\n\n");
    out
}

fn quant_item_text(v: &serde_json::Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.trim().to_string());
    }
    if let Some(obj) = v.as_object() {
        let mut parts: Vec<String> = Vec::new();
        if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
            let t = text.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
        if let Some(name) = obj.get("name").and_then(|t| t.as_str()) {
            let t = name.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
        let mut qty_bits = Vec::new();
        for k in ["value", "quantity"].iter() {
            if let Some(v) = obj.get(*k).and_then(|v| v.as_str()) {
                qty_bits.push(v.trim().to_string());
            }
        }
        for k in ["unitText", "unitCode"].iter() {
            if let Some(v) = obj.get(*k).and_then(|v| v.as_str()) {
                qty_bits.push(v.trim().to_string());
            }
        }
        if !qty_bits.is_empty() {
            parts.insert(0, qty_bits.join(" "));
        }
        if !parts.is_empty() {
            return Some(parts.join(" "));
        }
    }
    None
}

fn extract_ingredients(value: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = value.get("recipeIngredient").and_then(|v| v.as_array()) {
        for it in arr {
            if let Some(t) = quant_item_text(it) {
                out.push(t);
            }
        }
    } else if let Some(arr) = value.get("ingredients").and_then(|v| v.as_array()) {
        for it in arr {
            if let Some(t) = quant_item_text(it) {
                out.push(t);
            }
        }
    }
    out
}

fn extract_instructions(value: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let raw = match value.get("recipeInstructions") {
        Some(r) => r,
        None => return out,
    };
    let arr: Vec<&serde_json::Value> = if let Some(a) = raw.as_array() {
        a.iter().collect()
    } else {
        vec![raw]
    };
    let mut step_no = 1;
    for entry in arr {
        if let Some(text) = entry.get("text").and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                out.push(format!("{}. {}", step_no, trimmed));
                step_no += 1;
            }
        } else if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                out.push(format!("{}. {}", step_no, trimmed));
                step_no += 1;
            }
        } else if let Some(s) = entry.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                out.push(format!("{}. {}", step_no, trimmed));
                step_no += 1;
            }
        } else if let Some(items) = entry.get("itemListElement").and_then(|v| v.as_array()) {
            for it in items {
                if let Some(text) = it.get("text").and_then(|v| v.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        out.push(format!("{}. {}", step_no, trimmed));
                        step_no += 1;
                    }
                }
            }
        }
    }
    out
}

/// Render a Recipe JSON-LD block as Markdown (frontmatter + ingredient list + numbered steps).
pub fn extract_recipe(html: &str) -> Result<Option<String>, StructuredError> {
    let blocks = find_blocks_of_type(html, "Recipe");
    if blocks.is_empty() {
        return Ok(None);
    }
    if blocks.len() > 1 {
        return Err(StructuredError::Ambiguous(format!(
            "found {} Recipe blocks; refusing to guess which to render",
            blocks.len()
        )));
    }
    let recipe = &blocks[0];

    let name = first_nonempty_string(recipe, &["name"])
        .unwrap_or_else(|| "Untitled Recipe".to_string());
    let description = first_nonempty_string(recipe, &["description"]).unwrap_or_default();
    let author = first_nonempty_string(recipe, &["author", "name"])
        .or_else(|| first_nonempty_string(recipe, &["author"]))
        .unwrap_or_default();
    let yield_servings = recipe
        .get("recipeYield")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            recipe
                .get("recipeYield")
                .and_then(|v| v.as_u64())
                .map(|n| n.to_string())
        })
        .unwrap_or_default();
    let category = first_nonempty_string(recipe, &["recipeCategory"]).unwrap_or_default();
    let cuisine = first_nonempty_string(recipe, &["recipeCuisine"]).unwrap_or_default();
    let prep_min = recipe
        .get("prepTime")
        .and_then(duration_minutes)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let cook_min = recipe
        .get("cookTime")
        .and_then(duration_minutes)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let total_min = recipe
        .get("totalTime")
        .and_then(duration_minutes)
        .map(|n| n.to_string())
        .unwrap_or_default();

    let ingredients = extract_ingredients(recipe);
    let steps = extract_instructions(recipe);

    if ingredients.is_empty() && steps.is_empty() {
        return Ok(None);
    }

    let mut out = String::new();
    let mut fm: Vec<(&str, String)> = vec![("title", name.clone())];
    if !description.is_empty() {
        fm.push(("description", description.clone()));
    }
    if !author.is_empty() {
        fm.push(("author", author));
    }
    if !yield_servings.is_empty() {
        fm.push(("yield", yield_servings));
    }
    if !category.is_empty() {
        fm.push(("category", category));
    }
    if !cuisine.is_empty() {
        fm.push(("cuisine", cuisine));
    }
    if !prep_min.is_empty() {
        fm.push(("prep_minutes", prep_min));
    }
    if !cook_min.is_empty() {
        fm.push(("cook_minutes", cook_min));
    }
    if !total_min.is_empty() {
        fm.push(("total_minutes", total_min));
    }
    out.push_str(&frontmatter_block(&fm));

    out.push_str(&format!("# {}\n\n", name));
    if !description.is_empty() {
        out.push_str(&format!("{}\n\n", description));
    }

    if !ingredients.is_empty() {
        out.push_str("## Ingredients\n\n");
        for ing in &ingredients {
            out.push_str(&format!("- {}\n", ing));
        }
        out.push('\n');
    }
    if !steps.is_empty() {
        out.push_str("## Instructions\n\n");
        for step in &steps {
            out.push_str(&format!("{}\n", step));
        }
        out.push('\n');
    }
    Ok(Some(out))
}

/// Render an FAQPage JSON-LD block as Markdown (each Q = heading, A = prose).
pub fn extract_faq(html: &str) -> Result<Option<String>, StructuredError> {
    let blocks = find_blocks_of_type(html, "FAQPage");
    if blocks.is_empty() {
        return Ok(None);
    }
    let faq = &blocks[0];

    let main_entities = match faq.get("mainEntity") {
        Some(m) => m,
        None => return Ok(None),
    };
    let entries: Vec<&serde_json::Value> = if let Some(arr) = main_entities.as_array() {
        arr.iter().collect()
    } else {
        vec![main_entities]
    };
    if entries.is_empty() {
        return Ok(None);
    }
    let title = first_nonempty_string(faq, &["name"]).unwrap_or_else(|| "FAQ".to_string());

    let mut out = String::from("---\n");
    out.push_str(&format!("title: {}\n", yaml_escape(&title)));
    out.push_str("type: faq\n");
    out.push_str("---\n\n");
    out.push_str(&format!("# {}\n\n", title));

    for (i, e) in entries.iter().enumerate() {
        let q = first_nonempty_string(e, &["name"])
            .unwrap_or_else(|| format!("Question {}", i + 1));
        let accepted = first_nonempty_string(e, &["acceptedAnswer", "text"])
            .or_else(|| first_nonempty_string(e, &["answer"]));
        let suggestion = first_nonempty_string(e, &["suggestedAnswer"]);
        let answer = accepted.or(suggestion).unwrap_or_default();

        out.push_str(&format!("## {}\n\n", q));
        if answer.is_empty() {
            out.push_str("_(no answer found)_\n\n");
        } else {
            out.push_str(&format!("{}\n\n", answer));
        }
    }
    Ok(Some(out))
}

fn push_field(out: &mut Vec<(String, String)>, label: &str, value: String) {
    if !value.is_empty() {
        out.push((label.to_string(), value));
    }
}

fn pick_employment_type(v: &serde_json::Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    None
}

/// Render a JobPosting JSON-LD block as Markdown (frontmatter + labeled body).
pub fn extract_job(html: &str) -> Result<Option<String>, StructuredError> {
    let blocks = find_blocks_of_type(html, "JobPosting");
    if blocks.is_empty() {
        return Ok(None);
    }
    if blocks.len() > 1 {
        return Err(StructuredError::Ambiguous(
            "found multiple JobPosting blocks".into(),
        ));
    }
    let job = &blocks[0];

    let title = first_nonempty_string(job, &["title"])
        .or_else(|| first_nonempty_string(job, &["name"]))
        .unwrap_or_else(|| "Job Posting".to_string());
    let description = first_nonempty_string(job, &["description"]).unwrap_or_default();
    let date_posted = first_nonempty_string(job, &["datePosted"]).unwrap_or_default();
    let employment_type = job
        .get("employmentType")
        .and_then(pick_employment_type)
        .or_else(|| {
            first_nonempty_string(job, &["employmentType"])
        })
        .unwrap_or_default();
    let org_name = first_nonempty_string(job, &["hiringOrganization", "name"])
        .or_else(|| first_nonempty_string(job, &["hiringOrganization"]))
        .unwrap_or_default();
    let org_url = first_nonempty_string(job, &["hiringOrganization", "sameAs"])
        .or_else(|| first_nonempty_string(job, &["hiringOrganization", "url"]))
        .unwrap_or_default();
    let location = build_location(job);
    let salary = build_salary(job);
    let apply_url = first_nonempty_string(job, &["url"])
        .or_else(|| first_nonempty_string(job, &["applicationContact", "url"]))
        .unwrap_or_default();

    let mut fields: Vec<(String, String)> = Vec::new();
    push_field(&mut fields, "title", title.clone());
    if !org_name.is_empty() {
        push_field(&mut fields, "company", org_name.clone());
    }
    if !location.is_empty() {
        push_field(&mut fields, "location", location.clone());
    }
    if !employment_type.is_empty() {
        push_field(&mut fields, "employment_type", employment_type.clone());
    }
    if !salary.is_empty() {
        push_field(&mut fields, "salary", salary.clone());
    }
    if !date_posted.is_empty() {
        push_field(&mut fields, "date_posted", date_posted);
    }
    if !apply_url.is_empty() {
        push_field(&mut fields, "apply_url", apply_url.clone());
    }
    if !org_url.is_empty() {
        push_field(&mut fields, "company_url", org_url);
    }
    let fm_slice: Vec<(&str, String)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    let mut md = frontmatter_block(&fm_slice);
    md.push_str(&format!("# {}\n\n", title));
    if !org_name.is_empty() {
        md.push_str(&format!("**Company:** {}\n\n", org_name));
    }
    if !location.is_empty() {
        md.push_str(&format!("**Location:** {}\n\n", location));
    }
    if !employment_type.is_empty() {
        md.push_str(&format!("**Type:** {}\n\n", employment_type));
    }
    if !salary.is_empty() {
        md.push_str(&format!("**Salary:** {}\n\n", salary));
    }
    if !apply_url.is_empty() {
        md.push_str(&format!("**Apply:** <{}>\n\n", apply_url));
    }
    if !description.is_empty() {
        md.push_str("## Description\n\n");
        md.push_str(&strip_simple_html(&description));
        md.push('\n');
    }
    Ok(Some(md))
}

fn build_location(job: &serde_json::Value) -> String {
    let place = job
        .get("jobLocation")
        .and_then(|jl| jl.get("address"))
        .or_else(|| job.get("jobLocation").and_then(|j| j.get("address")));
    let addr = match place {
        Some(a) => a,
        None => return String::new(),
    };
    let mut parts = Vec::new();
    for key in ["addressLocality", "addressRegion", "addressCountry"].iter() {
        if let Some(v) = addr.get(*key).and_then(|v| v.as_str()) {
            let s = v.trim();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    let line = parts.join(", ");
    if line.is_empty() {
        first_nonempty_string(addr, &["streetAddress"]).unwrap_or_default()
    } else {
        line
    }
}

fn build_salary(job: &serde_json::Value) -> String {
    let base = match job.get("baseSalary").and_then(|v| v.get("value")) {
        Some(v) => v,
        None => return String::new(),
    };
    let value = base
        .get("value")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            base.get("value")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .or_else(|| base.get("minValue").and_then(|v| v.as_f64()));
    let currency = base
        .get("currency")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            job.get("baseSalary")
                .and_then(|v| v.get("currency"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let unit = base
        .get("unitText")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            job.get("baseSalary")
                .and_then(|v| v.get("unitText"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if let Some(n) = value {
        let mut parts = Vec::new();
        parts.push(format!("{:.2}", n));
        if !currency.is_empty() {
            parts.push(currency);
        }
        if !unit.is_empty() {
            parts.push(format!("/{}", unit));
        }
        return parts.join(" ");
    }
    String::new()
}

fn strip_simple_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Render an Event JSON-LD block as Markdown.
pub fn extract_event(html: &str) -> Result<Option<String>, StructuredError> {
    let blocks = find_blocks_of_type(html, "Event");
    if blocks.is_empty() {
        return Ok(None);
    }
    if blocks.len() > 1 {
        return Err(StructuredError::Ambiguous(format!(
            "found {} Event blocks",
            blocks.len()
        )));
    }
    let event = &blocks[0];

    let name = first_nonempty_string(event, &["name"])
        .unwrap_or_else(|| "Event".to_string());
    let start = first_nonempty_string(event, &["startDate"])
        .or_else(|| duration_to_iso8601(event.get("startDate")))
        .unwrap_or_default();
    let end = first_nonempty_string(event, &["endDate"]).unwrap_or_default();
    let status = first_nonempty_string(event, &["eventStatus"])
        .or_else(|| first_nonempty_string(event, &["eventAttendanceMode"]))
        .unwrap_or_default();
    let venue_name = first_nonempty_string(event, &["location", "name"])
        .or_else(|| first_nonempty_string(event, &["location"]))
        .unwrap_or_default();
    let venue_addr = first_nonempty_string(event, &["location", "address", "streetAddress"])
        .or_else(|| {
            let addr = event.get("location").and_then(|v| v.get("address"));
            let built = build_event_addr(addr);
            if built.is_empty() { None } else { Some(built) }
        })
        .unwrap_or_default();
    let url = first_nonempty_string(event, &["url"]).unwrap_or_default();
    let offers_price = event
        .get("offers")
        .and_then(|o| o.get("price"))
        .and_then(|p| p.as_str())
        .map(str::to_string)
        .or_else(|| {
            event
                .get("offers")
                .and_then(|o| o.get("price"))
                .and_then(|p| p.as_f64())
                .map(|n| n.to_string())
        })
        .unwrap_or_default();
    let offers_url = first_nonempty_string(event, &["offers", "url"]).unwrap_or_default();

    let mut fm: Vec<(&str, String)> = vec![("title", name.clone())];
    if !start.is_empty() {
        fm.push(("start_date", start.clone()));
    }
    if !end.is_empty() {
        fm.push(("end_date", end.clone()));
    }
    if !status.is_empty() {
        fm.push(("status", status));
    }
    if !venue_name.is_empty() {
        fm.push(("venue", venue_name.clone()));
    }
    if !venue_addr.is_empty() {
        fm.push(("venue_address", venue_addr.clone()));
    }
    if !offers_price.is_empty() {
        fm.push(("price", offers_price.clone()));
    }
    if !offers_url.is_empty() {
        fm.push(("ticket_url", offers_url.clone()));
    }
    if !url.is_empty() {
        fm.push(("url", url.clone()));
    }

    let mut md = frontmatter_block(&fm);
    md.push_str(&format!("# {}\n\n", name));
    if !start.is_empty() {
        md.push_str(&format!("- **Starts:** {}\n", start));
    }
    if !end.is_empty() {
        md.push_str(&format!("- **Ends:** {}\n", end));
    }
    if !venue_name.is_empty() {
        md.push_str(&format!("- **Venue:** {}", venue_name));
        if !venue_addr.is_empty() {
            md.push_str(&format!(" ({})", venue_addr));
        }
        md.push('\n');
    }
    if !offers_price.is_empty() {
        md.push_str(&format!("- **Price:** {}\n", offers_price));
    }
    if !offers_url.is_empty() {
        md.push_str(&format!("- **Tickets:** <{}>\n", offers_url));
    } else if !url.is_empty() {
        md.push_str(&format!("- **More info:** <{}>\n", url));
    }
    Ok(Some(md))
}

fn build_event_addr(addr: Option<&serde_json::Value>) -> String {
    let addr = match addr {
        Some(a) => a,
        None => return String::new(),
    };
    let mut parts = Vec::new();
    for key in ["streetAddress", "addressLocality", "addressRegion", "addressCountry"].iter() {
        if let Some(v) = addr.get(*key).and_then(|v| v.as_str()) {
            let s = v.trim();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe_html() -> &'static str {
        r#"<html><head>
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@type": "Recipe",
          "name": "Pancakes",
          "description": "Fluffy pancakes for breakfast",
          "author": {"@type": "Person", "name": "Ada"},
          "recipeYield": "4 servings",
          "recipeCategory": "Breakfast",
          "recipeCuisine": "American",
          "prepTime": "PT5M",
          "cookTime": "PT15M",
          "totalTime": "PT20M",
          "recipeIngredient": ["2 cups flour", "1 cup milk", "1 egg"],
          "recipeInstructions": [
            {"@type": "HowToStep", "text": "Mix dry ingredients."},
            {"@type": "HowToStep", "text": "Add wet ingredients and stir."},
            {"@type": "HowToStep", "text": "Cook on a hot griddle."}
          ]
        }
        </script>
        </head><body></body></html>"#
    }

    #[test]
    fn recipe_extracts_frontmatter_and_lists() {
        let md = extract_recipe(recipe_html()).unwrap().expect("markdown");
        assert!(md.contains("title: Pancakes"));
        assert!(md.contains("prep_minutes: 5"));
        assert!(md.contains("cook_minutes: 15"));
        assert!(md.contains("yield: 4 servings"));
        assert!(md.contains("## Ingredients"));
        assert!(md.contains("- 2 cups flour"));
        assert!(md.contains("## Instructions"));
        assert!(md.contains("1. Mix dry ingredients."));
        assert!(md.contains("3. Cook on a hot griddle."));
    }

    #[test]
    fn recipe_missing_returns_none() {
        let html = "<html><body><p>No structured data here.</p></body></html>";
        assert!(matches!(extract_recipe(html), Ok(None)));
    }

    #[test]
    fn recipe_ambiguous_errors() {
        let html = r#"<html><head>
        <script type="application/ld+json">[{"@type":"Recipe","name":"A"},{"@type":"Recipe","name":"B"}]</script>
        </head></html>"#;
        assert!(extract_recipe(html).is_err());
    }

    #[test]
    fn faq_extracts_qa_pairs() {
        let html = r#"<html><head>
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@type": "FAQPage",
          "name": "Shipping FAQ",
          "mainEntity": [
            {"@type": "Question", "name": "When do you ship?", "acceptedAnswer": {"@type": "Answer", "text": "We ship within 24 hours."}},
            {"@type": "Question", "name": "Do you ship internationally?", "acceptedAnswer": {"@type": "Answer", "text": "Yes, to most countries."}}
          ]
        }
        </script>
        </head></html>"#;
        let md = extract_faq(html).unwrap().expect("md");
        assert!(md.contains("title: Shipping FAQ"));
        assert!(md.contains("## When do you ship?"));
        assert!(md.contains("We ship within 24 hours."));
        assert!(md.contains("## Do you ship internationally?"));
    }

    #[test]
    fn job_extracts_core_fields() {
        let html = r#"<html><head>
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@type": "JobPosting",
          "title": "Senior Rust Engineer",
          "description": "Build cool stuff in Rust.",
          "datePosted": "2026-01-15",
          "employmentType": "FULL_TIME",
          "hiringOrganization": {"@type": "Organization", "name": "Acme", "sameAs": "https://acme.example"},
          "jobLocation": {"@type": "Place", "address": {"@type": "PostalAddress", "addressLocality": "Berlin", "addressCountry": "DE"}},
          "baseSalary": {"@type": "MonetaryAmount", "currency": "EUR", "value": {"@type": "QuantitativeValue", "value": 120000, "unitText": "YEAR"}},
          "url": "https://acme.example/jobs/1"
        }
        </script>
        </head></html>"#;
        let md = extract_job(html).unwrap().expect("md");
        assert!(md.contains("title: Senior Rust Engineer"));
        assert!(md.contains("company: Acme"));
        assert!(md.contains("Berlin, DE"));
        assert!(md.contains("120000.00 EUR"));
        assert!(md.contains("Build cool stuff in Rust."));
    }

    #[test]
    fn event_extracts_basic_fields() {
        let html = r#"<html><head>
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@type": "Event",
          "name": "RustConf 2026",
          "startDate": "2026-09-01T09:00",
          "endDate": "2026-09-03T18:00",
          "location": {"@type": "Place", "name": "Convention Center", "address": {"@type": "PostalAddress", "addressLocality": "Paris", "addressCountry": "FR"}},
          "offers": {"@type": "Offer", "price": "299", "priceCurrency": "EUR", "url": "https://tickets.example/rust"}
        }
        </script>
        </head></html>"#;
        let md = extract_event(html).unwrap().expect("md");
        assert!(md.contains("title: RustConf 2026"));
        assert!(md.contains("start_date: \"2026-09-01T09:00\""));
        assert!(md.contains("venue: Convention Center"));
        assert!(md.contains("venue_address: Paris, FR"));
        assert!(md.contains("price: 299"));
    }

    #[test]
    fn parse_iso8601_minutes_handles_compact_form() {
        assert_eq!(parse_iso8601_minutes("PT1H30M"), Some(90));
        assert_eq!(parse_iso8601_minutes("PT45M"), Some(45));
        assert_eq!(parse_iso8601_minutes("PT2H"), Some(120));
        assert_eq!(parse_iso8601_minutes("PT0M"), None);
    }

    #[test]
    fn recipe_block_can_be_embedded_in_graph() {
        let html = r#"<html><head>
        <script type="application/ld+json">
        {"@context":"https://schema.org","@graph":[
          {"@type":"Recipe","name":"X","recipeIngredient":["a"],"recipeInstructions":[{"text":"step"}]}
        ]}
        </script></head></html>"#;
        let md = extract_recipe(html).unwrap().expect("md");
        assert!(md.contains("# X"));
        assert!(md.contains("- a"));
    }
}
