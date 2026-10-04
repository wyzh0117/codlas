# ADR-016 — What V1's first implementation round deliberately does not include

Status: accepted, amended 2026-10-05 (the MCP item below moved from deferred to built)
Date: 2026-10-03
Ref: PROJECT_MASTER §20, §24, §19 禁止 8; V1_DEVELOPMENT §61 stage list

## Context

The first implementation round had to produce a runnable closed loop (Stage A/B/C/G plus the
Map and Context engines) rather than a wide but unconnected surface. Several §13.2 items are
front-end or transport concerns that a core loop does not need, and pretending otherwise is
the 禁止 8 failure mode.

## Decision

Built and tested in this round: Cargo workspace, domain model, ten repository ports, SQLite
schema + migrations + FTS5, event store, charter/scope/baseline with user lock and change
proposals, deterministic reducer, PROJECT_MAP compiler, snapshot + diff, six drift rules,
inbox, state judge, entity resolution, context compiler with budget/policy/audit, secret
scanning and storage, adapters, git/file evidence capture wired to `wp import git|files`
reading the project's bound `rootPath`, search layers L1/L2/L3 behind one pipeline with
`wp reindex`, the secret/policy/export CLI (including `.projectpack`), and two test layers —
`golden.rs` over the libraries, `cli_e2e.rs` over the real binary.

Deferred on purpose, each with the stage that owns it:

- **Tauri desktop UI (V1-E workbench screens)** — `apps/desktop` does not exist yet. The CLI
  renders the same compiled map, the same diff and the same context pack, so the UI round is
  a presentation layer over proven logic rather than a redesign of it. `cargo-tauri` 2.11.5
  and the Rust toolchain are verified working on this machine.
- **Browser extension + Native Messaging (V1-D)** — §34/§35. Needs the adapter contract to
  settle first, which it now has; the MV3 side is the only part that genuinely requires a
  browser session.
- **MCP server (V1-D/F)** — *no longer deferred.* Built in the second round as
  `crates/mcp-server` + the `wp-mcp` binary; what shipped, and what still has not, is
  ADR-026's decision.
- **Cloud, sync, multi-device** — AEP-001, untriggered. `ProjectEvent.originNodeId` and
  `SyncCursor.lastContentHash` are reserved so the shape is ready.
- **Semantic/vector retrieval** — AEP-002, untriggered. L1/L2/L3 cover the V1 acceptance
  criteria.
- **`ChatGptWebAdapter` beyond transcript import** — reading a live web session is the
  extension's job, not the core's.
- **Auto-execute of an agent** — §37 explicitly does not require it in V1.

## Consequences

§14's ten-point V1 acceptance list is exercised at the core-and-CLI level for items 1–6 and
8–10 (charter → sources → entities → map → drift → pack → prepare → re-entry into the
timeline); item 2's folder binding is the project's own `rootPath` read by
`wp import git|files`, and item 7's gesture ("点击某个 Issue") is the CLI's
`wp context build --select kind:id` rather than a click. What remains open is the surface
itself: **the V1-E desktop screens and the V1-D browser extension**. The MCP transport closed
in the second round, and §55's backup side, Stage H's Adapter Health and Performance closed
with it — see ADR-021 through ADR-027. PROJECT_MASTER §20 states the current position.

The two remaining items are still deferred for the reason this record was written: each needs
a lifecycle (a window, a browser session) that no core logic depends on, and 禁止 8 says a
presentation layer over proven logic is built after the logic, not beside it.
