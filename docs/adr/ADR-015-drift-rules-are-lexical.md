# ADR-015 — Drift rules are lexical in V1, and they only ever propose

Status: accepted
Date: 2026-10-03
Ref: V1_DEVELOPMENT §26, §29, §8.6 (PROJECT_MASTER); AEP-004

## Context

§26 specifies V1 drift detection as "Rules + Structured Comparison + Optional LLM Judge"
and §8.6 states a Drift Alert is a prompt that must not block development. The temptation
is to reach for a model to decide "does this contradict that decision". The risk of giving
in: an unexplainable alert, a per-check API cost, and a rule that silently changes behaviour
when the model changes.

## Decision

All six V1 rules are deterministic and run in `guard-engine` with no model in the loop:

| rule | signal it can actually see |
| --- | --- |
| `scope_drift` | task/issue/proposal wording vs `outOfScope` + `forbidden` items |
| `decision_conflict` | high token overlap with an **Active** decision *and* a flipped negation or opposition-vocabulary hit |
| `architecture_drift` | `ArchitectureInvariant.detect` patterns matched against artifact paths and layer role; `scan_source(state, path, text)` is the precise textual path |
| `feature_creep` | new work with no anchor in `mustHave`, `inScope`, an open change proposal or an AEP |
| `repeated_redesign` | `redesign_count ≥ 3`, or ≥3 supersede/reverse hits on one subject |
| `stale_assumption` | active decision older than an injected `now` minus threshold, contradicted by newer events |

Supporting rules that are load-bearing, not incidental:

- Alert ids are content-addressed over `(project, type, summary, subject)`, so `check()` is
  idempotent and never piles up duplicates. An alert a user already judged is never
  resurrected.
- Every alert carries non-empty `baseline_refs` (what it was measured against) and
  `evidence_refs` (which events/entities triggered it). An alert that cannot cite its
  baseline is not allowed.
- The engine never mutates charter, scope or baseline, and never applies a change proposal's
  delta. Accepting a proposal records status plus `CHANGE_PROPOSAL_ACCEPTED`; a user applies
  it. That is 禁止 5 kept at the engine boundary rather than in the UI.
- Matching is CJK-aware (bigram tokenizer) because the project's own records are Chinese.

## Consequences

Known, documented blind spot: a paraphrased contradiction with no shared vocabulary and no
negation marker is missed. That is the exact gap AEP-004 (Basic Drift Rules → Intelligent
Drift Engine) exists to close, and its trigger condition is unchanged — the rule layer is the
floor it will be measured against, not something it has to be rewritten around.
