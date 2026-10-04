# ADR-021 — An adapter's health has exactly four outcomes, and the last run replaces the one before

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §51, §58, §62 Degradation; PROJECT_MASTER §7.9, §13.2 Stage H

## Context

Stage H asks for "Adapter Health", §62 asks that degradation be *reported* rather than
hidden, and §51 is the case that makes both necessary: a partial ingestion where material
landed but the cursor did not move. The schema already had an `adapter_states` table
(frozen in §49), but a table with no agreed vocabulary produces a status field that means
whatever the last writer needed it to mean — and "error" written by five adapters is five
different claims about whether an adapter works.

Silence is the failure mode here. An adapter that ingested nothing and said nothing is
indistinguishable from one that is not installed, so the user cannot tell whether to fix
something or to expect nothing.

## Decision

1. **Four outcomes, each one something a pipeline run can actually observe**
   (`AdapterHealth`, crates/domain/src/agent.rs):
   - `ok` — detected, ingested, cursor advanced.
   - `unavailable` — `detect()` found no such source on this machine. Nothing was attempted,
     so nothing failed.
   - `degraded` — material landed but part of the run failed, in practice the cursor write.
     The next run will re-send what is already stored and L1/L2 dedup will absorb it (§51).
   - `failed` — the run produced nothing and the cursor stayed where it was.
   `Unavailable ≠ Failed` is the load-bearing distinction: it is what keeps §62's honesty from
   turning "this source is not here" into a bug report against an adapter that is fine.
2. **One row per adapter, keyed by adapter id alone** (`AdapterState { adapterId, health,
   detail, updatedAt }`, crates/core/src/ports.rs `AdapterRepository`). Not per project: a
   source that is absent from this machine is absent for every project on it. `record_state`
   replaces the row, so `wp adapters` shows the current verdict and never a stack of history.
3. **History stays where it already lives.** The verdict is a cache of "what the last run
   learned", not a log; the event log and the §54 audit table remain the record of what
   happened. Inventing a second append-only history in `adapter_states` would be 禁止 4.
4. **`detail` is shown as-is** — the error text or an empty-batch note, never a stack trace.

## Consequences

- Every pipeline run ends with an answer, including the runs that do nothing
  (`crates/ingestion/tests/health.rs`: `a_clean_run_leaves_an_ok_verdict`,
  `a_source_that_is_not_here_is_unavailable_not_failed`,
  `material_that_landed_with_a_broken_cursor_is_degraded`,
  `a_second_run_replaces_the_verdict_rather_than_stacking_history`).
- `wp adapters [--json]` is the surface for Stage H's acceptance item, and it is a read of the
  port rather than a re-derivation.
- Adding a fifth outcome means adding a state the pipeline cannot observe; the enum is the
  place to argue about that, not the CLI's formatter.
- The table has no project column, so a future per-source, per-project health view is a
  schema change plus an AEP, not a silent widening of `adapter_id`.
