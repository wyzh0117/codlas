# ADR-018 — The full-text index stores what a record says, not how it is encoded

Status: accepted (fixes four defects the CLI round surfaced)
Date: 2026-10-03
Ref: V1_DEVELOPMENT §40 (L2), §43; PROJECT_MASTER §19 禁止 4

## Context

The first indexer wrote `serde_json::to_string(record)` into `search_index.body`. Using the
CLI then produced four defects at once:

- a hit's snippet was a JSON blob (`{"evidenceRefs":["evt:…"]…}`) that no one can read;
- a query for a 64-hex hash or an `evt:` id matched nearly every row, because the encoding
  contained those strings;
- L1 hits whose record had no body rendered as blank lines;
- `wp search` bypassed §40's L1/L3 layers entirely, so `wp reindex` was unreachable and the
  three layers never actually ran together.

Each defect passed the library suite of the day; all four were found by running the binary.
That is the instrument ADR-020 exists to keep.

## Decision

1. `searchable_text(value)` (crates/storage/src/entity.rs) walks the JSON tree, keeps string
   leaves, and joins them with ". ". `is_opaque` drops ≥32-char pure-hex strings, so ids and
   hashes stay L1's business instead of flooding L2. Events index
   `"<type> <searchable_text(payload)>"`.
2. `SearchService::find_in(project, query, kinds, limit)` runs the three §40 layers as one
   pipeline: L1 `catalog::records` (structured filters, kind-scoped), L2 FTS5 `snippet()`
   over the same scope, L3 `entity_links` walk in both directions skipping hits whose kind
   the caller excluded. `find` is the no-filter delegate.
3. An L1 hit with an empty body falls back to the title (`Record::hit`): §40 says a hit must
   be explainable, and a blank snippet explains nothing.
4. `wp search [--kind …]` calls `find_in` and prints the layer label, score and snippet;
   `wp reindex` re-encodes every entity and event through the same `searchable_text` path.

## Consequences

- A snippet now reads as the sentence the user wrote. The test
  `crates/search/tests/retrieval.rs::a_hit_reads_as_prose_and_its_encoding_matches_nothing`
  pins both halves: the prose appears, and `evidenceRefs` / `createdByEvent` / a 64-hex id
  match nothing.
- §40's layer label is user-visible (`structured` / `full_text` / `relation`), so a hit can
  still be explained after ranking changes.
- Databases written before this change need `wp reindex` (schema unchanged, content
  rewritten); the log remains the source (§7.2), so reindexing is safe and repeatable.
- `searchable_text` is deliberately shallow: it indexes what a record writes, not what a
  renderer derives. Known limitation (AEP-002, now with reproduced evidence): FTS5
  `unicode61` does not segment Chinese, so a multi-character query inside a longer Han run
  does not match — while `guard-engine`'s own bigram tokenizer (crates/guard-engine/src/text.rs)
  would have. Retrieval is currently the weaker CJK matcher of the two.
