# ADR-025 — The adapter contract answers all seven verbs, and answers them only what it advertised

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §37, §58, §62 Degradation, §7.8; PROJECT_MASTER §19 禁止 8; ADR-008 / AEP-003

## Context

§58 names seven verbs — `detect`, `capabilities`, `ingest`, `open`, `prepare`, `inject`,
`fallback` — and V1_DEVELOPMENT's acceptance line for the layer is that **every** adapter meets
the contract. Round 1 shipped the first three plus `open`/`prepare`, and had no test that a
`Box<dyn AgentAdapter>` could be asked all seven. `inject` was simply absent, and `fallback`
existed only as prose inside `wp context prepare`'s printout, so the one verb §62 makes
mandatory ("degradation is reported, never hidden") was the verb the trait could not answer.

An SDK with default method bodies invites exactly one failure: an adapter that advertises
`OPEN_APP` or `INJECT_TEXT` in its capabilities and then does nothing when the workbench calls
for it. Nothing in the type system can prevent that, so the contract test has to.

## Decision

1. **`inject` joins the trait**, defaulting to `Ok(None)` like `open` and `prepare`. The verb
   exists so that adding a real transport later is an `impl`, not a change to callers or to §58.
2. **`fallback` joins the trait, and defaults to an answer** rather than to `Option`: a
   `Fallback { adapterId, instruction, deepLink, payloadPath }` built from the adapter's own id
   and its `deep_link(target)`. It is the only verb that cannot return nothing, because silence
   is the failure §62 describes.
3. **A rusted-shut adapter is still a complete adapter.** Every write-side verb has a default,
   so a reader that only implements the three required verbs answers all seven — which is what
   lets V1 ship adapters that read and send nothing.
4. **The review is capability honesty**, run over every shipped adapter in
   `crates/ingestion/tests/contract.rs`: no verb may be answered unless §37's `AgentCapabilities`
   claims it, every claimed verb must be answerable, an absent source (`detect() == false`) is a
   fact rather than a breach, and a fallback must name the payload the user is being asked to
   carry. `a_thread_link_shows_up_in_the_fallback_it_belongs_to` covers the thread-scoped case.
5. **The review is tested against itself.** `an_adapter_may_not_answer_a_verb_it_did_not_advertise`
   is a negative control: an adapter that claims `PREPARE_PAYLOAD` but never produces one is
   named by the review. Without that test, a green suite would prove nothing, because defaults
   make an empty adapter look compliant.
6. **V1 ships no adapter that writes to an agent.** `inject` is exercised by fixtures
   (`Willing`, `WritesAFile`) and by the honest `None` from the real ones; `AUTO_EXECUTE` is
   never claimed (§37), and §7.8 keeps sending a human action. Implementing the write side is
   AEP-003's territory, not this round's (禁止 8).

## Consequences

- `wp context prepare` still prints its own copy instruction from `PreparedPayload.copy_fallback`;
  the two say the same thing today, and `fallback()` is now the version a UI or an adapter can
  ask for instead of a string written where it is used. Migrating the CLI's print onto the verb
  is a one-line change with no behaviour difference, so it was left for the UI round rather than
  done for its own sake.
- Adding an adapter means passing `review()`; a third-party adapter gets the same check as the
  built-ins, which is the shape AEP-003 needs to make plugins plausible.
- A `payloadPath` of `None` means "the text is only in the terminal" — an explicit statement
  rather than an absent field, so a UI cannot render a phantom file link.
- The gap this closes is honest about what remains open: the verbs are answerable, but the one
  that would move a payload into another app still returns `None` for every adapter V1 ships,
  and ADR-016 keeps that deferred rather than half-built.
