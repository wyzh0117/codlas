# ADR-013 — One `entities` table with a JSON payload, not one table per object

Status: accepted (implementation deviation from V1_DEVELOPMENT §49, allowed by §50;
key corrected by migration v3)
Date: 2026-10-03
Ref: V1_DEVELOPMENT §49 (full table list), §50 (minimum database version)

## Context

§49 sketches `tasks`, `issues`, `decisions`, `artifacts`, `milestones`,
`change_proposals`, `architecture_evolution_plans`, `drift_alerts` as separate tables. §50
then permits collapsing them: "Task / Decision / Issue 可先作为 Entity subtype". The V1 field
sets for these objects are still moving — §19 alone requires a task to carry an
`unverifiedClaim` that no table column predicted.

## Decision

Follow §50. One table:

```sql
entities(kind, id, project_id, status, data_json, updated_at, PRIMARY KEY (kind, id))
```

plus `entity_links` for relationships and `entity_versions` for before/after transitions.
Typed access goes through `EntityStore`; the payload is the domain struct's own serde form,
so the struct definition is the schema.

The primary key is the pair, not `id` alone. The first cut of the baseline keyed `entities`
by `id` and upserted `ON CONFLICT(id)` without rewriting `kind`, so two entities of
different kinds sharing an id left **one row carrying one kind's name and the other kind's
body** — a confirmed decision whose subject id happened to equal an issue's id silently
turned that issue into decision JSON, and the reducer then failed to decode it (or, worse,
decoded it). Migration v3 rebuilds the table with a kind-scoped key; it is a forward
migration rather than an edit to baseline v1, because §49's own purpose is that a later
release can migrate user data instead of discarding it. Nothing referenced `entities(id)`
as a foreign key, so the rebuild needed no other table.

Charter, scope, architecture baseline, events, messages, evidence, snapshots and audit rows
keep their own tables, because those are versioned or append-only records with real
constraints (§9 `UNIQUE(project_id, version)`, §52 `UNIQUE(conversation_id, content_hash)`,
§15 `events.seq`).

## Consequences

- Adding a field to a Task is a code change, not a migration. In V1 that is the right
  trade; by V2 (entity/architecture graph, AEP-005) the JSON payload is what gets migrated.
- Querying is by `kind`, `project_id`, `status` (`idx_entities_project_kind`) and full text;
  anything more selective has to be promoted to a column. AEP-005 is the planned moment.
- `status` is duplicated out of the payload so the guard engine and the inbox can filter
  without decoding every row. The projector keeps them consistent (§25 keeps
  `DriftAlert.status` inside the payload too).
