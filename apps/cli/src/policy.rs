//! §53 surfaces: the per-project data policy, and the secret store that keeps values
//! out of SQLite.
//!
//! The database holds a `secret_ref` and nothing else. Values go to the file store
//! under the workbench directory, or to the login keychain when the user has said —
//! out of band, via `VIBE_SECURITY_ALLOW_KEYCHAIN=1` — that they want the real
//! keychain touched. Policy is a human statement too: the CLI is the only thing here
//! that writes it, and it writes it as the user.

use std::io::Read;
use std::path::PathBuf;

use clap::Subcommand;
use vibe_context_engine::policy;
use vibe_core::error::{CoreError, Result};
use vibe_domain::ProjectPolicy;
use vibe_security::{KeychainConsent, KeychainSecretStore, LocalSecretStore, SecretStore};

use crate::app::App;
use crate::context::policy_summary;

/// Settings key holding one project's name → ref index. Refs only, never values, and
/// never shared between projects: a secret stored while working on one project should
/// not show up in another project's list.
fn index_key(project_id: &str) -> String {
    format!("workbench.secrets.{project_id}")
}

#[derive(Subcommand)]
pub enum PolicyCommand {
    /// The rules in force for this project.
    Show,
    /// Change one rule: `--set name=true|false`, repeatable.
    Set {
        #[arg(long = "set")]
        changes: Vec<String>,
    },
    /// Would a pack be allowed to go to this target? Prints the refusal reason if not.
    Check {
        #[arg(long)]
        to: String,
    },
}

#[derive(Subcommand)]
pub enum SecretCommand {
    /// Store a value. Reads stdin when --value is absent, so it never lands in a
    /// shell history line.
    Put {
        name: String,
        #[arg(long)]
        value: Option<String>,
        /// Use the login keychain instead of the file store.
        #[arg(long)]
        keychain: bool,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// The names and refs this project's workbench holds — never the values.
    List {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Print the value back. Reading one is an explicit act.
    Get {
        name: String,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Delete the value, then drop it from the index.
    Rm {
        name: String,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

fn parse_rule(raw: &str) -> Result<(&'static str, bool)> {
    let (name, value) = raw.split_once('=').ok_or_else(|| {
        CoreError::Validation(format!(
            "rules look like name=true|false, got '{raw}' — known names: local-only, \
             allow-cloud-model, never-send-files, never-send-raw-conversation, redact-secrets"
        ))
    })?;
    let key = match name.trim().replace('-', "_").as_str() {
        "local_only" => "local_only",
        "allow_cloud_model" => "allow_cloud_model",
        "never_send_files" => "never_send_files",
        "never_send_raw_conversation" => "never_send_raw_conversation",
        "redact_secrets" => "redact_secrets",
        other => {
            return Err(CoreError::Validation(format!(
                "unknown policy rule '{other}' — local-only, allow-cloud-model, \
                 never-send-files, never-send-raw-conversation, redact-secrets"
            )))
        }
    };
    let enabled = match value.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" => true,
        "false" | "0" | "no" => false,
        other => {
            return Err(CoreError::Validation(format!(
                "'{other}' is not true or false for rule '{name}'"
            )))
        }
    };
    Ok((key, enabled))
}

fn apply(policy: &mut ProjectPolicy, key: &str, enabled: bool) {
    match key {
        "local_only" => policy.local_only = enabled,
        "allow_cloud_model" => policy.allow_cloud_model = enabled,
        "never_send_files" => policy.never_send_files = enabled,
        "never_send_raw_conversation" => policy.never_send_raw_conversation = enabled,
        _ => policy.redact_secrets = enabled,
    }
}

pub fn dispatch_policy(app: &App, project: Option<&str>, command: PolicyCommand) -> Result<()> {
    let id = app.project(project)?;
    match command {
        PolicyCommand::Show => {
            let current = policy::load(&app.repos, &id)?;
            if app.json {
                return app.print_json(&current);
            }
            app.say(format!(
                "local-only: {}\nallow-cloud-model: {}\nnever-send-files: {}\n\
                 never-send-raw-conversation: {}\nredact-secrets: {}\n\nas one line: {}",
                current.local_only,
                current.allow_cloud_model,
                current.never_send_files,
                current.never_send_raw_conversation,
                current.redact_secrets,
                policy_summary(&current),
            ));
            Ok(())
        }
        PolicyCommand::Set { changes } => {
            if changes.is_empty() {
                return Err(CoreError::Validation(
                    "nothing to change: pass --set name=true|false".into(),
                ));
            }
            let mut current = policy::load(&app.repos, &id)?;
            for raw in &changes {
                let (key, enabled) = parse_rule(raw)?;
                apply(&mut current, key, enabled);
            }
            policy::save(&app.repos, &id, &current)?;
            if app.json {
                return app.print_json(&current);
            }
            app.say(format!("policy now: {}", policy_summary(&current)));
            Ok(())
        }
        PolicyCommand::Check { to } => {
            let current = policy::load(&app.repos, &id)?;
            let reason = policy::refusal_reason(&current, Some(&to));
            if app.json {
                return app.print_json(&serde_json::json!({
                    "target": to,
                    "allowed": reason.is_none(),
                    "reason": reason,
                }));
            }
            match reason {
                None => {
                    app.say(format!("{to} is allowed under this project's policy"));
                    Ok(())
                }
                Some(why) => {
                    app.say(format!("{to} is refused: {why}"));
                    Err(CoreError::Validation(format!("{to} is refused: {why}")))
                }
            }
        }
    }
}

/// Where file-backed secrets live: beside the database, never inside it.
fn secrets_dir(app: &App, explicit: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir;
    }
    app.db
        .parent()
        .map(|parent| parent.join("secrets"))
        .unwrap_or_else(|| PathBuf::from("secrets"))
}

/// The store a ref points at, chosen by its prefix — so `get` works whichever backend
/// wrote it.
fn store_for(dir: &std::path::Path, secret_ref: &str) -> Box<dyn SecretStore> {
    if secret_ref.starts_with("keychain:") {
        match KeychainConsent::from_environment() {
            Some(consent) => Box::new(KeychainSecretStore::open(consent, "com.goulan.workbench")),
            None => Box::new(LocalSecretStore::new(dir)),
        }
    } else {
        Box::new(LocalSecretStore::new(dir))
    }
}

fn index(app: &App, project_id: &str) -> Result<Vec<serde_json::Value>> {
    Ok(app
        .repos
        .settings
        .get_json(&index_key(project_id))?
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default())
}

fn save_index(app: &App, project_id: &str, entries: &[serde_json::Value]) -> Result<()> {
    app.repos.settings.set_json(
        &index_key(project_id),
        &serde_json::Value::Array(entries.to_vec()),
    )
}

fn find(entries: &[serde_json::Value], name: &str) -> Option<serde_json::Value> {
    entries
        .iter()
        .find(|entry| entry.get("name").and_then(|v| v.as_str()) == Some(name))
        .cloned()
}

fn read_value(explicit: Option<&str>) -> Result<String> {
    if let Some(value) = explicit {
        return Ok(value.to_string());
    }
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| CoreError::Io(format!("cannot read the value from stdin: {e}")))?;
    let value = buffer.trim_end_matches(['\n', '\r']);
    if value.is_empty() {
        return Err(CoreError::Validation(
            "empty value: pass --value or pipe the secret on stdin".into(),
        ));
    }
    Ok(value.to_string())
}

pub fn dispatch_secret(
    app: &App,
    project: Option<&str>,
    command: SecretCommand,
) -> Result<()> {
    let id = app.project(project)?;
    match command {
        SecretCommand::Put {
            name,
            value,
            keychain,
            dir,
        } => {
            let path = secrets_dir(app, dir);
            let value = read_value(value.as_deref())?;
            let written: Box<dyn SecretStore> = match (keychain, KeychainConsent::from_environment()) {
                (true, Some(consent)) => {
                    Box::new(KeychainSecretStore::open(consent, "com.goulan.workbench"))
                }
                (true, None) => {
                    return Err(CoreError::Validation(format!(
                        "the keychain is opt-in: set {}=1 and run again, or drop --keychain \
                         to use the file store at {}",
                        KeychainConsent::ENV,
                        path.display()
                    )))
                }
                (false, _) => Box::new(LocalSecretStore::new(&path)),
            };
            let secret_ref = written.put(&name, &value)?;
            let mut entries = index(app, &id)?;
            entries.retain(|entry| {
                entry.get("name").and_then(|v| v.as_str()) != Some(name.as_str())
            });
            entries.push(serde_json::json!({
                "name": name,
                "secretRef": secret_ref,
                "store": if secret_ref.starts_with("keychain:") { "keychain" } else { "file" },
            }));
            save_index(app, &id, &entries)?;
            if app.json {
                return app.print_json(&entries.last().expect("just pushed"));
            }
            // A ref is safe to print; the value never is.
            app.say(format!("{name} → {secret_ref}"));
            Ok(())
        }
        SecretCommand::List { dir } => {
            let path = secrets_dir(app, dir);
            let entries = index(app, &id)?;
            if app.json {
                return app.print_json(&entries);
            }
            if entries.is_empty() {
                app.say(format!(
                    "no secrets stored (values would go to {})",
                    path.display()
                ));
                return Ok(());
            }
            for entry in &entries {
                app.say(format!(
                    "{:<24} {:<10} {}",
                    entry.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                    entry.get("store").and_then(|v| v.as_str()).unwrap_or("?"),
                    entry
                        .get("secretRef")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                ));
            }
            Ok(())
        }
        SecretCommand::Get { name, dir } => {
            let path = secrets_dir(app, dir);
            let entry = find(&index(app, &id)?, &name).ok_or_else(|| CoreError::NotFound {
                what: format!("secret '{name}'"),
            })?;
            let secret_ref = entry
                .get("secretRef")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CoreError::Encoding("index entry has no secretRef".into()))?;
            let value = store_for(&path, secret_ref).lookup(secret_ref)?;
            match value {
                Some(value) => {
                    println!("{value}");
                    Ok(())
                }
                None => Err(CoreError::NotFound {
                    what: format!("{secret_ref} is indexed but its value is gone from the store"),
                }),
            }
        }
        SecretCommand::Rm { name, dir } => {
            let path = secrets_dir(app, dir);
            let entries = index(app, &id)?;
            let entry = find(&entries, &name).ok_or_else(|| CoreError::NotFound {
                what: format!("secret '{name}'"),
            })?;
            let secret_ref = entry
                .get("secretRef")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CoreError::Encoding("index entry has no secretRef".into()))?;
            store_for(&path, secret_ref).delete(secret_ref)?;
            let kept: Vec<serde_json::Value> = entries
                .iter()
                .filter(|item| item.get("name").and_then(|v| v.as_str()) != Some(name.as_str()))
                .cloned()
                .collect();
            save_index(app, &id, &kept)?;
            app.say(format!("{name} removed ({secret_ref})"));
            Ok(())
        }
    }
}
