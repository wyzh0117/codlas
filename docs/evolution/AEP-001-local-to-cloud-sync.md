# AEP-001 — Local-only → Personal Local + Cloud Sync

Status: dormant
Field shape per V1_DEVELOPMENT §24 / PROJECT_MASTER §8.5.

## Current

One SQLite file per machine behind `vibe_storage::SqliteStore`, which is the only crate that
knows rusqlite exists. Every fact is local: events, entities, evidence, snapshots, audit.
Export is a manual `wp export` producing a `.projectpack` zip (V1_DEVELOPMENT §55).

## Limitation

A user with a laptop and a desktop has two divergent histories and no supported way to merge
them. Losing the disk means losing the project unless an export was taken by habit.

## Why Not Now

Sync requires an identity for the writing node and a conflict policy, and both are only
meaningful once there is a second node. PROJECT_MASTER 禁止 8 forbids building it on
speculation, and 禁止 7 forbids letting it leak into core.

## Trigger

Start when any one holds:
- multi-device use becomes a weekly pattern, not an experiment;
- a local backup has actually proven insufficient (a real loss, or an export that was missing
  when it was needed);
- remote read access from a phone or browser becomes a recurring request.

## Target

```
Project Core → Repository Port → Local Repository → SQLite
                                        ↕
                                   Sync Engine
                                        ↕
                                  Personal Cloud
```

Personal only: backup, multi-device, restore. No team workspace, ever (PROJECT_MASTER §16).

## Change surface

- `ProjectEvent.originNodeId` — reserved and always `None` today, exactly so this plan stays
  cheap (V1_DEVELOPMENT §15).
- `events.seq` is currently a store-local AUTOINCREMENT (ADR-014). A merged log needs a
  clock that survives two writers; this is the hard part of the plan.
- `SyncCursor` already carries `lastContentHash` and is per `(adapterId, sourceId)`; it gains
  a node dimension.
- New: a conflict view. Last-write-wins is not acceptable here because PROJECT_MASTER §7.4
  makes human override the highest authority, so a sync conflict must surface as an inbox
  item, not resolve silently.
- `SqliteStore` remains one implementation behind the same ten ports (ADR-004). Core does not
  change.

## Preserved core

Event-first append-only log, the deterministic reducer, the single materialized state, the
repository boundary, charter-is-user-controlled, compiled map.

## Benefit / Cost

Benefit: the project survives hardware and follows the user. Cost: vector/merge semantics,
a hosted component, and a conflict UX — enough that it is a plan, not a feature.
