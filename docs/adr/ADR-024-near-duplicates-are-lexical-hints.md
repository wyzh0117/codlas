# ADR-024 — Level-3 near-duplicates are lexical hints, and a hint carries the thread it came from

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §52, §59, §60; PROJECT_MASTER §7.2, §19 禁止 9; AEP-002

## Context

§52 splits deduplication into three levels and gives them two different powers. L1 (external
id) and L2 (content hash plus role) are decided at insert and *drop* material, because "the
same bytes again" is a fact a hash can prove. L3 is the soft case — two turns that say almost
the same thing — and §52's rule for it is one clause: **只提示，不直接删除**.

Round 1 shipped L1/L2 and left L3 with neither an implementation nor a surface, so "the same
question twice this afternoon" was invisible. The tempting implementations were the two the
spec forbids: merge the pair into one record, or write a hint into the event log as if it were
a fact. The first destroys the trace §17 exists to keep; the second makes a guess a second
state source (禁止 4).

A second problem is cheaper to hit and worse: the scan needs *a window of recent messages*,
and the port that returns them ordered by `observed_at` alone. A batch import stamps every row
with the same timestamp, so which messages were in the window — and therefore which hints
appeared — depended on the order SQLite's index happened to walk. Same database, different
answer.

## Decision

1. **Lexical, not semantic** (crates/ingestion/src/near_duplicate.rs): Jaccard over token sets,
   default threshold 0.8 — eight of ten distinct words agreeing. Semantic near-duplication
   needs an embedding or a model, which §60 prices as a Strong Model call and 禁止 7 keeps out
   of the core path; the vector version stays AEP-002's job.
2. **A CJK run is compared by character bigrams**, matching the reasoning that
   `guard-engine` already uses: Han has no spaces to split on, so a whole run would otherwise
   be one token and two paraphrased Chinese turns would score 0.0. Deliberately a dumb
   tokenizer, because a dumb one is checkable by eye.
3. **Every hint carries its own caveat and its own thread.** `note` is the constant
   "both messages are still in the database; nothing was merged, deleted or rewritten", and
   `conversation_id` names the thread — so a hint is actionable (`wp duplicates --conversation
   <id>` re-runs scoped to exactly it) and a reader can check it against the records.
4. **Bounded by construction**: a 240-message window, a limit on pairs, results sorted
   most-similar first, excerpts clipped on character boundaries so a truncated Chinese line
   stays readable.
5. **Cost is pruned before it is paid.** Pairs grow as the square of the window, so
   `similarity_upper_bound` rejects a pair from set sizes alone when no score could reach the
   threshold — and the test proves the bound never rejects a pair the real score would accept.
6. **The window's order became deterministic**: `recent_messages` now reads
   `ORDER BY observed_at DESC, rowid DESC` (crates/storage/src/conversation.rs) before
   reversing. `rowid` is store-assigned and monotonic, so it is the tie-break the timestamp
   cannot provide.

## Consequences

- `wp duplicates [--conversation <id>] [--threshold] [--window] [--limit]` is a read-only
  command; there is no path from this module to a write, and `crates/ingestion/tests/near_duplicate.rs`
  asserts exactly that (`the_pass_holds_no_way_to_write`).
- The CLI's e2e test closes the loop the way §52 wants it closed: import, take the hint, import
  the *same* file again, and show that nothing was deleted — the second import reports the rows
  as duplicates rather than fewer messages.
- Fixing the tie-break changed a documented property ("oldest first"), so it is stated here:
  a projection whose inputs depend on index walk order is not deterministic, and §7.2's
  replay promise is what that would have broken — the hint surface just happened to be the
  first thing to read the window.
- Re-running the same scan is stable (the test asserts byte-identical JSON twice), which is what
  lets a UI show hints without flicker later.
- AEP-002's trigger is unchanged. What is new is a lexical stand-in whose number is visible,
  so a user who disagrees with a threshold can say so with evidence.
