# ADR-019 — `.projectpack` is a stored zip whose manifest says "projection, not a second source of truth"

Status: accepted
Date: 2026-10-03
Ref: V1_DEVELOPMENT §55; PROJECT_MASTER §19 禁止 4; ADR-002, ADR-011

## Context

§55 requires six exports (timeline, project state, charter, decisions, AEPs, project map) and
allows V1 to package them as `.projectpack` built from zip + json + md. The risk the master
names repeatedly (禁止 4: 禁止维护两套 Project State) is a second materialized copy that
drifts from the log. The packaging has to make its subordination legible to whoever unzips
it months later.

## Decision

`wp export pack [FILE]` (default `project.projectpack`, apps/cli/src/export.rs) writes a zip
containing:

| member | content |
| --- | --- |
| `manifest.json` | format / version / projectId / projectName / stateHash / members + the note |
| `project-state.json` | the reducer's read-only projection |
| `project-map.md` | the compiled map, byte-identical to `wp export map` |
| `charter.json` | charter + scope + architecture baseline |
| `decisions.json`, `aep.json` | state sections |
| `timeline.json` | the accepted event log in `seq` order |

- Everything is read through `project_read_only` and the same compiler the workbench uses:
  exporting never advances a baseline, never appends an event, never saves a snapshot.
- Compression is `Stored`, not `Deflated`: every member is JSON or markdown, and a reader
  without a zip codec should still be able to open the file. Size is not the point;
  legibility is.
- `manifest.stateHash` is the map's `source_state_hash`, so a pack states which state it
  projects; the manifest carries the literal sentence "projection of the accepted log, not a
  second source of truth". `apps/cli/tests/cli_e2e.rs` asserts that sentence, and that the
  packed map equals the standalone `wp export map` byte for byte.

## Consequences

- `timeline.json` is a mandatory member, not an option: a pack without the log could not be
  replayed, and an unreplayable summary is exactly the second-source failure 禁止 4 warns
  about.
- No import side exists in V1. An import would be a write path outside the event store
  (禁止 7/禁止 4 again); a pack is for moving knowledge to a human or another agent, not
  back in. Re-entering agent results is `wp import` / `wp event`, which go through the store.
- Secret values are never members; only refs appear, and §53's policy filter runs before any
  adjacent context compile, so a pack is safe to hand to a cloud target for the same reason a
  context pack is.
