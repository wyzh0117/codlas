# ADR-022 — Watch signals are events with a verb in the payload, and they move no state

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §16, §39, §19; PROJECT_MASTER §7.2, §7.3, 禁止 9

## Context

§39 asks for the working copy to be watched: which branch, what is uncommitted, which files
appeared, changed or vanished. §16's catalogue lists `FILE_CHANGED` and `GIT_COMMIT` and says
"at least", and two of the signals §39 names — branch and dirty status — had no type at all.

Three questions had to be answered before writing anything, and each has a documented answer
because the wrong one quietly breaks the project's core claim:

- Does "the tree got dirty" belong in `GIT_COMMIT`? No: `GIT_COMMIT` means *a commit exists*.
  Folding a branch switch or a dirty tree into it would put facts in the log that history
  does not contain.
- Does a file change belong in Project State? No: §19 makes completion a confirmed event, and
  禁止 9 forbids treating an observation as the project's memory.
- Does V1 need a watcher process? No: 禁止 8. A daemon would need a lifecycle, a config file
  and a failure mode that nothing in §13.2 accepts yet.

## Decision

1. **Two new catalogue types** for the git side, because they are different facts:
   `GIT_BRANCH_CHANGED` and `GIT_STATUS_CHANGED` (crates/domain/src/event_catalog.rs, listed in
   `ALL` so an unknown-type append still refuses them if they are ever removed).
2. **One type for the file side**, `FILE_CHANGED`, with the verb in the payload:
   `payload.change ∈ {created, modified, deleted, baseline}` (`ScanKind`,
   crates/ingestion/src/file_scan.rs). The signal §39 wants is "this path changed, and here is
   how", and §16's list is a floor rather than a ceiling. One type keeps the dedup key honest —
   the payload is part of the event's identity, so the same path re-reported with the same hash
   appends nothing while a genuinely new hash does.
   `baseline` exists because the *first* pass over a directory is not a claim that anything
   changed; it is a snapshot the next pass can diff against.
3. **Watching is a pass, not a process.** `GitFacts::watch_workspace` reads branch and status
   now, compares them against the snapshot the previous pass left in `settings`, and appends
   only what differs. `FileScan::diff` does the same against recorded hashes. No thread, no
   timer, no filesystem notification API — so the same code runs behind a `wp watch` call and
   behind any future UI button, and nothing needs to be stopped or upgraded.
4. **The reducer changes no state for any of these.** They join `TEST_*` / `BUILD_*` in the
   projector's "recorded, not folded" group (crates/state-engine/src/projector.rs), and each
   becomes a Git- or File-typed evidence row. A dirty tree says *where* the work is.
5. **A vanished file gets `ARTIFACT_DELETED`, not a silent row edit** — the artifact's history
   has to stay readable in the log, which is the only place §45 can point at.

## Consequences

- `wp watch [--dir] [--repo-id]` writes nothing when nothing changed:
  `watching_twice_without_touching_anything_writes_nothing` and
  `a_round_trip_over_the_same_paths_is_not_two_facts` pin idempotency, and
  `watch_signals_fold_into_no_project_state_at_all` pins §19 by asserting the state hash is
  identical before and after a pass that appended four events.
- A directory with no repository in it is reported as such rather than answered with an
  invented branch (`a_directory_with_no_repository_in_it_is_reported_rather_than_invented`).
  The git reads still go through the read-only subcommand allowlist (ADR round 1).
- §14's judgement flow is unchanged: a watcher event can be cited by a decision or a confirmed
  completion, but it cannot produce one — which is exactly why the state hash stays put.
- A real file-system watcher remains an AEP-grade change, not a bug fix: it would change when
  events are produced, not what they mean.
