# ADR-023 — A backup is an online copy, made and restored through SQLite's own backup API

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §55 Export/Backup, §62 Recovery; PROJECT_MASTER §7.5, §13.2 Stage H; ADR-019

## Context

Round 1 shipped §55's export side, so a project could already leave the machine as six
documents or one `.projectpack`. That is not a backup: a pack is *one project's projection*,
compiled at a moment, and it carries none of the workbench's settings, audit rows, indexes or
other projects. Stage H and §62 Recovery ask for the whole database to be copyable, verifiable
and restorable, and round 1's own KNOWN ISSUES recorded that gap ("导出已经能当备份用，但没有
restore 路径").

Two naive implementations are wrong in ways that only appear under real use:

- `std::fs::copy` of the main `.db` file freezes whatever the WAL has not yet checkpointed, so
  a backup taken seconds after a write can silently lack it;
- restoring by swapping files replaces a database this very process still holds open, which
  works on a laptop until it does not.

A backup you cannot verify is not a backup, and a restore that cannot be refused is a way to
lose work.

## Decision

1. **SQLite's Online Backup API in both directions** (crates/storage/src/backup.rs, rusqlite's
   `backup::Backup`, 128 pages per step). Copy and restore go through the same mechanism the
   engine uses to keep a WAL database consistent, so "the copy matches a committed state" is
   the engine's claim rather than this crate's hope.
2. **`PRAGMA integrity_check` is the verdict**, returned as SQLite's own string — `"ok"` or its
   first complaint — inside `BackupReport` together with schema version and row counts. The
   workbench never grades its own homework here.
3. **Maintenance is a separate port, not a twelfth repository.** `DatabaseMaintenance`
   (`backup_to / integrity_check / inspect / restore / schema_version`) lives beside
   `Repositories` in crates/core/src/ports.rs and `App` holds it as its own handle, because no
   project logic reads the database *as a file* (§7.5). Paths are the only storage concept
   allowed to cross it.
4. **`restorable(known_version)` gates every read path**: a file whose schema is newer than
   this build is refused before anything is copied, so a downgrade cannot half-succeed.
5. **Restore is destructive, and says so four times** (apps/cli/src/backup.rs):
   - `create` refuses to copy a live database that is not whole;
   - `restore` stops without `--yes`, having explained what will be discarded;
   - restoring the live file onto itself is refused, because it would prove nothing;
   - the current database is snapshotted first — and if *that* fails, the failure is only
     warned, because Recovery must not be blocked by the safety net.
6. **Secrets are out of scope on purpose.** Values never entered SQLite (ADR-017), so a backup
   carries `secret_ref` strings and a restore moves no credential. The report says so.

## Consequences

- `wp backup create|status|verify|restore` covers §55's backup side and §62's Recovery verb;
  `a_backup_returns_the_workbench_to_an_earlier_moment` (CLI) and six storage tests cover it,
  including `restoring_discards_what_was_recorded_after_the_backup`,
  `an_unrelated_sqlite_file_is_refused_as_a_backup` and
  `a_missing_backup_is_not_found_rather_than_corrupt`.
- `a_backup_reads_data_the_live_database_wrote_after_it_was_taken_nothing` pins the property
  that matters most: the copy is a point-in-time snapshot, not a live view.
- A pack and a backup remain different artifacts for different jobs; neither is a substitute,
  and the CLI's prose says which one it wrote.
- Backup files are plain SQLite databases, so `wp backup verify <file>` works on a file from
  another machine — while `sqlite3 file .dump` also works, which is the escape hatch §62's
  "report the degradation" clause wants to exist.
