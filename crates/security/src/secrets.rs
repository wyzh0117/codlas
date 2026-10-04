//! Secret detection and redaction (V1_DEVELOPMENT.md §53).
//!
//! Rules are patterns, not heuristics about length: a span is a byte range in the
//! input, so a caller that needs to know what changed reads the [`Redaction`] list
//! instead of diffing lengths.

use std::sync::OnceLock;

use regex::Regex;

/// Stable category labels. They are what the audit log and the pack show in place
/// of a secret (§54 records what was redacted, never the value).
pub const API_KEY: &str = "api_key";
pub const AWS_ACCESS_KEY_ID: &str = "aws_access_key_id";
pub const GITHUB_TOKEN: &str = "github_token";
pub const JWT: &str = "jwt";
pub const SLACK_TOKEN: &str = "slack_token";
pub const PRIVATE_KEY: &str = "private_key";
pub const SECRET: &str = "secret";
pub const TOKEN: &str = "token";
pub const PASSWORD: &str = "password";
/// Category used by the policy gate when it strips a file body (§53
/// `never_send_files`). Not a secret, but redacted in exactly the same, observable way.
pub const FILE_BODY: &str = "file_body";
/// Category used when `never_send_raw_conversation` replaces a message excerpt.
pub const RAW_CONVERSATION: &str = "raw_conversation";

/// A secret-shaped span of bytes in the caller's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretSpan {
    /// Inclusive byte offset.
    pub start: usize,
    /// Exclusive byte offset.
    pub end: usize,
    pub category: &'static str,
}

/// One thing that was taken out of the text: what kind of secret it was, a
/// one-way fingerprint so repeats are recognisable, and how many times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    pub category: &'static str,
    /// Truncated SHA-256 of the removed span. Not reversible, stable across runs.
    pub fingerprint: String,
    pub count: u32,
}

struct Rule {
    category: &'static str,
    /// Capture group holding the secret; 0 is the whole match.
    value_group: usize,
    pattern: &'static str,
}

/// Order matters: the specific families run before the generic assignment rule so
/// an `api_key = sk-…` line is labelled once, as the family that owns it.
const RULES: &[Rule] = &[
    Rule {
        category: PRIVATE_KEY,
        value_group: 0,
        pattern: r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
    },
    Rule {
        category: PRIVATE_KEY,
        value_group: 0,
        pattern: r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----",
    },
    Rule {
        category: AWS_ACCESS_KEY_ID,
        value_group: 0,
        pattern: r"\b(AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b",
    },
    Rule {
        category: GITHUB_TOKEN,
        value_group: 0,
        pattern: r"\bgh[pousr]_[A-Za-z0-9]{16,}\b",
    },
    Rule {
        category: GITHUB_TOKEN,
        value_group: 0,
        pattern: r"\bgithub_pat_[A-Za-z0-9_]{16,}\b",
    },
    Rule {
        category: JWT,
        value_group: 0,
        pattern: r"\beyJ[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}\b",
    },
    Rule {
        category: SLACK_TOKEN,
        value_group: 0,
        pattern: r"\bxox[baprs]-[A-Za-z0-9_\-]{6,}",
    },
    Rule {
        category: API_KEY,
        value_group: 0,
        pattern: r"\bsk-[A-Za-z0-9_\-]{16,}",
    },
    Rule {
        category: API_KEY,
        value_group: 0,
        pattern: r"\bAIza[0-9A-Za-z_\-]{30,}",
    },
    Rule {
        // Generic `api_key | secret | token | password = <20+ chars>`. Group 3 is
        // the value; the keyword stays in the text so the reader knows what was
        // hidden. The leading `(^|[^…])` stands in for the word boundary the crate
        // would otherwise need, so `client_secret=` matches as well as `secret =`.
        category: API_KEY,
        value_group: 3,
        pattern: r#"(?i)(^|[^A-Za-z0-9_])(api[_-]?key|apikey|access[_-]?key|secret[_-]?key)\s*[:=]\s*["']?([^\s"',;)]{20,})"#,
    },
    Rule {
        category: SECRET,
        value_group: 3,
        pattern: r#"(?i)(^|[^A-Za-z0-9_])(secret|client[_-]?secret)\s*[:=]\s*["']?([^\s"',;)]{20,})"#,
    },
    Rule {
        category: TOKEN,
        value_group: 3,
        pattern: r#"(?i)(^|[^A-Za-z0-9_])(access[_-]?token|auth[_-]?token|token)\s*[:=]\s*["']?([^\s"',;)]{20,})"#,
    },
    Rule {
        category: PASSWORD,
        value_group: 3,
        pattern: r#"(?i)(^|[^A-Za-z0-9_])(password|passwd|pwd)\s*[:=]\s*["']?([^\s"',;)]{20,})"#,
    },
    Rule {
        category: TOKEN,
        value_group: 0,
        pattern: r"(?i)\bbearer\s+[A-Za-z0-9\-_.=]{20,}",
    },
];

/// Compiled once per process, in [`RULES`] order — the index of a regex is the
/// index of its rule, which is how `find` recovers the category and value group.
fn regexes() -> &'static [Regex] {
    static CACHE: OnceLock<Vec<Regex>> = OnceLock::new();
    CACHE.get_or_init(|| {
        RULES
            .iter()
            .map(|rule| Regex::new(rule.pattern).expect("secret pattern must compile"))
            .collect()
    })
}

/// Pure detector. Cheap enough to run on every compiled item (§60: rules first).
#[derive(Debug, Clone, Copy, Default)]
pub struct SecretScanner;

impl SecretScanner {
    pub fn new() -> Self {
        Self
    }

    /// Secret-shaped spans, sorted, never overlapping.
    pub fn find(&self, text: &str) -> Vec<SecretSpan> {
        let mut spans: Vec<SecretSpan> = Vec::new();
        for (index, regex) in regexes().iter().enumerate() {
            let rule = &RULES[index];
            for captures in regex.captures_iter(text) {
                let Some(found) = captures.get(rule.value_group) else {
                    continue;
                };
                let start = snap_back(text, found.start());
                let end = snap_forward(text, found.end());
                if end > start {
                    spans.push(SecretSpan {
                        start,
                        end,
                        category: rule.category,
                    });
                }
            }
        }
        spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut kept: Vec<SecretSpan> = Vec::with_capacity(spans.len());
        let mut cursor = 0usize;
        for span in spans {
            if span.start >= cursor {
                cursor = span.end;
                kept.push(span);
            }
        }
        kept
    }

    /// Replace every span with `[REDACTED:<category>]` and report what was taken out.
    ///
    /// The text length is *not* preserved — callers must read the returned list
    /// rather than assume nothing moved.
    pub fn redact(&self, text: &str) -> (String, Vec<Redaction>) {
        let spans = self.find(text);
        if spans.is_empty() {
            return (text.to_string(), Vec::new());
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0usize;
        let mut redactions: Vec<Redaction> = Vec::new();
        for span in &spans {
            out.push_str(&text[last..span.start]);
            out.push_str(&marker(span.category));
            record(
                &mut redactions,
                span.category,
                &vibe_domain::content_hash(&text[span.start..span.end]),
            );
            last = span.end;
        }
        out.push_str(&text[last..]);
        (out, redactions)
    }

    pub fn contains_secret(&self, text: &str) -> bool {
        !self.find(text).is_empty()
    }
}

/// The literal a redacted span becomes. Kept here so a policy test can assert the
/// marker without guessing its spelling.
pub fn marker(category: &str) -> String {
    format!("[REDACTED:{category}]")
}

/// A new `Redaction`, or `count += 1` on an existing one. Same secret, same
/// fingerprint, so a leaked key repeated in one body is one row.
pub fn record(redactions: &mut Vec<Redaction>, category: &'static str, hash: &str) {
    let fingerprint: String = hash.chars().take(12).collect();
    if let Some(existing) = redactions
        .iter_mut()
        .find(|r| r.category == category && r.fingerprint == fingerprint)
    {
        existing.count += 1;
        return;
    }
    redactions.push(Redaction {
        category,
        fingerprint,
        count: 1,
    });
}

fn snap_back(text: &str, at: usize) -> usize {
    let mut index = at.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn snap_forward(text: &str, at: usize) -> usize {
    let mut index = at.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_category_shaped() {
        assert_eq!(marker(API_KEY), "[REDACTED:api_key]");
    }

    #[test]
    fn spans_never_overlap() {
        let text = "password = sk-abcdefghijklmnopqrstuvwxyz0123456789";
        let spans = SecretScanner::new().find(text);
        assert!(spans.windows(2).all(|pair| pair[0].end <= pair[1].start));
        assert_eq!(spans.len(), 1, "the family that owns the value labels it");
    }
}
