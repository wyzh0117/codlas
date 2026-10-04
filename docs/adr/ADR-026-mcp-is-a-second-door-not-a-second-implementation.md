# ADR-026 — The MCP surface is a second door onto the same ports, and it only opens inward

Status: accepted
Date: 2026-10-05
Ref: V1_DEVELOPMENT §38.5, §29, §53, §54, §59, §60, §62; PROJECT_MASTER §7.5, 禁止 4, 禁止 7, 禁止 8

## Context

§38.5 asks for a "basic MCP interface" so an agent can reach the workbench directly. Round 1
deferred it (ADR-016) on the grounds that the context compiler already produced a
`PreparedPayload`, which was true and still is: MCP adds an entry point, not a capability.

Three ways to build it badly were available:

- a network server (a port to guard, a second lifecycle, and a cost §60 says the deterministic
  chain does not have);
- a parallel implementation of "read a project" for the tool layer, which would be 禁止 4's
  second state source with a new protocol on top;
- a write-capable surface, which would move §29's judgement — review, confirm, resolve — out of
  the terminal, where a human is the one pressing return.

## Decision

1. **stdio only.** `crates/mcp-server` speaks MCP's newline-delimited JSON-RPC 2.0: one message
   per line in, one answer per line out, `initialize` / `ping` / `tools/list` / `tools/call` and
   the `notifications/initialized` notification (answered with silence, as the transport
   requires). No async runtime, no HTTP: the crate's manifest declares no network or model
   dependency, and `no_manifest_in_the_workspace_reaches_a_network_or_a_model`
   (ADR-027) is the test that keeps it that way.
2. **Six tools, all reads, and the names are checked.** `project_state`, `project_map`,
   `project_timeline`, `search`, `open_evidence`, `context_pack`. The catalogue test asserts no
   tool name contains `confirm`, `set_`, `create`, `delete`, `resolve` or `append` — a rename
   that smuggles a verb in fails a test rather than a review.
3. **Every tool body is a call to the service the CLI already calls** — `Reducer`,
   `ProjectMapCompiler`, `EventStore`, `SearchService`, `EvidenceRepository`,
   `ContextCompiler` — all through `Repositories`. The server is a second surface, not a second
   implementation (禁止 4, 禁止 7).
4. **`context_pack` goes through the same gate.** Same Policy Filter, same §54 audit row, so a
   pack that leaves down a pipe is no freer than one printed for the user, and the reading is
   recorded either way.
5. **Project resolution follows the CLI's order** — explicit `projectId`, then the `--project`
   the server started with, then the pointer `wp project use` left, then the lone project. With
   two projects and no naming, the server refuses **and lists them by id and name**; guessing
   would let one project's facts answer for another.
6. **Three failure classes, kept apart.** Unknown tool and malformed arguments are the client's
   mistake, so JSON-RPC answers them (-32602). A workbench refusal is a *result* with
   `isError: true` (§62: an agent reads it instead of inferring a crash). Blank queries, an
   oversized `limit` and an over-threshold request are refused rather than silently truncated.
7. **stdout is protocol, diagnostics are stderr.** `wp-mcp` prints where it serves and how many
   messages it answered to stderr only; `--print-config` emits the client snippet so a config
   file is copied rather than invented.
8. **The event-log tool reads oldest-first.** The store returns its newest window in `seq DESC`
   because that is what a `LIMIT` must cut from; the tool reverses it, so a model reading a
   history reads it in the order it happened and a `limit` still means "the most recent ones".

## Consequences

- §14's "发给任意 Agent" leg now has a standard read interface, while sending remains ADR-025's
  human-carried fallback. Nothing in this crate can change a project's memory.
- A test can drive the whole protocol without a child process (`handle(&str) -> Option<String>`),
  which is how eleven of the suite's tests run; `the_built_binary_serves_a_real_pipe` spawns the
  actual `wp-mcp` to prove stdout stays protocol-only and a closed stdin is a clean exit. The
  same two-layer discipline as ADR-020.
- Isolation is per-call: a `search` never returns another project's record, which the suite
  proves by putting a rare token in a second project.
- §38.5's *basic* interface is what shipped. Prompts, resources, sampling and tool annotations
  are not in V1's acceptance list and are not scaffolded here (禁止 8); adding one is a change to
  `dispatch`, not to the tools' contract.
- A `wp` subcommand for the server was not added: the server is started by a client's config, not
  by a human typing, and `--print-config` covers the one thing a human needs to copy.
