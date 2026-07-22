//! Deterministic PII (personally identifiable information) redaction.
//!
//! Mirrors Firecrawl's PII redaction (4 credits/page) but is fully local,
//! regex-based, and free. Redacts:
//!
//! - Email addresses
//! - Phone numbers (US/international)
//! - Social Security Numbers (US format)
//! - Credit card numbers (13–19 digit groups)

use regex::Regex;

/// Redact common PII patterns from text, replacing them with `[REDACTED]`.
pub fn redact_pii(text: &str) -> String {
    let email = Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap();
    let ssn = Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap();
    let credit_card = Regex::new(r"\b(?:\d[ -]*?){13,19}\b").unwrap();
    let phone = Regex::new(r"\b\+?\d{1,3}?[ .-]?\(?\d{1,4}?\)?[ .-]?\d{3,4}[ .-]?\d{4}\b").unwrap();

    let text = email.replace_all(text, "[REDACTED_EMAIL]");
    let text = ssn.replace_all(&text, "[REDACTED_SSN]");
    let text = credit_card.replace_all(&text, "[REDACTED_CC]");
    let text = phone.replace_all(&text, "[REDACTED_PHONE]");
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email() {
        let result = redact_pii("Contact john@example.com for info");
        assert!(result.contains("[REDACTED_EMAIL]"));
        assert!(!result.contains("john@example.com"));
    }

    #[test]
    fn redacts_multiple_emails() {
        let result = redact_pii("a@b.com and c@d.org");
        assert_eq!(result.matches("[REDACTED_EMAIL]").count(), 2);
    }

    #[test]
    fn redacts_ssn() {
        let result = redact_pii("SSN: 123-45-6789");
        assert!(result.contains("[REDACTED_SSN]"));
        assert!(!result.contains("123-45-6789"));
    }

    #[test]
    fn redacts_credit_card() {
        let result = redact_pii("Card: 4111 1111 1111 1111");
        assert!(result.contains("[REDACTED_CC]"));
    }

    #[test]
    fn redacts_credit_card_with_dashes() {
        let result = redact_pii("Card: 4111-1111-1111-1111");
        assert!(result.contains("[REDACTED_CC]"));
    }

    #[test]
    fn redacts_us_phone() {
        let result = redact_pii("Call (555) 123-4567 today");
        assert!(result.contains("[REDACTED_PHONE]"));
        assert!(!result.contains("(555) 123-4567"));
    }

    #[test]
    fn redacts_international_phone() {
        let result = redact_pii("Call +44 20 7946 0958");
        assert!(result.contains("[REDACTED_PHONE]"));
    }

    #[test]
    fn leaves_normal_text_unchanged() {
        let result = redact_pii("Hello world, this is a normal sentence.");
        assert_eq!(result, "Hello world, this is a normal sentence.");
    }

    #[test]
    fn redacts_all_types_at_once() {
        let text = "Email: a@b.com, SSN: 111-22-3333, Phone: (555) 000-1234, Card: 4111 1111 1111 1111";
        let result = redact_pii(text);
        assert!(result.contains("[REDACTED_EMAIL]"));
        assert!(result.contains("[REDACTED_SSN]"));
        assert!(result.contains("[REDACTED_PHONE]"));
        assert!(result.contains("[REDACTED_CC]"));
    }
}
