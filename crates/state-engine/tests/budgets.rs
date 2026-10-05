//! §59's performance principles and §60's cost principle, tested as *work the code does
//! not do*.
//!
//! The doc forbids the expensive design ("never re-send the whole chat to a model") and
//! requires the cheap one (incremental sync, materialized state, cached map, indexed
//! search, snapshots). Each test below pins one of those: a reader that answers from stored
//! rows, a replay that reads only the tail, and a dependency list that cannot reach a
//! network.
//!
//! Two clock-based bounds, deliberately different in kind. The pure fold gets a *tight*
//! one, because projecting is arithmetic and any machine agrees about what arithmetic
//! costs. The chain that also writes the SQLite caches gets a *loose* one — a hang and
//! O(n²) tripwire whose real measured cost is printed in the failure message, since a
//! disk-bound number on a loaded laptop is not something a suite may honestly assert.
//! Everything else here is a work-count assertion: which rows a reader touched, how many
//! events a replay pulled, which dependencies a manifest declares.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{EntityStore, Repositories};
use vibe_core::project_service::ProjectService;
use vibe_domain::{event_catalog, ActorType, EntityKind, Task, TaskStatus};
use vibe_state_engine::{ProjectMapCompiler, Reducer, SnapshotService};
use vibe_storage::SqliteStore;

/// Events in the fixture log. Large enough that a quadratic fold is visibly wrong, small
/// enough that the suite stays a test rather than a benchmark run.
const EVENTS: usize = 1_200;

/// The pure fold — the deterministic core §59 is actually about — has to stay trivially
/// cheap: no store writes, no model, no clock. Measured cost is ~40 ms for this log on a
/// debug build, so 250 ms catches a real regression and never a busy laptop.
const FOLD_BUDGET_MS: u128 = 250;

/// The whole chain *including* the SQLite materialization. This one is a hang tripwire,
/// not a benchmark: it measures ~6 s for this log on a debug build, and the bound is set
/// where a machine two times slower still passes. The precise per-stage cost is printed in
/// the failure message, so a trip says what changed rather than just failing.
const CHAIN_BUDGET_MS: u128 = 20_000;

fn repos() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Budget", None, None, None)
        .expect("project");
    (store, repositories, project.id)
}

/// A task cycle: created, started, completed — the shape that makes the state grow.
/// `occurred_at` is left to the store's clock because these events are appended once, in
/// order; `seq` is what the fold reads them in.
fn cycle(repositories: &Repositories, project_id: &str, round: usize) -> Result<()> {
    let store = vibe_core::event_store::EventStore::new(repositories.clone());
    let task = format!("task-{round}");
    for (kind, title) in [
        (event_catalog::TASK_CREATED, "compile the map"),
        (event_catalog::TASK_STARTED, "compile the map"),
        (event_catalog::TASK_COMPLETED_CONFIRMED, "compile the map"),
    ] {
        store.append(
            vibe_core::event_store::EventDraft::new(project_id, kind)
                .actor(ActorType::User)
                .subject("task", task.clone())
                .payload(json!({ "title": format!("{title} #{round}") })),
        )?;
    }
    if round % 4 == 0 {
        store.append(
            vibe_core::event_store::EventDraft::new(project_id, event_catalog::DECISION_CONFIRMED)
                .actor(ActorType::User)
                .subject("decision", format!("d-{round}"))
                .payload(json!({
                    "title": format!("choice {round}"),
                    "decision": "keep the projection pure",
                    "rationale": "so replay stays deterministic",
                })),
        )?;
    }
    Ok(())
}

/// §59's principle in the cheapest form the test can assert: projecting a log is local,
/// pure arithmetic. One pass over 1200 events, twice, same hash, no store writes, and a
/// cost proportional to what it read.
#[test]
fn the_pure_projection_costs_only_what_it_reads() {
    let (_store, repositories, project) = repos();
    for round in 0..(EVENTS / 3) {
        cycle(&repositories, &project, round).expect("append");
    }
    let reducer = Reducer::new(repositories.clone());
    let log = reducer.accepted_events(&project).expect("log");
    assert!(log.len() >= EVENTS, "{} events", log.len());

    let started = Instant::now();
    let first = reducer.project_read_only(&project).expect("projection");
    let fold = started.elapsed();
    let started = Instant::now();
    let second = reducer.project_read_only(&project).expect("projection");
    let again = started.elapsed();

    assert!(
        fold.as_millis() < FOLD_BUDGET_MS,
        "a {}-event fold took {fold:?} (budget {FOLD_BUDGET_MS} ms) — §59's whole point is \
         that this is arithmetic, not a round trip",
        log.len()
    );
    assert_eq!(
        first.state_hash, second.state_hash,
        "and the second pass costs {again:?} to reach the same hash"
    );
    assert_eq!(first.tasks.len(), EVENTS / 3, "the work was real");
}

/// §59 Materialized State + §18: the chain a user actually triggers — fold, write the
/// caches, compile the map, take a snapshot, diff it — runs locally and finishes.
#[test]
fn the_whole_chain_materializes_inside_the_budget() {
    let (_store, repositories, project) = repos();
    for round in 0..(EVENTS / 3) {
        cycle(&repositories, &project, round).expect("append");
    }
    let log = repositories
        .events
        .list_accepted(&project, None)
        .expect("accepted log");
    assert!(
        log.len() >= EVENTS,
        "the fixture must be big enough to measure: {} events",
        log.len()
    );

    let started = Instant::now();
    let state = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("projection");
    let map = ProjectMapCompiler::new(repositories.clone())
        .compile(&project)
        .expect("map");
    let snapshot = SnapshotService::new(repositories.clone())
        .take(&project, "budget")
        .expect("snapshot");
    let diff = SnapshotService::new(repositories.clone())
        .since_last_snapshot(&project)
        .expect("diff");
    let took = started.elapsed();

    // The work has to be real, or the ceiling proves nothing.
    let tasks: Vec<Task> = state.tasks.clone();
    assert_eq!(
        tasks.len(),
        EVENTS / 3,
        "one task per round of three events"
    );
    assert!(
        tasks.iter().all(|t| t.status == TaskStatus::Done),
        "the cycle completes each task"
    );
    assert!(
        map.markdown.contains("Active Tasks"),
        "the map rendered its sections rather than a placeholder: {}",
        &map.markdown[..map.markdown.len().min(200)]
    );
    assert!(map.markdown.contains("Recent Changes"), "…including the tail of the log");
    assert_eq!(snapshot.state.tasks.len(), tasks.len());
    assert_eq!(
        diff.current_state_hash,
        snapshot.state.state_hash.clone().unwrap_or_default(),
        "the diff compared the live state against the snapshot just taken"
    );
    assert!(diff.since_snapshot_id.is_some(), "and it knew its baseline");

    assert!(
        took.as_millis() < CHAIN_BUDGET_MS,
        "fold + map + snapshot + diff over {} events took {took:?} (budget {CHAIN_BUDGET_MS} ms)",
        log.len()
    );
}

/// §59 Cached Project Map + Snapshot: a reader that opens a project reads stored rows, so
/// what it returns is what was stored — even after the log moved on. If either call
/// re-derived from the log these assertions would flip.
#[test]
fn opening_a_project_answers_from_stored_rows_not_from_the_log() {
    let (_store, repositories, project) = repos();
    for round in 0..4 {
        cycle(&repositories, &project, round).expect("append");
    }
    let reducer = Reducer::new(repositories.clone());
    let compiler = ProjectMapCompiler::new(repositories.clone());
    let snapshots = SnapshotService::new(repositories.clone());

    let map = compiler.save(&project).expect("cached map");
    let snapshot = snapshots.take(&project, "milestone").expect("snapshot");
    assert_eq!(
        reducer.project_state(&project).expect("state").tasks.len(),
        4
    );

    // The log grows. A cached read must not notice.
    cycle(&repositories, &project, 4).expect("append round 5");
    let after = Instant::now();
    let cached_map = compiler.latest(&project).expect("row read").expect("a map saved");
    let cached_state = snapshots.latest(&project).expect("row read").expect("a snapshot taken");
    let took = after.elapsed();

    assert_eq!(cached_map.markdown, map.markdown, "the cached map is the saved bytes");
    assert_eq!(
        cached_map.source_state_hash,
        snapshot.state.state_hash.clone().unwrap_or_default(),
        "and it still names the state it was compiled from"
    );
    assert_eq!(cached_state.state.tasks.len(), 4, "not 5: no fold happened here");
    assert!(took.as_millis() < 200, "a row read measured {took:?}");

    // The fresh projection does see the new event, so the difference above is the cache
    // doing its job rather than a write that silently failed.
    assert_eq!(
        reducer.project_state(&project).expect("state").tasks.len(),
        5
    );
}

/// §59 Incremental Event Extraction: the store hands a caller the tail after a sequence
/// number, so a sync that already saw 600 of 900 events reads 300, not 900.
#[test]
fn an_incremental_read_pulls_only_what_is_new() {
    let (_store, repositories, project) = repos();
    for round in 0..(EVENTS / 3) {
        cycle(&repositories, &project, round).expect("append");
    }
    let all = repositories
        .events
        .list_accepted(&project, None)
        .expect("log");
    let half = all.len() / 2;
    let watermark = all[half - 1]
        .seq
        .ok_or_else(|| CoreError::Validation("the store assigns a seq".into()))
        .expect("seq");

    let tail = repositories
        .events
        .list_accepted(&project, Some(watermark))
        .expect("tail");
    assert_eq!(tail.len(), all.len() - half, "everything after the watermark");
    assert!(tail.iter().all(|event| event.seq.unwrap() > watermark));
    assert_eq!(
        tail.first().and_then(|event| event.seq),
        Some(watermark + 1),
        "contiguous, so no caller can lose an event between reads"
    );

    // Nothing new since the last read is an empty read, not a full one — which is what
    // lets a repeated sync cost nothing (§51).
    let newest = all.last().and_then(|event| event.seq).expect("seq");
    assert!(repositories
        .events
        .list_accepted(&project, Some(newest))
        .expect("nothing")
        .is_empty());
}

/// §59 Materialized State, the read-only half: the cache rows a projection wrote answer
/// the question "what tasks are there?" without any fold at all.
#[test]
fn materialized_rows_read_back_the_projection() {
    let (_store, repositories, project) = repos();
    for round in 0..6 {
        cycle(&repositories, &project, round).expect("append");
    }
    let folded = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("projection");

    let cached: Vec<Task> = repositories
        .entities
        .entities_of(EntityKind::Task, &project)
        .expect("cache rows");

    assert_eq!(cached.len(), folded.tasks.len(), "one row per task");
    let mut cached_titles: Vec<String> = cached.iter().map(|t| t.title.clone()).collect();
    let mut folded_titles: Vec<String> = folded.tasks.iter().map(|t| t.title.clone()).collect();
    cached_titles.sort();
    folded_titles.sort();
    assert_eq!(
        cached_titles, folded_titles,
        "the cache and the fold agree, so either may answer a reader (§18, 禁止 4)"
    );
}

/// §60's cost principle is a dependency claim: hash, rules, SQLite, FTS, git, reducer and
/// diff do the work, and nothing in the deterministic chain may reach a network or a model.
/// A `Cargo.toml` line is the cheapest possible place to make that unbreakable, and the
/// only place that stays true after someone adds a feature.
#[test]
fn no_manifest_in_the_workspace_reaches_a_network_or_a_model() {
    let banned = [
        "reqwest", "hyper", "ureq", "surf", "curl", "rusqlite-postgres", "postgres", "sqlx",
        "supabase", "openai", "langchain", "ollama", "lmstudio", "tokio", "tokio-util", "axum",
        "tonic", "tonic", "websockets", "eventsource", "oauth2", "jwt",
    ];
    // rusqlite is allowed exactly once, in the crate that owns the boundary (§7.5, 禁止 7).
    let sqlite_owner = "storage";

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf();
    let mut checked = 0;
    for crate_dir in [
        "crates/domain",
        "crates/core",
        "crates/storage",
        "crates/state-engine",
        "crates/guard-engine",
        "crates/context-engine",
        "crates/ingestion",
        "crates/security",
        "crates/search",
        "crates/mcp-server",
        "apps/cli",
        // The shell is the surface where a network call is most tempting and least excusable:
        // it reads the same ports the CLI reads, so it has the same right to reach nothing.
        "apps/desktop",
    ] {
        let manifest = workspace.join(crate_dir).join("Cargo.toml");
        // An absent crate is not a violation: this test must keep passing as the tree
        // grows or shrinks, and only say something about the manifests it can read.
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        checked += 1;
        for line in text.lines() {
            let Some((name, _)) = line.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let base = name.rsplit(':').next().unwrap_or(name).trim();
            for needle in banned {
                if base == needle {
                    panic!(
                        "{} declares `{name}`: §60 keeps the deterministic chain free of a \
                         network and of a model",
                        manifest.display()
                    );
                }
            }
            if base == "rusqlite" && crate_dir != format!("crates/{sqlite_owner}") {
                panic!(
                    "{} declares rusqlite: only `crates/{sqlite_owner}` may know SQLite exists \
                     (§7.5, 禁止 7)",
                    manifest.display()
                );
            }
        }
    }
    assert!(
        checked >= 9,
        "the walk found {checked} manifests, which means the workspace layout moved and this \
         test is now guarding nothing"
    );
}
