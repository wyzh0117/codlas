//! `wp` — the workbench CLI. V1_DEVELOPMENT.md §61 stages B, C, D, E, F, G, H run
//! through this binary; the desktop UI is a later surface over the same services.
//!
//! The binary holds no logic of its own. Every command is a thin call into a crate,
//! which is what makes the golden test (`tests/golden`) and this CLI two views of the
//! same closed loop rather than two implementations of it.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use vibe_cli::app::{self, App};
use vibe_cli::{backup, baseline, capture, context, export, host, policy, views};
use vibe_core::error::Result;

#[derive(Parser)]
#[command(
    name = "wp",
    version,
    about = "Personal Vibe Coding project workbench — continuity and design fidelity",
    long_about = "Records a project as an event log, projects it into a materialized state, \
                  compiles the PROJECT_MAP from that state, and warns when development \
                  drifts from the charter the user wrote."
)]
struct Cli {
    /// Workbench database. Defaults to $WP_DB, then ./.vibe/workbench.db.
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Project to act on. Defaults to the last one used, or the only one present.
    #[arg(long, global = true)]
    project: Option<String>,
    /// Emit machine-readable JSON on stdout instead of human text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

/// The groups are flattened so the verbs read as `wp charter set`,
/// `wp import transcript`, `wp drift check`, `wp context build` rather than carrying
/// a group word.
#[derive(Subcommand)]
enum Command {
    /// Project, charter, scope, architecture baseline — the user-owned layer (§17-§19).
    #[command(flatten)]
    Baseline(baseline::BaselineCommand),
    /// Capture from adapters, review what the judge was unsure about (§27, §36-§38).
    #[command(flatten)]
    Capture(capture::CaptureCommand),
    /// Read the projections: state, map, timeline, drift, inbox, search, audit.
    #[command(flatten)]
    View(views::ViewCommand),
    /// Compile a context pack, plan one, prepare it for an agent (§40-§44, §59).
    #[command(subcommand)]
    Context(context::ContextCommand),
    /// The §53 rules and the secret store that keeps values out of SQLite.
    #[command(subcommand)]
    Policy(policy::PolicyCommand),
    /// Hand a secret to the store and keep only its reference here.
    #[command(subcommand)]
    Secret(policy::SecretCommand),
    /// §55: the six exports, or all of them in one `.projectpack`.
    #[command(subcommand)]
    Export(export::ExportCommand),
    /// §55 / §62 Recovery: back the whole workbench up, verify a file, restore it.
    #[command(subcommand)]
    Backup(backup::BackupCommand),
    /// §34/§35: the browser's door — serve the Native Messaging channel, or write the
    /// Chrome record that launches it.
    #[command(subcommand)]
    Host(host::HostSubcommand),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("wp: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let db = cli.db.unwrap_or_else(app::default_db);
    let mut runtime = App::open(&db)?;
    runtime.json = cli.json;
    let project = cli.project.as_deref();
    match cli.command {
        Command::Baseline(inner) => baseline::dispatch(&runtime, project, inner),
        Command::Capture(inner) => capture::dispatch(&runtime, project, inner),
        Command::View(inner) => views::dispatch(&runtime, project, inner),
        Command::Context(inner) => context::dispatch(&runtime, project, inner),
        Command::Policy(inner) => policy::dispatch_policy(&runtime, project, inner),
        Command::Secret(inner) => policy::dispatch_secret(&runtime, project, inner),
        Command::Export(inner) => export::dispatch(&runtime, project, inner),
        Command::Backup(inner) => backup::dispatch(&runtime, inner),
        Command::Host(inner) => host::dispatch(&runtime, project, inner),
    }
}
