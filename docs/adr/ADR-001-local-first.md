# ADR-001 — Local-first storage

Status: accepted (design), implemented 2026-10-03
Ref: PROJECT_MASTER §7.1, V1_DEVELOPMENT §4.1

## Context

The product exists so a single developer keeps a project's design history. That history
is the most valuable asset the tool holds, and it must survive losing network, losing an
agent account, or closing the laptop.

## Decision

Every record — project, charter versions, scope versions, architecture baselines,
sources, conversations, messages, events, entities, evidence, snapshots, context packs,
audit rows — lives in a local SQLite file. Nothing is uploaded, mirrored or backed up to a
service as part of V1. Cloud appears only as `AEP-001`, whose trigger conditions are
unchanged.

Consequences of the choice that show up in code:

- `SqliteStore::open(path)` is the only place a path is resolved; the default data dir is
  chosen by the caller (`apps/cli` today), never by a crate.
- `PRAGMA journal_mode = WAL` so the workbench can read while an adapter is appending.
- The `.projectpack` export (§55) is the only supported way to move a project today.

## Consequences

Works offline, no account, no cost. Single-device only until AEP-001 triggers; a lost disk
without an export loses the project, which is why export is in V1 scope rather than later.
