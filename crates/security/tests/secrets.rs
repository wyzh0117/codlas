//! §53 detection + redaction over every secret family the brief names. The point
//! of these tests is not "a marker appeared" but "no piece of the original value
//! survived the marker".

use vibe_security::secrets::{API_KEY, AWS_ACCESS_KEY_ID, GITHUB_TOKEN, JWT, PASSWORD, PRIVATE_KEY, SLACK_TOKEN, SECRET, TOKEN};
use vibe_security::{Redaction, SecretScanner};

/// Values shaped like the real thing but inert.
fn samples() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "openai style key",
            "use sk-proj-A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6 for the gateway",
            "A1b2C3d4E5f6G7h8",
        ),
        (
            "aws access key id",
            "AWS_ACCESS_KEY_ID AKIAIOSFODNN7EXAMPLE was in the env file",
            "IOSFODNN7EXAMPLE",
        ),
        (
            "github token ghp",
            "pushed with ghp_16C7e42F292c6912E7710c838347Ae178B4a",
            "16C7e42F292c6912E7710c838347Ae178B4a",
        ),
        (
            "github token gho",
            "oauth gho_9pQ1rStUvWxYz0AbCdEfGhIjKlMnOpQrStUv",
            "9pQ1rStUvWxYz0AbCdEfGhIjKl",
        ),
        (
            "github fine grained",
            "github_pat_11AAGXQ0A2Tj8wVpCd3xLq_9KlsM7pQrStUvWx",
            "9KlsM7pQrStUvWx",
        ),
        (
            "jwt",
            "header eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvIn0.dZj6lTiYHRmg3b7SKNmX7eNDbwmQ7cNfM6Bn3kBxXo expired",
            "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvIn0",
        ),
        (
            "slack token",
            "bot replies on xoxb-123456789012-1234567890123-abcdefABCDEF01234",
            "123456789012-1234567890123",
        ),
        (
            "generic api_key assignment",
            "api_key = qwertyuiopasdfghjklzxcvbnm123456",
            "qwertyuiopasdfghjklzxcvbnm",
        ),
        (
            "generic secret assignment",
            "client_secret: zxcvbnmlkjhgfdcba0987654321qwe",
            "zxcvbnmlkjhgfdcba",
        ),
        (
            "generic token assignment",
            "access_token = 1234567890abcdefABCDEF1234567890",
            "1234567890abcdefABCDEF",
        ),
        (
            "generic password assignment",
            "password = Hunter2ThisIsAVeryLongPasswordValue",
            "ThisIsAVeryLongPasswordValue",
        ),
        (
            "private key block",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEAx7Vm0fW\nqL6yZ3tR9sB1cD4eF7gH0jK3mP6\n-----END RSA PRIVATE KEY-----",
            "MIIEpAIBAAKCAQEAx7Vm0fW",
        ),
    ]
}

#[test]
fn every_family_is_detected() {
    let scanner = SecretScanner::new();
    for (label, text, _) in samples() {
        assert!(
            scanner.contains_secret(text),
            "{label} should read as a secret: {text}"
        );
        let spans = scanner.find(text);
        assert!(!spans.is_empty(), "{label} produced no span");
        for span in &spans {
            assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
            assert!(!span.category.is_empty());
        }
    }
}

#[test]
fn every_family_is_redacted_and_no_fragment_survives() {
    let scanner = SecretScanner::new();
    for (label, text, fragment) in samples() {
        let (clean, redactions) = scanner.redact(text);
        assert!(!redactions.is_empty(), "{label} reported nothing");
        assert!(
            !clean.contains(fragment),
            "{label} left part of the value in the text: {clean}"
        );
        assert!(
            !scanner.contains_secret(&clean),
            "{label} redaction still reads as a secret: {clean}"
        );
        assert!(clean.contains("[REDACTED:"), "{label} lost its marker: {clean}");
    }
}

#[test]
fn categories_are_the_family_labels_the_audit_shows() {
    let scanner = SecretScanner::new();
    let category_of = |text: &str| {
        scanner
            .find(text)
            .first()
            .map(|span| span.category)
            .unwrap_or("<none>")
    };
    assert_eq!(category_of("key sk-proj-A1b2C3d4E5f6G7h8I9j0K1"), API_KEY);
    assert_eq!(category_of("AKIAIOSFODNN7EXAMPLE"), AWS_ACCESS_KEY_ID);
    assert_eq!(
        category_of("ghp_16C7e42F292c6912E7710c838347Ae178B4a"),
        GITHUB_TOKEN
    );
    assert_eq!(
        category_of("github_pat_11AAGXQ0A2Tj8wVpCd3xLq_9KlsM7pQr"),
        GITHUB_TOKEN
    );
    assert_eq!(
        category_of("a.eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dZj6lTiYHRmg3b7SKNm"),
        JWT
    );
    assert_eq!(category_of("xoxp-123456789012-1234567890"), SLACK_TOKEN);
    assert_eq!(category_of("password = Hunter2ThisIsAVeryLongPassword"), PASSWORD);
    assert_eq!(category_of("access_token = 1234567890abcdefABCDEF12345"), TOKEN);
    assert_eq!(category_of("client_secret: zxcvbnmlkjhgfdcba09876543"), SECRET);
    assert_eq!(
        category_of("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----"),
        PRIVATE_KEY
    );
}

#[test]
fn a_leaked_key_is_reported_as_category_and_fingerprint_never_as_value() {
    let scanner = SecretScanner::new();
    let secret = "sk-proj-A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6";
    let (clean, redactions) = scanner.redact(&format!("config: {secret}"));
    let one: &Redaction = &redactions[0];
    assert_eq!(one.category, API_KEY);
    assert_eq!(one.count, 1);
    assert_eq!(one.fingerprint.len(), 12, "short, stable, one-way");
    assert!(!clean.contains(secret));
    assert!(
        !format!("{redactions:?}").contains(secret),
        "a Redaction must not carry the value anywhere"
    );
}

#[test]
fn the_same_secret_twice_is_one_fingerprint_with_a_count() {
    let scanner = SecretScanner::new();
    let text = "sk-proj-A1b2C3d4E5f6G7h8I9j0K1l2 and again sk-proj-A1b2C3d4E5f6G7h8I9j0K1l2";
    let (clean, redactions) = scanner.redact(text);
    assert_eq!(redactions.len(), 1);
    assert_eq!(redactions[0].count, 2);
    assert_eq!(clean.matches("[REDACTED:api_key]").count(), 2);
}

#[test]
fn different_secrets_get_different_fingerprints() {
    let scanner = SecretScanner::new();
    let (_, redactions) = scanner.redact(
        "one sk-proj-A1b2C3d4E5f6G7h8I9j0K1l2 two AKIAIOSFODNN7EXAMPLE three api_key = qwertyuiopasdfghjklzxcvbnm12",
    );
    let prints: Vec<&str> = redactions.iter().map(|r| r.fingerprint.as_str()).collect();
    let unique: std::collections::HashSet<&str> = prints.iter().copied().collect();
    assert_eq!(prints.len(), 3, "{redactions:?}");
    assert_eq!(prints.len(), unique.len(), "fingerprints collided");
}

#[test]
fn clean_text_passes_through_unchanged() {
    let scanner = SecretScanner::new();
    let text = "Decision D-011: keep the repository port so core never names a database.";
    let (clean, redactions) = scanner.redact(text);
    assert_eq!(clean, text);
    assert!(redactions.is_empty());
    assert!(!scanner.contains_secret(text));
    assert!(scanner.find("").is_empty());
}

#[test]
fn cjk_text_keeps_its_byte_boundaries_intact() {
    let scanner = SecretScanner::new();
    let text = format!("用户说：为什么还保留 Repository 层？ api_key = {}", "z".repeat(24));
    let (clean, redactions) = scanner.redact(&text);
    assert_eq!(redactions.len(), 1);
    assert!(clean.starts_with("用户说：为什么还保留 Repository 层？"));
    assert!(clean.contains(&format!("[REDACTED:{}]", API_KEY)));
}
