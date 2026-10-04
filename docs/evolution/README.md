# AEP Registry — Architecture Evolution Plans

PROJECT_MASTER §18 keeps only the index; the detail lives here, one file per plan.
Each plan follows the §8.5 shape: Current / Limitation / Why Not Now / Trigger / Target /
Change Surface / Preserved Core / Benefit / Cost / Status.

| id | title | status | trigger (short form) |
| --- | --- | --- | --- |
| AEP-001 | Local-only → Personal Local + Cloud Sync | dormant | multi-device use becomes frequent, or local backup stops being enough |
| AEP-002 | FTS5 → Hybrid Semantic Retrieval | dormant | lexical search measurably misses recall on a real project |
| AEP-003 | Built-in Adapter → Plugin Adapter Runtime | dormant | a third adapter is needed that cannot ship in-tree |
| AEP-004 | Basic Drift Rules → Intelligent Drift Engine | watching | paraphrased contradictions escape ADR-015's lexical rules in practice |
| AEP-005 | Flat Project State → Entity / Architecture Graph | dormant | `entities.data_json` starts needing promoted columns for a real query |

Statuses are the §18 set: `dormant`, `watching`, `triggered`, `in_progress`, `completed`,
`superseded`.

`AEP-004` is `watching` rather than `dormant` on purpose: ADR-015 documents a known blind
spot (a paraphrased contradiction with no shared vocabulary is missed), so the condition for
starting that plan is already partially observable. Nothing has been built for it, and
禁止 8 still applies — watching means the trigger is instrumented, not implemented.

The V1 code leaves specific hooks for these plans, and they are the reason the plans are
cheap later rather than expensive:

- AEP-001: `ProjectEvent.originNodeId`, `SyncCursor.lastContentHash`, and ten ports with
  exactly one SQLite implementation (ADR-004).
- AEP-002: `SearchIndex` is a port, and `vibe-search` composes L1/L2/L3 above it, so a
  semantic layer slots in as another layer instead of replacing search.
- AEP-003: `AgentAdapter` is trait-object registration keyed by id and `SourceType`; no
  pipeline stage branches on a vendor name.
- AEP-005: `entity_links` and `entity_versions` already exist, so the graph is an extension
  of stored data rather than a new store.
