//! §55 / §62 Recovery at the store boundary: a backup must be a whole, readable,
//! restorable copy — not a file that only looks like one because SQLite wrote it.
//!
//! These tests restore into a live connection rather than swapping files, which is the
//! only shape that matters: the workbench is open while it is being backed up, and a
//! restore has to land in the database some body already holds.

use vibe_core::ports::{DatabaseMaintenance, EntityStore, Repositories};
use vibe_domain::EntityKind;
use vibe_storage::{SqliteStore, SCHEMA_VERSION};

fn store_at(dir: &std::path::Path, name: &str) -> std::sync::Arc<SqliteStore> {
    SqliteStore::open(dir.join(name)).expect("open")
}

fn project(repos: &Repositories, name: &str) -> String {
    vibe_core::project_service::ProjectService::new(repos.clone())
        .create_project(name, None, None, None)
        .expect("project")
        .id
}

#[test]
fn a_backup_is_a_whole_copy_that_another_database_can_read_back() {
    let dir = tempfile::tempdir().expect("temp dir");
    let live = store_at(dir.path(), "workbench.db");
    let repos = live.repositories();
    project(&repos, "continuity");

    let dest = dir.path().join("backups").join("first.db");
    let report = live.backup_to(&dest).expect("backup");

    assert_eq!(report.projects, 1, "the copy carries the project");
    assert_eq!(report.integrity, "ok", "and is whole");
    assert_eq!(report.schema_version, SCHEMA_VERSION);
    assert!(report.pages > 0);
    assert!(
        dest.exists(),
        "a default destination inside a missing directory still writes"
    );

    // A different database file, restored from that copy.
    let other = store_at(dir.path(), "other.db");
    let restored = other.restore(&dest).expect("restore");
    assert_eq!(restored.projects, 1);
    assert_eq!(restored.integrity, "ok");
    assert_eq!(
        other
            .repositories()
            .projects
            .list_projects()
            .expect("list")
            .len(),
        1
    );
}

#[test]
fn restoring_discards_what_was_recorded_after_the_backup() {
    let live = SqliteStore::open_in_memory().expect("open");
    let repos = live.repositories();
    let kept = project(&repos, "kept");

    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("kept.db");
    live.backup_to(&file).expect("backup");

    project(&repos, "written after the backup");
    assert_eq!(repos.projects.list_projects().expect("list").len(), 2);

    let report = live.restore(&file).expect("restore");
    assert_eq!(report.projects, 1, "the copy's content replaced the live one");

    let names = repos.projects.list_projects().expect("list");
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].id, kept);
}

#[test]
fn a_backup_reads_data_the_live_database_wrote_after_it_was_taken_nothing() {
    // Proves the copy is a real snapshot: rows written afterwards must not appear in it,
    // even though both databases share a process and a WAL mode.
    let dir = tempfile::tempdir().expect("temp dir");
    let live = store_at(dir.path(), "live.db");
    let repos = live.repositories();
    project(&repos, "before");

    let file = dir.path().join("snapshot.db");
    let report = live.backup_to(&file).expect("backup");
    project(&repos, "after");

    let cold = store_at(dir.path(), "cold.db");
    let reopened = cold.inspect(&file).expect("inspect");
    assert_eq!(reopened, report);
    assert_eq!(reopened.projects, 1, "the snapshot is frozen at its own moment");

    cold.restore(&file).expect("restore");
    let ids = cold.repositories().projects.list_projects().expect("list");
    assert_eq!(ids.len(), 1);
}

#[test]
fn an_unrelated_sqlite_file_is_refused_as_a_backup() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("notes.db");
    let conn = rusqlite::Connection::open(&file).expect("foreign database");
    conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, text TEXT);")
        .expect("table");
    drop(conn);

    let live = SqliteStore::open_in_memory().expect("open");
    let message = live
        .inspect(&file)
        .expect_err("a file with no workbench tables is not a backup")
        .to_string();
    assert!(
        message.contains("not a workbench database")
            && message.contains("schema_migrations")
            && message.contains("notes.db"),
        "{message}"
    );
    assert!(live
        .restore(&file)
        .expect_err("and it is not restorable either")
        .to_string()
        .contains("not a workbench database"));
}

#[test]
fn a_missing_backup_is_not_found_rather_than_corrupt() {
    let dir = tempfile::tempdir().expect("temp dir");
    let live = SqliteStore::open_in_memory().expect("open");
    let error = live
        .inspect(&dir.path().join("never-written.db"))
        .expect_err("nothing there");
    assert!(error.to_string().contains("not found"), "{error}");
}

#[test]
fn a_live_database_reports_itself_whole_and_a_backup_carries_every_table() {
    let dir = tempfile::tempdir().expect("temp dir");
    let live = SqliteStore::open_in_memory().expect("open");
    let repos = live.repositories();
    let id = project(&repos, "integrity");
    repos
        .entities
        .put_entity(
            EntityKind::Task,
            &vibe_domain::Task {
                id: "task-recovery".into(),
                project_id: id.clone(),
                title: "restore the workbench".into(),
                ..Default::default()
            },
            Some("open"),
        )
        .expect("task");

    assert_eq!(live.integrity_check().expect("integrity"), "ok");
    assert_eq!(live.schema_version().expect("version"), SCHEMA_VERSION);

    let report = live.backup_to(&dir.path().join("report.db")).expect("backup");
    assert_eq!(report.projects, 1);
    assert_eq!(
        report.events,
        repos
            .events
            .list_events(&id, None, None)
            .expect("events")
            .len() as i64,
        "creating a project writes an event, and the copy has to carry it"
    );

    // Recovery is proved by reading the tables back from a restored database, not by
    // the report's own counts.
    let cold = store_at(dir.path(), "cold.db");
    cold.restore(&dir.path().join("report.db")).expect("restore");
    let cold_repos = cold.repositories();
    let tasks: Vec<vibe_domain::Task> = cold_repos
        .entities
        .entities_of(EntityKind::Task, &id)
        .expect("tasks come back too");
    assert_eq!(tasks.len(), 1, "a backup is the whole workbench, not one table");
    assert_eq!(tasks[0].title, "restore the workbench");
}
