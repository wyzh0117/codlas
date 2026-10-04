# ADR-017 — The system keychain is explicit opt-in; the default secret store is a 0600 file

Status: accepted (records a deliberate reading of V1_DEVELOPMENT §53)
Date: 2026-10-03
Ref: V1_DEVELOPMENT §53, §54; PROJECT_MASTER §19 禁止 8; ADR-016

## Context

§53 states two things at once: "API Key、Token 不进 SQLite 明文" and "使用系统 Keychain。
数据库只保存 secret_ref". The first is an invariant no test or automation may break. The
second names one implementation. Writing to the OS keychain is machine-global shared state:
a test run, a reindex, or any background invocation would either pop a GUI authorization
dialog or leave entries the user did not ask for — the same blast-radius reasoning that keeps
release actions behind explicit confirmation.

## Decision

`SecretStore` (put / lookup / delete / describe) is a port in `crates/security`, with two
implementations:

- `LocalSecretStore` — the default. One `<id>.value` plus `<id>.meta.json` per secret in a
  directory the caller injects (`apps/cli` names `secrets/` beside the database). Files are
  created `create_new` + mode 0600 via `create_private`. The ref is `local:<generated id>`;
  the ref is validated against a lowercase/digit whitelist *before* any path is built, so
  `local:../escape` fails without touching the filesystem.
- `KeychainSecretStore` — macOS `/usr/bin/security add-generic-password -U`, value handed
  over **stdin** so it never appears in an argument vector; the RFC3339 `-C` comment carries
  `createdAt`; statuses 44/45 mean absent. Its only constructor takes a `KeychainConsent`,
  and the only way to obtain one is `VIBE_SECURITY_ALLOW_KEYCHAIN=1` in the environment of
  the process the user started.

`wp secret put --keychain` without the variable refuses with a message naming the variable
and the file-store alternative; `wp secret put` without the flag never touches the keychain.
The database only ever sees `secretRef`; `wp secret list` records name/ref/store in project
settings, and `describe` structurally cannot return a value.

The real-keychain round trip is a manual, `#[ignore]`d test
(`crates/security/tests/keychain_opt_in.rs`):

```
VIBE_SECURITY_ALLOW_KEYCHAIN=1 cargo test -p vibe-security --test keychain_opt_in -- --ignored
```

## Consequences

- V1's out-of-the-box posture is weaker at rest than the keychain: a 0600 file, protected by
  filesystem permissions only. The invariant §53 exists to guarantee — no plaintext in
  SQLite — holds for both stores, and `apps/cli/tests/cli_e2e.rs` proves it by reading every
  byte of the database file and asserting the value is absent while `local:<id>` is present.
- Refs are not interchangeable across stores. `local:` refs are id-keyed (putting the same
  name twice creates two secrets; the settings index keeps the newest), while `keychain:`
  refs are name-keyed and `-U` makes a second put an upsert on one account.
  `SecretMetadata.store` is therefore always shown beside the ref.
- `store_for` (apps/cli/src/policy.rs:195) resolves a ref by prefix. A `keychain:` ref read
  back without the consent variable currently fails through `LocalSecretStore`'s ref
  validation ("not a local secret ref") rather than a message naming the variable — it fails
  closed, but the wording is indirect. Left as is for V1's single-user setting.
- Non-macOS platforms keep the file store; the keychain wrapper is `/usr/bin/security`-bound
  on purpose. A portable hidden-file backend for other platforms would be its own ADR.
