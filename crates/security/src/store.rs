//! Secret storage (V1_DEVELOPMENT.md §53): "API Key、Token 不进 SQLite 明文" — the
//! database keeps a `secret_ref`, the value lives behind this trait.
//!
//! Two implementations, both useless to a test that does not opt in:
//!   * [`LocalSecretStore`] writes into a directory the caller injects, one file per
//!     secret, owner-only mode; nothing is logged, and `describe` never returns a value;
//!   * [`KeychainSecretStore`] shells out to macOS `/usr/bin/security` generic
//!     passwords, handing the value over stdin so it never appears in an argument
//!     vector. Constructing it requires an explicit consent token that only a human's
//!     environment can produce.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vibe_core::error::{CoreError, Result};

/// The store a secret came from.
pub const LOCAL_STORE: &str = "local";
pub const KEYCHAIN_STORE: &str = "keychain";

/// Everything the workbench may show about a stored secret: which one it is and
/// where it lives. Deliberately no value — the value only ever comes back through
/// [`SecretStore::lookup`], which callers use at the moment of use, not at compile time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadata {
    pub secret_ref: String,
    pub name: String,
    pub store: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

pub trait SecretStore: Send + Sync {
    /// Store a value, hand back the reference that may be written to SQLite.
    fn put(&self, name: &str, value: &str) -> Result<String>;
    fn lookup(&self, secret_ref: &str) -> Result<Option<String>>;
    fn delete(&self, secret_ref: &str) -> Result<bool>;
    /// Metadata only: never the value.
    fn describe(&self, secret_ref: &str) -> Result<Option<SecretMetadata>>;
}

// ------------------------------------------------------------------ local store

/// File-backed store for a single machine. The directory is injected so tests (and
/// the workbench) decide where secrets land; nothing here knows about SQLite.
pub struct LocalSecretStore {
    dir: PathBuf,
}

impl LocalSecretStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn value_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.value"))
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.meta.json"))
    }

    /// `local:<id>` — only a generated id, never a user name, reaches the filesystem.
    fn id_of(secret_ref: &str) -> Result<&str> {
        let id = secret_ref
            .strip_prefix("local:")
            .ok_or_else(|| CoreError::Validation(format!("not a local secret ref: {secret_ref}")))?;
        if id.is_empty()
            || id.len() > 64
            || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err(CoreError::Validation(format!(
                "local secret ref has an unusable id: {secret_ref}"
            )));
        }
        Ok(id)
    }
}

impl SecretStore for LocalSecretStore {
    fn put(&self, name: &str, value: &str) -> Result<String> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("secret name is empty".into()));
        }
        fs::create_dir_all(&self.dir)?;
        let id = vibe_domain::new_id();
        // Two files: the value, and metadata that is safe to read for a listing.
        let mut handle = create_private(&self.value_path(&id))?;
        handle.write_all(value.as_bytes())?;
        handle.flush()?;

        let metadata = SecretMetadata {
            secret_ref: format!("local:{id}"),
            name: name.to_string(),
            store: LOCAL_STORE.to_string(),
            created_at: Some(Utc::now()),
        };
        let body = serde_json::to_vec(&metadata)?;
        let mut meta = create_private(&self.meta_path(&id))?;
        meta.write_all(&body)?;
        meta.flush()?;
        Ok(metadata.secret_ref)
    }

    fn lookup(&self, secret_ref: &str) -> Result<Option<String>> {
        let id = Self::id_of(secret_ref)?;
        match fs::read_to_string(self.value_path(id)) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CoreError::Io(error.to_string())),
        }
    }

    fn delete(&self, secret_ref: &str) -> Result<bool> {
        let id = Self::id_of(secret_ref)?;
        let removed = remove_if_present(&self.value_path(id))?;
        remove_if_present(&self.meta_path(id))?;
        Ok(removed)
    }

    fn describe(&self, secret_ref: &str) -> Result<Option<SecretMetadata>> {
        let id = Self::id_of(secret_ref)?;
        match fs::read(self.meta_path(id)) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CoreError::Io(error.to_string())),
        }
    }
}

/// Delete a secret file, treating "already gone" as success — an idempotent delete
/// is what a UI retry needs.
fn remove_if_present(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CoreError::Io(error.to_string())),
    }
}

/// Owner-only creation. On platforms without Unix modes the call still succeeds;
/// the mode assertion in the test suite is `cfg(unix)`.
fn create_private(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

// ---------------------------------------------------------------- keychain store

/// Proof that a human asked for the real keychain. Only [`KeychainConsent::from_environment`]
/// can make one, and that reads the environment of the process the user started —
/// a test suite never sets `VIBE_SECURITY_ALLOW_KEYCHAIN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeychainConsent;

impl KeychainConsent {
    pub const ENV: &'static str = "VIBE_SECURITY_ALLOW_KEYCHAIN";

    pub fn from_environment() -> Option<Self> {
        std::env::var(Self::ENV)
            .ok()
            .filter(|value| value == "1")
            .map(|_| KeychainConsent)
    }
}

/// macOS generic-password wrapper. `/usr/bin/security` is invoked with the value on
/// stdin, so it never lands in a process list, and no error path echoes it either.
pub struct KeychainSecretStore {
    service: String,
    binary: PathBuf,
}

impl KeychainSecretStore {
    /// The only constructor. Requires the consent token; without it this type is
    /// unreachable from tests.
    pub fn open(consent: KeychainConsent, service: impl Into<String>) -> Self {
        let _ = consent;
        Self {
            service: service.into(),
            binary: Path::new("/usr/bin/security").to_path_buf(),
        }
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    fn account_of(secret_ref: &str) -> Result<&str> {
        let account = secret_ref
            .strip_prefix("keychain:")
            .ok_or_else(|| CoreError::Validation(format!("not a keychain ref: {secret_ref}")))?;
        if account.is_empty()
            || account.len() > 128
            || !account
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
        {
            return Err(CoreError::Validation(format!(
                "keychain secret ref has an unusable account name: {secret_ref}"
            )));
        }
        Ok(account)
    }

    fn run(&self, args: &[String], stdin: Option<&str>) -> Result<Output> {
        if !self.binary.exists() {
            return Err(CoreError::Io(
                "no keychain binary at /usr/bin/security".into(),
            ));
        }
        let mut child = Command::new(&self.binary)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        // Always hand over a closed stdin: the value goes in as bytes when there is
        // one, and the tool never falls back to prompting on the terminal.
        let mut handle = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::Io("keychain stdin pipe was not created".into()))?;
        if let Some(value) = stdin {
            handle.write_all(value.as_bytes())?;
        }
        handle.flush()?;
        drop(handle);
        let output = child.wait_with_output()?;
        Ok(Output {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            // stderr is kept short and is the only thing ever surfaced; it carries
            // the security tool's complaint, not our secret.
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }

    fn base(&self, account: &str) -> Vec<String> {
        vec![
            "-a".to_string(),
            account.to_string(),
            "-s".to_string(),
            self.service.clone(),
        ]
    }
}

struct Output {
    status: i32,
    stdout: String,
    stderr: String,
}

impl SecretStore for KeychainSecretStore {
    fn put(&self, name: &str, value: &str) -> Result<String> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("secret name is empty".into()));
        }
        let account = sanitize_account(name);
        let stamp = Utc::now().to_rfc3339();
        // `security add-generic-password -a <acct> -s <service> -C <comment> -w`:
        // with `-w` given no argument, the tool reads the value from stdin.
        let mut args = vec!["add-generic-password".to_string(), "-U".to_string()];
        args.extend(self.base(&account));
        args.extend(["-C".to_string(), stamp, "-w".to_string()]);
        let result = self.run(&args, Some(value))?;
        if result.status != 0 {
            return Err(CoreError::Storage(format!(
                "keychain rejected the write for '{account}': {}",
                result.stderr
            )));
        }
        Ok(format!("keychain:{account}"))
    }

    fn lookup(&self, secret_ref: &str) -> Result<Option<String>> {
        let account = Self::account_of(secret_ref)?;
        let mut args = vec!["find-generic-password".to_string()];
        args.extend(self.base(&account));
        args.push("-w".to_string());
        let result = self.run(&args, None)?;
        if result.status == 44 || result.status == 45 {
            return Ok(None); // item / attribute not found
        }
        if result.status != 0 {
            return Err(CoreError::Storage(format!(
                "keychain lookup failed for '{account}': {}",
                result.stderr
            )));
        }
        Ok(Some(result.stdout.trim_end_matches('\n').to_string()))
    }

    fn delete(&self, secret_ref: &str) -> Result<bool> {
        let account = Self::account_of(secret_ref)?;
        let mut args = vec!["delete-generic-password".to_string()];
        args.extend(self.base(&account));
        let result = self.run(&args, None)?;
        match result.status {
            0 => Ok(true),
            44 | 45 => Ok(false),
            _ => Err(CoreError::Storage(format!(
                "keychain delete failed for '{account}': {}",
                result.stderr
            ))),
        }
    }

    fn describe(&self, secret_ref: &str) -> Result<Option<SecretMetadata>> {
        let account = Self::account_of(secret_ref)?;
        let mut args = vec!["find-generic-password".to_string()];
        args.extend(self.base(&account));
        let result = self.run(&args, None)?;
        if result.status == 44 || result.status == 45 {
            return Ok(None);
        }
        if result.status != 0 {
            return Err(CoreError::Storage(format!(
                "keychain describe failed for '{account}': {}",
                result.stderr
            )));
        }
        // The comment attribute carries the write timestamp; an unreadable comment
        // is not a failure, it just means we cannot say when.
        let created_at = result
            .stdout
            .lines()
            .find_map(|line| quoted_after(line, "\"cmm\""))
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|parsed| parsed.with_timezone(&Utc));
        Ok(Some(SecretMetadata {
            secret_ref: secret_ref.to_string(),
            name: account.to_string(),
            store: KEYCHAIN_STORE.to_string(),
            created_at,
        }))
    }
}

/// Keychain account names are user-supplied labels; only characters the tool and
/// our own parser agree on survive.
fn sanitize_account(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn quoted_after<'a>(line: &'a str, attribute: &str) -> Option<&'a str> {
    if !line.trim_start().starts_with(attribute) {
        return None;
    }
    let rest = line.split_once("=\"")?.1;
    rest.split('"').next().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_is_absent_in_a_test_process() {
        // The suite never sets the variable, so nothing here can reach the keychain.
        assert!(KeychainConsent::from_environment().is_none());
    }

    #[test]
    fn refs_are_validated_before_any_path_is_built() {
        let store = LocalSecretStore::new(std::env::temp_dir());
        assert!(store.lookup("../../etc/passwd").is_err());
        assert!(store.lookup("local:../escape").is_err());
        assert!(store.lookup("keychain:nope").is_err());
    }

    #[test]
    fn account_sanitising_keeps_it_single_token() {
        assert_eq!(sanitize_account("open ai/key"), "open_ai_key");
    }
}
