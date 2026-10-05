//! Stage F commands: compile a Context Pack, look at what the gate would withhold,
//! prepare the pack for an agent, and ask which agent fits (§41-§44, §53, §59, §62).
//!
//! The CLI never sends anything. `prepare` renders the payload and prints it — the
//! user moving it into another agent is their action (§7.8), and the copy fallback is
//! the honest end of the line when no adapter is wired up (§62).

use std::path::PathBuf;

use clap::{Args, Subcommand};
use vibe_context_engine::{
    ContextCompiler, ContextRequest, Excerpt, Selection, DEFAULT_TOTAL_TOKENS,
};
use vibe_core::error::{CoreError, Result};

use crate::app::App;
use crate::views::one_line;

/// The parts of a request a caller can state on the command line.
#[derive(Args)]
pub struct Ask {
    /// The user's own words. §41 puts them in the pack verbatim, §43 never truncates
    /// them, so this is the one thing a pack is built around.
    #[arg(long)]
    question: Option<String>,
    /// `kind:id` — selected context, §42.1, the top of the ladder. Repeatable.
    #[arg(long = "select")]
    selected: Vec<String>,
    /// Ranking terms only: they order what retrieval found, they do not widen it.
    #[arg(long)]
    focus: Vec<String>,
    /// Recent items per §42 tier. A bounded window, never the whole history (§59).
    #[arg(long)]
    window: Option<usize>,
    /// Total tokens for the §43 shares. Defaults to a stated number rather than to an
    /// unbounded pack.
    #[arg(long)]
    tokens: Option<usize>,
    /// `kind:id=path` — text this process already holds, offered for one reference
    /// line. The engine reads no files itself (§22); §53 strips these again when the
    /// project says `neverSendFiles`.
    #[arg(long = "excerpt")]
    excerpts: Vec<String>,
}

#[derive(Subcommand)]
pub enum ContextCommand {
    /// Compile a pack and print it, or write it with --out.
    Build {
        #[command(flatten)]
        ask: Ask,
        /// Where the pack is headed, for the §53 gate.
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// What would travel, what was withheld and why. Writes no pack, saves nothing.
    Plan {
        #[command(flatten)]
        ask: Ask,
        #[arg(long)]
        to: Option<String>,
    },
    /// Compile, then render the payload for one agent and print it for copying.
    Prepare {
        #[command(flatten)]
        ask: Ask,
        #[arg(long)]
        to: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Which adapter suits this request, and whether the project's policy allows it.
    Route {
        #[command(flatten)]
        ask: Ask,
    },
}

/// `kind:id` in, the pair out. Public because the desktop shell focuses a pack with the same
/// reference grammar a user types on the command line — one rule, not a second spelling of it.
pub fn parse_ref(raw: &str, what: &str) -> Result<(String, String)> {
    let (kind, id) = raw
        .split_once(':')
        .ok_or_else(|| CoreError::Validation(format!("{what} looks like kind:id, got '{raw}'")))?;
    if kind.trim().is_empty() || id.trim().is_empty() {
        return Err(CoreError::Validation(format!(
            "{what} needs both a kind and an id: '{raw}'"
        )));
    }
    Ok((kind.trim().to_string(), id.trim().to_string()))
}

/// One `--excerpt kind:id=path` into the pair the request carries.
fn parse_excerpt(raw: &str) -> Result<Excerpt> {
    let (reference, path) = raw.split_once('=').ok_or_else(|| {
        CoreError::Validation("excerpts look like kind:id=path, e.g. artifact:A-1=./src/lib.rs".into())
    })?;
    let (kind, id) = parse_ref(reference, "excerpt reference")?;
    let text = std::fs::read_to_string(path).map_err(|error| CoreError::NotFound {
        what: format!("{path}: {error}"),
    })?;
    Ok(Excerpt::new(format!("{kind}:{id}"), text))
}

fn request(project_id: &str, ask: &Ask, to: Option<&str>) -> Result<ContextRequest> {
    let mut req = match ask.question.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => ContextRequest::new(project_id, text),
        _ => {
            if ask.selected.is_empty() {
                return Err(CoreError::Validation(
                    "nothing to compile: pass --question, or --select kind:id".into(),
                ));
            }
            // No prose from the user: the selection is the intent, stated exactly.
            ContextRequest::new(
                project_id,
                format!("context for {}", ask.selected.join(", ")),
            )
        }
    };
    for raw in &ask.selected {
        let (kind, id) = parse_ref(raw, "selection")?;
        req = req.select(Selection::new(kind, id));
    }
    for term in &ask.focus {
        req = req.focused_on(term.clone());
    }
    for raw in &ask.excerpts {
        req = req.attach_excerpt(parse_excerpt(raw)?);
    }
    if let Some(window) = ask.window {
        req = req.with_window(window);
    }
    let total = ask.tokens.unwrap_or(DEFAULT_TOTAL_TOKENS);
    req = req.with_budget(ContextCompiler::default_budget(total));
    if let Some(target) = to {
        req = req.for_agent(target);
    }
    Ok(req)
}

pub fn dispatch(app: &App, project: Option<&str>, command: ContextCommand) -> Result<()> {
    let id = app.project(project)?;
    let compiler = ContextCompiler::new(app.repos.clone());
    // §42.1 resolves a selection through the entity store, and those rows are the fold's own
    // cache: a record accepted since the last `wp map` or snapshot would otherwise read back as
    // "not found in this project". Materializing first is the same write-through the map compile
    // already does, not a second state (ADR-030).
    vibe_state_engine::Reducer::new(app.repos.clone()).project_state(&id)?;
    match command {
        ContextCommand::Build { ask, to, out } => {
            let req = request(&id, &ask, to.as_deref())?;
            let pack = compiler.build(&req)?;
            if let Some(path) = &out {
                crate::app::write_file(path, pack.markdown.as_bytes())?;
            }
            if app.json {
                return app.print_json(&pack);
            }
            if out.is_none() {
                println!("{}", pack.markdown);
            }
            app.say(format!(
                "\n[{}] {} items, {} token estimate, {} redacted, policy: {}",
                pack.id,
                pack.items.len(),
                pack.token_estimate,
                pack.redacted_count,
                policy_summary(&pack.policy_applied)
            ));
            if let Some(path) = &out {
                app.say(format!("written to {}", path.display()));
            }
            Ok(())
        }
        ContextCommand::Plan { ask, to } => {
            let req = request(&id, &ask, to.as_deref())?;
            let planned = compiler.plan(&req)?;
            if app.json {
                return app.print_json(
                    &planned
                        .iter()
                        .map(|item| {
                            serde_json::json!({
                                "section": item.section.heading(),
                                "sourceRef": item.source_ref,
                                "priority": item.priority,
                                "tokens": item.estimated_tokens,
                                "kept": item.kept,
                                "reason": item.reason,
                            })
                        })
                        .collect::<Vec<_>>(),
                );
            }
            let kept = planned.iter().filter(|item| item.kept).count();
            app.say(format!(
                "{kept} of {} item(s) would travel:\n",
                planned.len()
            ));
            for item in planned.iter().filter(|item| item.kept) {
                app.say(format!(
                    "  {:>2} {:<26} {:>5} tok  {}",
                    item.priority,
                    item.section.heading(),
                    item.estimated_tokens,
                    item.source_ref
                ));
            }
            let held: Vec<_> = planned.iter().filter(|item| !item.kept).collect();
            if !held.is_empty() {
                app.say("\nheld back:");
                for item in held {
                    app.say(format!(
                        "  {:>2} {:<26} {:>5} tok  {} — {}",
                        item.priority,
                        item.section.heading(),
                        item.estimated_tokens,
                        item.source_ref,
                        item.reason.as_deref().unwrap_or("no reason recorded"),
                    ));
                }
            }
            Ok(())
        }
        ContextCommand::Prepare { ask, to, out } => {
            let req = request(&id, &ask, Some(to.as_str()))?;
            let pack = compiler.build(&req)?;
            let payload = compiler.prepare(&pack, &to)?;
            if let Some(path) = &out {
                crate::app::write_file(path, payload.text.as_bytes())?;
            }
            let fallback = vibe_ingestion::fallback_for(
                &payload.target,
                &payload.adapter_id,
                out.as_ref().and_then(|p| p.to_str()),
            );
            if app.json {
                return app.print_json(&serde_json::json!({
                    "adapterId": payload.adapter_id,
                    "target": payload.target,
                    "text": payload.text,
                    "deepLink": payload.deep_link,
                    "copyFallback": payload.copy_fallback,
                    "fallback": fallback,
                    "packId": pack.id,
                }));
            }
            println!("{}", payload.text);
            app.say(format!(
                "\nprepared for {} ({} tokens) — {}",
                one_line(&payload.adapter_id),
                pack.token_estimate,
                fallback.instruction
            ));
            if let Some(path) = &fallback.payload_path {
                app.say(format!("payload written to {path} — that is the file to carry"));
            }
            Ok(())
        }
        ContextCommand::Route { ask } => {
            let req = request(&id, &ask, None)?;
            let picks = compiler.recommend(&req, None)?;
            if app.json {
                return app.print_json(
                    &picks
                        .iter()
                        .map(|pick| {
                            serde_json::json!({
                                "adapterHint": pick.adapter_hint,
                                "target": format!("{:?}", pick.target),
                                "suitedFor": pick.suited_for,
                                "reason": pick.reason,
                                "allowedByPolicy": pick.allowed_by_policy,
                                "score": pick.score,
                            })
                        })
                        .collect::<Vec<_>>(),
                );
            }
            for pick in picks {
                let blocked = if pick.allowed_by_policy {
                    String::new()
                } else {
                    " — blocked by §53 policy".to_string()
                };
                app.say(format!(
                    "{:>3}  {:<14} {:?}  {} ({}){}",
                    pick.score,
                    pick.adapter_hint,
                    pick.target,
                    pick.suited_for,
                    one_line(&pick.reason),
                    blocked
                ));
            }
            Ok(())
        }
    }
}

pub fn policy_summary(policy: &vibe_domain::ProjectPolicy) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if policy.local_only {
        parts.push("local-only");
    }
    if policy.allow_cloud_model {
        parts.push("cloud");
    }
    if policy.never_send_files {
        parts.push("no-files");
    }
    if policy.never_send_raw_conversation {
        parts.push("no-raw-conversation");
    }
    if policy.redact_secrets {
        parts.push("redact-secrets");
    }
    if parts.is_empty() {
        return "unrestricted".to_string();
    }
    parts.join(", ")
}
