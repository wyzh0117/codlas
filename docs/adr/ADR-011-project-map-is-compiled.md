# ADR-011 — PROJECT_MAP is compiled, and empty sections announce themselves

Status: accepted (design), implemented 2026-10-03
Ref: PROJECT_MASTER §10, 禁止 6, V1_DEVELOPMENT §30, §31, §33

## Context

§30 forbids letting an LLM rewrite the whole map each time. §10 defines the map as a
"readable projection" of current state. The open implementation question was what a
deterministic compiler does when a section has nothing to say — dropping the heading is the
easiest thing, and also the most dangerous, because a workbench that quietly omits
"Active Decisions" is indistinguishable from one that has none.

## Decision

- `ProjectMapCompiler::render` reads a `ProjectState` and nothing else — no repository, no
  clock, no model. `compile()` supplies the state and the tail of the event log;
  `generatedAt` lives on `ProjectMap`, never inside the markdown body, so identical state
  produces byte-identical markdown.
- All thirteen §31 headings are always emitted in the documented order. An empty section
  renders `EMPTY_PLACEHOLDER` (`_none recorded_`).
- "Next Actions" is generated from declared gaps, not from a model's guess: a missing
  charter, an unverified completion claim, open issues, dormant AEPs.
- A compiled map carries the `source_state_hash` it came from, and `ProjectMapCompiler::save`
  persists the pair, so the workbench can prove which state a displayed map was compiled from.
  `compile()` is side-effect-free apart from refreshing the projection; a snapshot takes its
  own copy of the map text so a frozen view cannot drift.
- Snapshots keep their own map copy; the "since last snapshot" view (§33) is a diff of two
  states, with its own markdown shape, and is the thing a returning user reads first.

## Consequences

The map is cacheable and cheap (§59), and a missing section is information rather than an
absence. Wording polish by a model remains optional and outside this path — if it is ever
added it must produce a separate field, never overwrite the compiled body.
