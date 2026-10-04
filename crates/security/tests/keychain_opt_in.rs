//! §53 says secrets belong in the system keychain; this suite says the keychain
//! must never be touched by a test run. `KeychainSecretStore` has exactly one
//! constructor and it needs a [`KeychainConsent`] that only a human's environment
//! can produce, so the assertion below is the proof that nothing here instantiated it.

use vibe_security::{KeychainConsent, KeychainSecretStore, SecretStore};

#[test]
fn the_keychain_store_cannot_be_built_without_explicit_consent() {
    // No test in this workspace sets the variable; if one ever does, this suite is
    // the first to say so rather than quietly writing into a real keychain.
    assert_eq!(KeychainConsent::ENV, "VIBE_SECURITY_ALLOW_KEYCHAIN");
    let consent = KeychainConsent::from_environment();
    assert!(
        consent.is_none(),
        "{} was set while running the test suite — refusing to continue",
        KeychainConsent::ENV
    );
    let without: Option<KeychainSecretStore> = consent.map(|token| {
        KeychainSecretStore::open(token, "com.example.vibe.unused")
    });
    assert!(
        without.is_none(),
        "a keychain store object exists during tests"
    );
}

/// Manual only, and deliberately `#[ignore]`d: this talks to the real macOS
/// keychain and adds then removes one generic password. Run it yourself with
/// `VIBE_SECURITY_ALLOW_KEYCHAIN=1 cargo test -p vibe-security --test keychain_opt_in -- --ignored`
/// if you want to exercise the wrapper against `/usr/bin/security`.
#[test]
#[ignore = "manual: touches the real macOS keychain"]
fn real_keychain_round_trip_is_a_manual_step() {
    let Some(consent) = KeychainConsent::from_environment() else {
        panic!("set {}=1 to run this on purpose", KeychainConsent::ENV);
    };
    let store = KeychainSecretStore::open(consent, "com.example.vibe.workbench.test");
    let reference = store
        .put("vibe-self-check", "not-a-real-secret")
        .expect("keychain write");
    assert_eq!(reference, "keychain:vibe-self-check");
    assert_eq!(
        store.lookup(&reference).unwrap().as_deref(),
        Some("not-a-real-secret")
    );
    let meta = store.describe(&reference).unwrap().expect("metadata");
    assert!(!format!("{meta:?}").contains("not-a-real-secret"));
    assert!(store.delete(&reference).unwrap());
    assert_eq!(store.lookup(&reference).unwrap(), None);
}
