# ADR-002 — Event-first, with a disposable projection

Status: accepted (design), implemented 2026-10-03
Ref: PROJECT_MASTER §7.2, 禁止 4 / 禁止 6, V1_DEVELOPMENT §15–§18

## Context

A model summary is allowed to *observe* a project; it is not allowed to *be* the project's
memory. If summaries could overwrite state, one bad rewrite would silently delete history,
and nobody could answer "why is the project like this?" afterwards.

## Decision

`Source → Event → Reducer → Project State` is the only write path.

- Events are append-only, carry `actorType`, `confidence`, `status` and `evidenceRefs`, and
  their type must come from `vibe_domain::event_catalog` (40 §16 constants). Appending an
  unknown type is a validation error, not a silent accept.
- Only `Accepted` events reduce. A candidate below the inbox floor is stored for audit but
  changes nothing.
- `projector::fold` is a pure function: it never reads the entity, artifact or evidence
  rows it is about to write. Materialized rows are a cache, and dropping every one of them
  then re-folding must reproduce the same `state_hash`.
- There is exactly one `ProjectState` type; no crate keeps a second copy of project facts.

## Consequences

Reading is a join-free cache hit, and time-travel, replay and golden tests are all the same
operation. The cost: an event that turns out to be wrong is never edited — it is superseded
by another event, so the log grows monotonically and rules must be written to tolerate that.
