# AEP-003 — Built-in Adapter → Plugin Adapter Runtime

Status: dormant
Owner: architecture
Ref: V1_DEVELOPMENT §36–§38, §58; PROJECT_MASTER §7.6, §18, 禁止 7

## Current

Adapters are Rust types implementing `AgentAdapter` (id, source type, detect, capabilities,
ingest, and the optional open/prepare/deep-link trio), registered into `IngestPipeline` by
the CLI at start-up. `detect()` inspects an injected filesystem root and never a hard-coded
vendor path; a subprocess only ever runs through the injected `ProcessRunner`.
Capabilities are declared as data (§37) and the pipeline consults capabilities rather than
names.

## Limitation

Adding an adapter means editing Rust and rebuilding. A format the tool meets in the wild —
an agent's private session log, a new chat UI — cannot be handled without a release cycle.

## Why Not Now

§38 scopes V1 to manual import, generic web, ChatGPT web, one CLI or local agent, and basic
MCP. Fewer than three adapters exist to date, so a plugin ABI would be a speculative
boundary (禁止 8). The §58 adapter contract tests are the part worth having now, and they are
in-tree.

## Trigger

- A third party needs an adapter the shipped binary does not contain, and waiting for a build
  is the actual complaint; or
- two adapters independently grow the same helper (transcript parsing, cursor arithmetic),
  which is the signal that the extension point is real.

## Target

Adapters as external programs speaking a versioned JSON protocol over stdio, declared by a
manifest listing `id`, `sourceType` and `capabilities()`. The in-tree adapters keep working
by implementing the same trait — the runtime is additive, not a rewrite.

## Change surface

- New `capabilities()` handshake schema plus the `packages/adapter-sdk` package §5 lists.
- A sandbox decision: an adapter that reads files is outside the Rust type guarantee, so the
  injected-root and `ProcessRunner` discipline becomes an enforced protocol rule.
- §51's cursor contract moves across the process boundary and must stay idempotent under a
  crashing adapter.
- Trust and policy: §53's project policy must gate what an adapter may read, not merely what
  the core may send.

## Preserved core

§7.6 stays literal: the core never learns a vendor name. Event-first capture is unchanged —
an adapter produces normalized messages and evidence; only the judge turns them into events.

## Benefit / Cost

Benefit: new agents without a release. Cost: an ABI to support forever, plus a security
boundary that today is a compile-time guarantee.
