//! Stage B commands: the project and its three user-owned baselines
//! (V1_DEVELOPMENT.md §17 Charter, §18 Scope, §19 Architecture Baseline, §34 Lock).
//!
//! Everything here writes as `ActorType::User` — the CLI is the user's hands. There is
//! no flag to claim otherwise, because a command that could lie about its actor would
//! undo the lock the same section demands.

use clap::Subcommand;
use serde::Deserialize;
use vibe_core::project_service::{BaselineDraft, CharterDraft, ProjectService};
use vibe_domain::{ActorType, ArchitectureInvariant, ArchitectureLayer, ScopeSection};

use crate::app::{read_json, App};

/// One file that establishes a project and its baselines at once (§61 Stage B).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSpec {
    pub project: ProjectHead,
    #[serde(default)]
    pub charter: Option<CharterSpec>,
    #[serde(default)]
    pub scope: Option<ScopeSpec>,
    #[serde(default)]
    pub baseline: Option<BaselineSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHead {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default)]
    pub current_phase: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CharterSpec {
    pub original_problem: String,
    pub target_user: Option<String>,
    pub core_value: String,
    pub must_have: Vec<String>,
    pub wont_have: Vec<String>,
    pub success_criteria: Vec<String>,
    pub architecture_principles: Vec<String>,
    pub constraints: Vec<String>,
    pub locked: bool,
}

impl From<CharterSpec> for CharterDraft {
    fn from(spec: CharterSpec) -> Self {
        Self {
            original_problem: spec.original_problem,
            target_user: spec.target_user,
            core_value: spec.core_value,
            must_have: spec.must_have,
            wont_have: spec.wont_have,
            success_criteria: spec.success_criteria,
            architecture_principles: spec.architecture_principles,
            constraints: spec.constraints,
            locked: spec.locked,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopeSpec {
    pub in_scope: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub deferred: Vec<String>,
    pub forbidden: Vec<String>,
}

impl ScopeSpec {
    /// Each entry becomes an add for its section; `update_scope` guarantees an item
    /// lives in at most one section.
    pub fn additions(&self) -> Vec<(ScopeSection, String)> {
        let mut adds = Vec::new();
        for (section, items) in [
            (ScopeSection::InScope, &self.in_scope),
            (ScopeSection::OutOfScope, &self.out_of_scope),
            (ScopeSection::Deferred, &self.deferred),
            (ScopeSection::Forbidden, &self.forbidden),
        ] {
            adds.extend(items.iter().map(|item| (section, item.clone())));
        }
        adds
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaselineSpec {
    pub summary: String,
    pub layers: Vec<LayerSpec>,
    pub invariants: Vec<InvariantSpec>,
    pub replaceable_parts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSpec {
    pub id: String,
    pub name: String,
    pub responsibility: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvariantSpec {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub detect: Vec<String>,
}

impl From<BaselineSpec> for BaselineDraft {
    fn from(spec: BaselineSpec) -> Self {
        Self {
            summary: spec.summary,
            layers: spec
                .layers
                .into_iter()
                .map(|l| ArchitectureLayer {
                    id: l.id,
                    name: l.name,
                    responsibility: Some(l.responsibility),
                })
                .collect(),
            invariants: spec
                .invariants
                .into_iter()
                .map(|i| ArchitectureInvariant {
                    id: i.id,
                    text: i.text,
                    detect: i.detect,
                })
                .collect(),
            replaceable_parts: spec.replaceable_parts,
        }
    }
}

#[derive(Subcommand)]
pub enum BaselineCommand {
    /// Create a project and, from one JSON file, its charter, scope and baseline.
    Init {
        #[arg(long)]
        file: std::path::PathBuf,
    },
    #[command(subcommand)]
    Project(ProjectCommand),
    #[command(subcommand)]
    Charter(CharterCommand),
    #[command(subcommand)]
    Scope(ScopeCommand),
    #[command(subcommand)]
    Baseline(BaselineSet),
}

#[derive(Subcommand)]
pub enum ProjectCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        phase: Option<String>,
    },
    List,
    /// Point later commands at this project.
    Use { id: String },
    Phase {
        #[arg(long)]
        phase: String,
    },
}

#[derive(Subcommand)]
pub enum CharterCommand {
    Set {
        #[arg(long)]
        file: std::path::PathBuf,
    },
    Show,
    /// Lock so only the user may write it again (§34).
    Lock {
        #[arg(long)]
        unlock: bool,
    },
    /// Every version ever written, oldest first (§9 versioned baselines).
    History,
}

#[derive(Subcommand)]
pub enum ScopeCommand {
    Set {
        #[arg(long)]
        file: std::path::PathBuf,
    },
    Show,
    Add {
        #[arg(long)]
        section: String,
        item: String,
    },
    Remove {
        #[arg(long)]
        section: String,
        item: String,
    },
    History,
}

#[derive(Subcommand)]
pub enum BaselineSet {
    Set {
        #[arg(long)]
        file: std::path::PathBuf,
    },
    Show,
    History,
}

pub fn dispatch(
    app: &App,
    project: Option<&str>,
    command: BaselineCommand,
) -> vibe_core::error::Result<()> {
    let service = ProjectService::new(app.repos.clone());
    match command {
        BaselineCommand::Init { file } => init(app, &service, &file),
        BaselineCommand::Project(inner) => project_cmd(app, &service, project, inner),
        BaselineCommand::Charter(inner) => charter_cmd(app, &service, project, inner),
        BaselineCommand::Scope(inner) => scope_cmd(app, &service, project, inner),
        BaselineCommand::Baseline(inner) => baseline_cmd(app, &service, project, inner),
    }
}

fn init(app: &App, service: &ProjectService, file: &std::path::Path) -> vibe_core::error::Result<()> {
    let spec: ProjectSpec = read_json(file)?;
    let project = service.create_project(
        &spec.project.name,
        spec.project.description.clone(),
        spec.project.root_path.clone(),
        spec.project.current_phase.clone(),
    )?;
    app.remember_project(&project.id)?;

    let mut written = vec!["project".to_string()];
    if let Some(charter) = spec.charter {
        service.save_charter(&project.id, charter.into(), ActorType::User)?;
        written.push("charter".into());
    }
    if let Some(scope) = spec.scope {
        service.update_scope(&project.id, scope.additions(), Vec::new(), ActorType::User)?;
        written.push("scope".into());
    }
    if let Some(baseline) = spec.baseline {
        service.set_baseline(&project.id, baseline.into(), ActorType::User)?;
        written.push("baseline".into());
    }

    app.say(format!("{} · {}", project.id, project.name));
    app.say(format!("wrote: {}", written.join(", ")));
    if !app.json {
        return Ok(());
    }
    app.print_json(&serde_json::json!({
        "projectId": project.id,
        "name": project.name,
        "written": written,
    }))
}

fn project_cmd(
    app: &App,
    service: &ProjectService,
    project: Option<&str>,
    command: ProjectCommand,
) -> vibe_core::error::Result<()> {
    match command {
        ProjectCommand::Create {
            name,
            description,
            root,
            phase,
        } => {
            let created = service.create_project(&name, description, root, phase)?;
            app.remember_project(&created.id)?;
            app.say(created.id.clone());
            if app.json {
                app.print_json(&created)?;
            }
            Ok(())
        }
        ProjectCommand::List => {
            let projects = service.list_projects()?;
            if app.json {
                return app.print_json(&projects);
            }
            if projects.is_empty() {
                app.say("no projects");
            }
            for p in projects {
                app.say(format!(
                    "{}\t{}\t{}",
                    p.id,
                    p.name,
                    p.current_phase.unwrap_or_else(|| "-".into())
                ));
            }
            Ok(())
        }
        ProjectCommand::Use { id } => {
            service.project(&id)?;
            app.remember_project(&id)?;
            app.say(id);
            Ok(())
        }
        ProjectCommand::Phase { phase } => {
            let id = app.project(project)?;
            let updated = service.update_phase(&id, &phase)?;
            if app.json {
                return app.print_json(&updated);
            }
            app.say(format!("{} · {}", updated.id, updated.current_phase.unwrap_or_default()));
            Ok(())
        }
    }
}

fn charter_cmd(
    app: &App,
    service: &ProjectService,
    project: Option<&str>,
    command: CharterCommand,
) -> vibe_core::error::Result<()> {
    let id = app.project(project)?;
    match command {
        CharterCommand::Set { file } => {
            let spec: CharterSpec = read_json(&file)?;
            let charter = service.save_charter(&id, spec.into(), ActorType::User)?;
            if app.json {
                return app.print_json(&charter);
            }
            app.say(format!(
                "charter v{} {}",
                charter.version,
                if charter.locked { "locked" } else { "unlocked" }
            ));
            Ok(())
        }
        CharterCommand::Show => match service.charter(&id)? {
            Some(charter) => app.print_json(&charter),
            None => {
                app.say("no charter");
                Ok(())
            }
        },
        CharterCommand::Lock { unlock } => {
            let charter = service.lock_charter(&id, !unlock)?;
            if app.json {
                return app.print_json(&charter);
            }
            app.say(format!(
                "charter v{} is now {}",
                charter.version,
                if charter.locked { "locked" } else { "unlocked" }
            ));
            Ok(())
        }
        CharterCommand::History => {
            let versions = app.repos.projects.list_charter_versions(&id)?;
            if app.json {
                return app.print_json(&versions);
            }
            for c in versions {
                app.say(format!(
                    "v{}\t{}\t{}",
                    c.version,
                    if c.locked { "locked" } else { "-" },
                    crate::views::one_line(&c.core_value)
                ));
            }
            Ok(())
        }
    }
}

fn section_of(name: &str) -> vibe_core::error::Result<ScopeSection> {
    let normalized = name.trim().to_lowercase();
    let mapped = match normalized.as_str() {
        "in" | "inscope" | "in-scope" => ScopeSection::InScope,
        "out" | "outofscope" | "out-of-scope" => ScopeSection::OutOfScope,
        "deferred" => ScopeSection::Deferred,
        "forbidden" => ScopeSection::Forbidden,
        other => {
            return Err(vibe_core::CoreError::Validation(format!(
                "unknown scope section '{other}' — use inScope, outOfScope, deferred or forbidden"
            )))
        }
    };
    Ok(mapped)
}

fn scope_cmd(
    app: &App,
    service: &ProjectService,
    project: Option<&str>,
    command: ScopeCommand,
) -> vibe_core::error::Result<()> {
    let id = app.project(project)?;
    match command {
        ScopeCommand::Set { file } => {
            let spec: ScopeSpec = read_json(&file)?;
            let scope = service.update_scope(&id, spec.additions(), Vec::new(), ActorType::User)?;
            if app.json {
                return app.print_json(&scope);
            }
            app.say(format!("scope v{}", scope.version));
            Ok(())
        }
        ScopeCommand::Show => match service.scope(&id)? {
            Some(scope) => app.print_json(&scope),
            None => {
                app.say("no scope boundary");
                Ok(())
            }
        },
        ScopeCommand::Add { section, item } => {
            let scope = service.update_scope(
                &id,
                vec![(section_of(&section)?, item)],
                Vec::new(),
                ActorType::User,
            )?;
            scope_line(app, &scope)
        }
        ScopeCommand::Remove { section, item } => {
            let scope = service.update_scope(
                &id,
                Vec::new(),
                vec![(section_of(&section)?, item)],
                ActorType::User,
            )?;
            scope_line(app, &scope)
        }
        ScopeCommand::History => {
            let versions = app.repos.projects.list_scope_versions(&id)?;
            if app.json {
                return app.print_json(&versions);
            }
            for s in versions {
                app.say(format!(
                    "v{}\t{} in / {} out / {} deferred / {} forbidden",
                    s.version,
                    s.in_scope.len(),
                    s.out_of_scope.len(),
                    s.deferred.len(),
                    s.forbidden.len()
                ));
            }
            Ok(())
        }
    }
}

fn scope_line(app: &App, scope: &vibe_domain::ScopeBoundary) -> vibe_core::error::Result<()> {
    if app.json {
        return app.print_json(scope);
    }
    app.say(format!(
        "scope v{}: {} in / {} out / {} deferred / {} forbidden",
        scope.version,
        scope.in_scope.len(),
        scope.out_of_scope.len(),
        scope.deferred.len(),
        scope.forbidden.len()
    ));
    Ok(())
}

fn baseline_cmd(
    app: &App,
    service: &ProjectService,
    project: Option<&str>,
    command: BaselineSet,
) -> vibe_core::error::Result<()> {
    let id = app.project(project)?;
    match command {
        BaselineSet::Set { file } => {
            let spec: BaselineSpec = read_json(&file)?;
            let baseline = service.set_baseline(&id, spec.into(), ActorType::User)?;
            if app.json {
                return app.print_json(&baseline);
            }
            app.say(format!(
                "baseline v{} · {} layers · {} invariants",
                baseline.version,
                baseline.layers.len(),
                baseline.invariants.len()
            ));
            Ok(())
        }
        BaselineSet::Show => match service.baseline(&id)? {
            Some(baseline) => app.print_json(&baseline),
            None => {
                app.say("no architecture baseline");
                Ok(())
            }
        },
        BaselineSet::History => {
            let versions = app.repos.projects.list_baseline_versions(&id)?;
            if app.json {
                return app.print_json(&versions);
            }
            for b in versions {
                app.say(format!(
                    "v{}\t{}",
                    b.version,
                    crate::views::one_line(&b.summary)
                ));
            }
            Ok(())
        }
    }
}
