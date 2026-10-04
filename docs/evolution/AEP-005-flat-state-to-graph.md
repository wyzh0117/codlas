# AEP-005 — Flat Project State → Entity / Architecture Graph

Status: dormant
Owner: architecture
Ref: V1_DEVELOPMENT §28, §40 L3, §49, §50; PROJECT_MASTER §15.1, §18; ADR-013

## Current

`ProjectState` is a flat projection: lists of tasks, issues, decisions, artifacts,
milestones, change proposals, AEPs, drift alerts, evidence. Relationships live in
`entity_links (from_kind, from_id, to_kind, to_id, relation)` with transitions recorded in
`entity_versions`, and L3 search walks those links in both directions. Payloads are JSON in
`entities.data_json`, so the domain struct is the schema (ADR-013).

## Limitation

Two things a flat list answers badly: "which modules does this decision touch, and what else
depends on them" (blast radius), and "what changed in the architecture between these two
snapshots" (architecture diff). Both are multi-hop questions over the same rows, and
repeated multi-hop reads over JSON is where the flat choice starts costing real work.
The V1 roadmap also wants a module relationship map (§15.1), which is a graph rendering, not
a list.

## Why Not Now

§50 sanctions the flat form for V1 and §56 defers the graph. V1's acceptance criteria (§14)
need decisions, tasks and evidence to be *linkable*, which they already are — a graph store
would add a query language and a migration with no criterion asking for either (禁止 8).

## Trigger

- A V2 view (Architecture Map, Architecture Diff, Decision → Code Impact) is actually being
  built, or
- a query needs more than two hops often enough that `entity_links` round-trips show up in
  measured latency, or
- `data_json` starts demanding promoted columns for a real filter — that pressure is the graph
  asking to exist.

## Target

`entity_links` becomes the spine: typed node kinds, typed edge kinds, a recursive read layer,
plus a stored module boundary view derived from `ArchitectureBaseline.layers`. SQLite
`WITH RECURSIVE` is likely enough — this is a modelling step, not necessarily new storage.

## Change surface

- Edge types become an enum with meaning (`depends_on`, `implements`, `contradicts`,
  `supersedes`, `evidences`) instead of the free-text `relation` strings used today.
- Charter / scope / baseline rows gain graph nodes so drift can point at an edge, not only at
  an entity.
- `entity_versions` becomes the substrate for architecture diff, so its coverage has to be
  complete before a diff is trustworthy.
- The context compiler's Relevant Artifacts and Related AEP sections become graph queries.
- A `state_hash` that covers sets must cover edges too, or snapshots stop being comparable.

## Preserved core

Events remain the source of truth (ADR-002); the graph is another projection, computed by the
same fold. One materialized state (禁止 4). The map stays a compiled view (ADR-011) — a
rendered graph is an additional projection, never a replacement for the thirteen sections.

## Benefit / Cost

Benefit: blast-radius and architecture-diff questions become single reads. Cost: a schema for
edges to evolve, and migration of every existing project's link rows.
