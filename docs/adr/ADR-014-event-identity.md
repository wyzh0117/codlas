# ADR-014 — Event identity: store-assigned `seq`, actor and status inside the dedup key

Status: accepted
Date: 2026-10-03
Ref: V1_DEVELOPMENT §15, §18, §27, §51, §52; ADR-002

## Context

Two defects found by the Stage C acceptance tests, both about identity.

1. The reducer's determinism claim rests on a single total order. If a caller could supply
   `seq`, a laptop that was offline for a week would hand back sequence numbers that
   interleave wrongly with the ones already stored, and replay would stop being reproducible.
2. `event_dedup_key(project, type, source, message, subject, payload)` did not include who
   spoke or what the event's judgement status was. So an agent's
   `TASK_COMPLETED_CLAIM` arriving as a `pending_review` candidate and the user's later
   confirmation of the very same subject and words hashed to the *same* key. The
   confirmation was dropped as a duplicate, and the task stayed open forever with a
   "confirmed" state that never landed. `an_agents_claim_stays_a_claim` failed on exactly
   this.

## Decision

- `events.seq` is `INTEGER PRIMARY KEY AUTOINCREMENT`, assigned by the store on insert, and
   the caller's `seq` is ignored on write. `list_accepted(project, after_seq)` and
   `fold`-then-`project_state` both order by it. Tests that need a fixed timestamp own
   `occurredAt`, never `seq`.
- `event_dedup_key` takes `actor_type` and `status` as two extra components. Identity is
   "this source material, saying this, as this kind of statement" — so re-ingesting the same
   transcript row is still idempotent, while a candidate and the fact it was later promoted
   to are two distinguishable rows.
- Reviewing an event in place (`EventStore::review` → `patch_event`) is the normal promotion
   path and preserves the original row; appending a fresh accepted event for the same
   subject is also legitimate, and both now coexist.

## Consequences

`state_hash` covers `last_seq`, so a state fingerprint is tied to the store's ordering, not
to a wall clock — which is what makes cross-machine replay (`the_same_log_projects_to_the_same_hash_in_two_stores`)
meaningful. The cost: a truly identical re-ingest that differs only in judged status creates
a second row by design, so inbox counts must be read as "statements", not "facts".
