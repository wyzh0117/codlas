//! §53 storage half: the value goes behind a reference, the reference is what a
//! project row may hold. Everything here happens inside a `tempfile` directory —
//! no keychain, no user files, no network.

use vibe_security::{LocalSecretStore, SecretStore};

fn store() -> (tempfile::TempDir, LocalSecretStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LocalSecretStore::new(dir.path().join("secrets"));
    (dir, store)
}

#[test]
fn a_value_round_trips_through_a_reference() {
    let (_keep, store) = store();
    let reference = store
        .put("openai-gateway", "sk-secret-value-8842")
        .expect("put");
    assert!(reference.starts_with("local:"), "{reference}");
    assert!(!reference.contains("sk-secret"), "the ref is not the value");
    assert_eq!(
        store.lookup(&reference).unwrap().as_deref(),
        Some("sk-secret-value-8842")
    );
    assert_eq!(store.lookup("local:deadbeef").unwrap(), None);
}

#[test]
fn describe_reports_the_secret_without_its_value() {
    let (_keep, store) = store();
    let reference = store.put("deploy-token", "t0ken-value").unwrap();
    let meta = store.describe(&reference).unwrap().expect("metadata");
    assert_eq!(meta.secret_ref, reference);
    assert_eq!(meta.name, "deploy-token");
    assert_eq!(meta.store, "local");
    assert!(meta.created_at.is_some());
    let shown = format!("{meta:?}");
    assert!(!shown.contains("t0ken-value"), "metadata must never carry a value");

    // Files on disk say the same thing.
    let dir = store.dir();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|v| v.to_str()) == Some("json") {
            let body = std::fs::read_to_string(&path).unwrap();
            assert!(!body.contains("t0ken-value"), "{body}");
        }
    }
}

#[test]
fn two_secrets_with_the_same_name_are_two_secrets() {
    let (_keep, store) = store();
    let first = store.put("dup", "one").unwrap();
    let second = store.put("dup", "two").unwrap();
    assert_ne!(first, second);
    assert_eq!(store.lookup(&first).unwrap().as_deref(), Some("one"));
    assert_eq!(store.lookup(&second).unwrap().as_deref(), Some("two"));
}

#[test]
fn delete_removes_both_files_and_is_idempotent() {
    let (_keep, store) = store();
    let reference = store.put("gone", "value").unwrap();
    assert!(store.delete(&reference).unwrap());
    assert_eq!(store.lookup(&reference).unwrap(), None);
    assert_eq!(store.describe(&reference).unwrap(), None);
    assert!(!store.delete(&reference).unwrap(), "second delete reports nothing");
    assert_eq!(std::fs::read_dir(store.dir()).unwrap().count(), 0);
}

#[test]
fn refs_that_would_escape_the_directory_are_rejected() {
    let (_keep, store) = store();
    for bad in ["../x", "local:../x", "local:a/b", "local:", "other:1", "local:UPPER"] {
        assert!(store.lookup(bad).is_err(), "{bad} should not resolve");
        assert!(store.delete(bad).is_err(), "{bad}");
        assert!(store.describe(bad).is_err(), "{bad}");
    }
    assert!(store.put("  ", "value").is_err(), "an empty name is not a secret");
}

#[cfg(unix)]
#[test]
fn secret_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let (_keep, store) = store();
    let reference = store.put("perm-check", "value").unwrap();
    let id = reference.strip_prefix("local:").unwrap();
    for suffix in ["value", "meta.json"] {
        let path = store.dir().join(format!("{id}.{suffix}"));
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "secrets must not be group or world readable");
    }
}

#[cfg(not(unix))]
#[test]
fn secret_files_are_owner_only_on_unix_only() {
    // Modes are a Unix concept; the assertion above is compiled out elsewhere and
    // the store still works.
    let (_keep, store) = store();
    assert!(store.put("perm-check", "value").is_ok());
}
