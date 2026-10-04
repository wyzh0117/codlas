# AEP-004 — Basic Drift Rules → Intelligent Drift Engine

Status: watching
Owner: architecture
Ref: V1_DEVELOPMENT §26, §27, §29; PROJECT_MASTER §8.6, §15.1, §18; ADR-015

## Current

Six deterministic rules in `guard-engine` (scope drift, decision conflict, architecture
drift, feature creep, repeated redesign, stale assumption) plus an inbox that aggregates
them with pending events and open change proposals. Matching is lexical with a CJK-aware
bigram tokenizer; an alert is content-addressed so `check()` is idempotent; every alert
carries `baselineRefs` and `evidenceRefs`. See ADR-015.

## Limitation

Lexical rules need shared vocabulary. A decision restated in different words — "让 core 直接
用 sqlite" against the invariant "Core 不直接依赖 SQLite" — is caught today only because the
negation flips and tokens overlap. A genuinely paraphrased contradiction, or a drift that is
visible only across several conversations at once, is missed. The reverse failure also
exists: three unrelated tasks sharing a common noun can look like feature creep.

## Why Not Now — and why the status is `watching`

§26 asks for rules first, and 禁止 6 forbids letting a model's opinion overwrite project
facts. Rules are also what make an alert explainable, which is the product's point. So the
architecture is not changed on speculation.

`watching` rather than `dormant` because ADR-015 already documents the blind spot as a
consequence of a decision taken, and the CLI surfaces alert provenance. The condition is
observable now; nothing has been built for it.

## Trigger

- A real project shows a contradiction the rules missed — the strongest signal; or
- precision drops far enough that the user dismisses most alerts, at which point smarter
  ranking pays for itself; or
- drift detection needs cross-conversation inference, which rules cannot express.

## Target

Rules stay the floor. A model layer ranks, groups and explains candidate alerts, and adds
contradiction detection over restatements. §60's cost split applies: cheap model for
classification, strong model for architecture comparison.

## Change surface

- A candidate stage between the rules and the inbox; `DriftType` gains no new member but
  `DriftSeverity` may need a model-assigned confidence.
- §27's thresholds (`auto_accept 0.90`, `inbox_floor 0.65`) become per-rule or per-model, and
  must stay user-tunable and persisted — that plumbing already exists in `settings`.
- Prompt cache and audit: §54 must record what a model was shown.
- Alert dedup by content hash has to tolerate a model re-wording an alert about the same
  underlying conflict — identity must stay attached to the cited baseline refs.

## Preserved core

§8.6 unchanged: an alert is a prompt, never a blocker. An alert is never persisted without
`baselineRefs` and `evidenceRefs`. The engine still writes no charter, scope or baseline
mutation, and never applies a change proposal's delta — a model proposes, a user decides
(禁止 5).

## Benefit / Cost

Benefit: catching the drift that actually hurts — direction change expressed in new words.
Cost: per-check spend, non-determinism to test around, and the standing risk that a confident
model produces an alert the user trusts without checking.
