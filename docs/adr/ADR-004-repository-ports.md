# ADR-004 — Repository ports, and what sits on each side of them

Status: accepted (design), implemented 2026-10-03
Ref: PROJECT_MASTER §7.5, V1_DEVELOPMENT §6

## Context

The documented risk is the core silently fusing to SQLite, and then a future personal-sync
layer requiring a rewrite. The docs already forbade it; the question was what actually
enforces it.

## Decision

`vibe-core::ports` declares ten traits and is the only boundary between logic and a store.
Eight are the ones named in §6 (`ProjectRepository`, `EventRepository`, `EntityRepository`,
`ConversationRepository`, `ArtifactRepository`, `SnapshotRepository`, `SearchIndex`,
`SettingsRepository`); two more were added because the §49 tables deserved their own
contract rather than being smuggled through another port: `EvidenceRepository` and
`AuditRepository`.

- `vibe-core` and every crate above it (`state-engine`, `guard-engine`, `context-engine`,
  `ingestion`, `search`) depend on the traits only. `rusqlite` is a dependency of exactly one
  manifest besides the workspace root — `crates/storage/Cargo.toml` — and appears in source
  only under `crates/storage/src`. The other mentions of the word in the tree are the ports'
  own prohibition comment and the fixture text the architecture-drift rule scans.
- `Repositories` bundles the ten as `Arc<dyn …>` so a use-case service takes one argument
  and stays `&self`-callable.
- `EntityStore` is a blanket extension trait over `EntityRepository` so callers get typed
  `entities_of::<Task>()` / `put_entity()` instead of hand-decoding JSON. It must be in
  scope at the call site — an intentional speed bump that keeps the raw JSON port visible.
- `SqliteStore` implements all ten over one `Mutex<Connection>` and hands them out via
  `repositories()`; tests use `open_in_memory()`.

## Consequences

A sync engine (AEP-001) can be inserted under these traits without touching a service. The
cost is indirection in tests and the discipline to never reach for `SqliteStore` from a
service; the `-p` build graph makes that violation a compile error rather than a review
comment.
