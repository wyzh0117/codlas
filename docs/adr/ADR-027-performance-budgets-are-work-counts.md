# ADR-027 — A performance budget is a work count first and a stopwatch number second

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §59, §60; PROJECT_MASTER §7.2, §7.9, §13.2 Stage H, 禁止 7, 禁止 8

## Context

§59 asks for performance budgets on the deterministic chain (replay, projection, map compile,
context compile) and §60 asks for a cost rule: the chain runs on hash, rules, SQLite, FTS, git,
reducer and diff — no model, no network. Round 1 recorded both as undone.

The obvious way to write these tests is to time something and assert a number. On a laptop that
also runs a browser, that produces a suite which fails for reasons unrelated to the code and
teaches the reader to ignore it. Meanwhile the claim §60 actually makes is a *dependency* claim,
which no timing test can check.

## Decision

1. **Two timed budgets, and only two.**
   - `FOLD_BUDGET_MS = 250` for the pure projection of a 1 200-event log, measured at ~40 ms on
     this machine's debug build. Tight enough that turning one pass into an n² pass, or a fold
     into a round trip, trips it immediately.
   - `CHAIN_BUDGET_MS = 20_000` for the same chain *including* SQLite materialization, measured
     at ~6 s. Deliberately loose: a disk-bound number on a loaded machine is not something a
     suite may honestly assert, so this one is a hang and n² tripwire, not a benchmark.
   A failure prints the per-stage costs, so a trip tells you what changed rather than only
   failing.
2. **Everything else is a work count, not a duration**: which rows a read touched, how many
   events a replay pulled, how many candidates a query scanned, how many manifests a rule read.
   `opening_a_project_answers_from_stored_rows_not_from_the_log`,
   `an_incremental_read_pulls_only_what_is_new`, `materialized_rows_read_back_the_projection`
   and the two search tests (`an_indexed_query_does_not_cost_more_because_the_workbench_grew`,
   `a_query_at_volume_returns_what_it_was_asked_for_and_no_more`) all assert counts.
3. **The cache is proved a projection, not a second truth**: materialized rows are compared
   against the same state a pure fold produces — §59's speed must not cost 禁止 4's single source.
4. **§60's cost rule is enforced in `Cargo.toml`** (`no_manifest_in_the_workspace_reaches_a_network_or_a_model`):
   every workspace manifest is read and a banned list (reqwest, hyper, sqlx, openai, ollama,
   tokio, axum, tonic, oauth2, …) must not appear, and `rusqlite` may appear in exactly one
   manifest, `crates/storage`. A dependency edge is the cheapest place to make the claim
   unbreakable and the only place that stays true after somebody adds a feature.
5. **The fixture is sized to be a test, not a benchmark**: 1 200 events, four rounds of
   create/start/complete with a decision every fourth round. Large enough that a quadratic fold
   is visibly wrong; small enough that the suite stays under a second per assertion path.

## Consequences

- `cargo test -p vibe-state-engine --test budgets` and `crates/search/tests/budgets.rs` are the
  Stage H "Performance" acceptance evidence, and both run inside the ordinary suite.
- The manifest walk is now the reason the MCP server crate (ADR-026) is on the list of checked
  directories: adding an HTTP or model dependency there fails a test about §60, which is exactly
  the coupling the rule wants.
- Timing bounds are stated with their measurements, so a slower machine knows what it is being
  asked to tolerate; when a bound trips, the message carries the numbers, not just the assertion.
- This does not measure UI latency, memory or concurrency, and does not pretend to: V1 has no
  surface those would apply to (ADR-016).
