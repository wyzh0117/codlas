# ADR-020 — V1's user surface is the CLI; two test layers, one implementation

Status: accepted, amended 2026-10-05 (the e2e count grew; §38.5's MCP door is now named)
Date: 2026-10-03
Ref: V1_DEVELOPMENT §61 (stage G), PROJECT_MASTER §14, §20; ADR-006, ADR-016, ADR-026

## Context

§14's acceptance list is behavioural ("选中一个问题即可交给任意 Agent"), not architectural.
With the Tauri shell (ADR-006) not built, the CLI is the only surface a *human* can actually
touch — which makes it the thing the acceptance test must exercise. Testing only the libraries
would leave the argument layout, file reads and print formats unproven, and those broke in
practice: flattened verbs (`wp audit`, not `wp view audit`) and zsh's non-word-splitting
produced failures no library test could see. The one other surface V1 now has, `wp-mcp`
(ADR-026), faces an agent rather than a human and is deliberately built on these same ports,
so it does not soften the rule below.

## Decision

- `apps/cli` stays thin: parse, borrow files, call the same services the workbench will call,
  print. No logic lives there that a desktop UI would have to reimplement — the concrete form
  of "界面是核心逻辑的展示层，不是第二套逻辑" (PROJECT_MASTER §26).
- `apps/cli/tests/golden.rs` (7 tests) drives the libraries against `tests/fixtures/` and
  pins compiled artifacts, including that the six drift rules fire on the material they are
  for and that replaying the same log twice yields the same state and map.
- `apps/cli/tests/cli_e2e.rs` (17 tests as of 2026-10-05) spawns the real binary
  (`env!("CARGO_BIN_EXE_wp")`) in a tempdir with `--db`, clearing `WP_DB` and
  `VIBE_SECURITY_ALLOW_KEYCHAIN` so nothing leaks between tests. The suite walks §14's loop:
  charter → transcript import → map save → pack → policy refusal → secret redaction →
  prepare → six exports + `.projectpack` → routing advice → layered search → bound-directory
  capture idempotence → capture refusal when unbound, and since the second round the same
  loop through `evidence`, `backup create|verify|restore`, `adapters`, `watch` and `duplicates`.
- The e2e helper (`Workbench::ready`) is the smallest possible project — `init` with a
  charter, `import transcript`, `map save`. A new test starts from those three lines, not a
  bespoke fixture.

## Consequences

- The CLI's *shape* is contract: `Baseline`/`Capture`/`View` are flattened verbs
  (`wp audit`, `wp import transcript`), while `Context`/`Policy`/`Secret`/`Export` are
  subcommand groups (`wp context build`). Changing that is a breaking change and a test
  change together.
- Feature correctness is still judged by using the binary: the four defects recorded in
  ADR-018 were found by running it, not by the library suite. The e2e layer exists so the
  next round starts from that instrument.
- The library tests stay the fast inner loop; the e2e suite spawns processes but is
  deliberately small (17 tests) and runs in the same `cargo test --workspace`.
- When the Tauri shell (V1-E) lands it must go through the same ports. The CLI is not
  scheduled for removal — it remains the workbench's headless form (ADR-016).
- V1 now has two surfaces, not one: `wp` for the human and `wp-mcp` for an agent (ADR-026).
  The distinction this ADR protects is still exactly one — neither gets to hold logic. Both
  are front doors over the same services, and a third door (the desktop UI) inherits that rule
  rather than needing a new decision.
