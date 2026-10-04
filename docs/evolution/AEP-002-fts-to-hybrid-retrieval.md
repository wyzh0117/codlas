# AEP-002 — FTS5 → Hybrid Semantic Retrieval

Status: dormant
Owner: architecture
Ref: V1_DEVELOPMENT §40, §56; PROJECT_MASTER §15.1, §18; ADR-015

## Current

Three retrieval layers, all zero-cost or cheap (§60), exposed through one port:
- L1 structured filters over `entities(kind, project_id, status)`;
- L2 SQLite FTS5 `unicode61 remove_diacritics 2`, ranked by `bm25`, with `snippet()`;
- L3 relation walk over `entity_links` in both directions.

The context compiler consumes L1/L2/L3 only through `Repositories::search`.

## Limitation

Lexical matching needs vocabulary overlap. "登录模块" does not retrieve a record written as
"session reconnection" or "认证中间件", and a Chinese project mixed with English identifiers
splits its own vocabulary across languages.

### Reproduced evidence (2026-10-03, this repository's own charter)

`unicode61` classifies Han characters as alphanumeric, so a whole run between punctuation
becomes **one token** — it has no notion of a Chinese word:

```
$ wp search 脱离                  → nothing found
$ wp search 项目逐渐脱离最初设计   → full_text hit; the snippet marks the entire run
```

`guard-engine` already tokenizes CJK as bigrams for drift matching
(`crates/guard-engine/src/text.rs`), so retrieval is currently the *weaker* CJK matcher of
the two. This is a reproduced miss, though the trigger below still requires evidence from the
user's own project (a pack missing a decision they expected to see), not merely a
reproduction.

Cheaper intermediate than embeddings, if only CJK recall is the problem: rebuild
`search_index` with `tokenize='trigram'` (the bundled SQLite 3.53.2 supports it; no model,
no index files). It makes 3+ character CJK substrings match; a 2-character word like 脱离
would still miss. That half-step is part of this AEP's trigger evaluation, not of V1.

## Why Not Now

A vector index means embeddings, a model dependency, a re-index cost and a sync question —
against acceptance criteria (§14) that FTS5 already satisfies. §40 explicitly defers it.

## Trigger

- Retrieval demonstrably misses recall on the user's own project — evidence being a context
  pack that lacked a decision the user expected to see, not a hunch.
- Or the inbox / drift engine needs near-duplicate detection at §52's L3 (semantic
  near-duplicate), which is the same machinery.

## Target

Hybrid: FTS5 stays the cheap first pass, a local embedding index supplies candidate recall,
and results are fused (reciprocal rank) with the layer label kept visible so a hit can still
be explained.

## Change surface

- `SearchIndex` port gains a vector candidate method; `vibe-storage` remains the only crate
  that knows how anything is persisted.
- New index files under the project data dir; reindex cost for existing projects.
- §43's budget allocation must account for semantically-near-but-not-quoted evidence.
- Policy interaction: embeddings must not be computed by a cloud model when
  `ProjectPolicy.local_only` or `allow_cloud_model = false` (§53).

## Preserved core

Context is still compiled from structured state (§7.7); retrieval improves what is selected,
never who decides. Evidence refs stay the unit of traceability.

## Benefit / Cost

Benefit: cross-lingual and paraphrase recall — the exact blind spot ADR-015 records for drift
too. Cost: model dependency, index size, and explaining a ranked list that is no longer
purely lexical.
