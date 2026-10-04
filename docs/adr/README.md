# ADR index

V1_DEVELOPMENT §57's starting list, plus the records this implementation round needed because
it made a choice the docs had not already frozen. An ADR here states what is true of the code
today; forward-looking architecture intent belongs in `docs/evolution/` as an AEP.

| id | title | status |
| --- | --- | --- |
| ADR-001 | Local-first storage | accepted |
| ADR-002 | Event-first, with a disposable projection | accepted |
| ADR-003 | Evidence-first | §17 implemented inside ADR-002's fold — see ADR-014 for how refs are earned |
| ADR-004 | Repository ports, and what sits on each side | accepted |
| ADR-005 | SQLite + WAL | accepted, folded into ADR-001 and ADR-013 |
| ADR-006 | Tauri 2 as the desktop shell | accepted by §17; **not yet built** — see ADR-016 |
| ADR-007 | Browser extension + Native Messaging | accepted by §17; **not yet built** — see ADR-016 |
| ADR-008 | Adapter-first | accepted; shape recorded in AEP-003 |
| ADR-009 | Context Pack | accepted (§41–§43), implemented by `context-engine` |
| ADR-010 | Charter is user-controlled | accepted; enforcement is `CoreError::Locked` + change proposals (§9) |
| ADR-011 | PROJECT_MAP is compiled, and empty sections announce themselves | accepted |
| ADR-012 | No team collaboration | accepted as a permanent exclusion (§4, §16, 禁止 1) — no decision to revisit |
| ADR-013 | One `entities` table with a JSON payload | accepted (deviation from §49, allowed by §50); key corrected by migration v3 |
| ADR-014 | Event identity: store-assigned `seq`, actor and status in the dedup key | accepted (fixes a real defect) |
| ADR-015 | Drift rules are lexical in V1, and they only ever propose | accepted |
| ADR-016 | What this round deliberately does not include | accepted |
| ADR-017 | The system keychain is opt-in; the default secret store is a 0600 file | accepted (a deliberate reading of §53) |
| ADR-018 | The full-text index stores prose, not the record's encoding | accepted (fixes four CLI-visible defects) |
| ADR-019 | `.projectpack` is a stored zip that announces it is a projection | accepted |
| ADR-020 | V1's user surface is the CLI; two test layers, one implementation | accepted |

A gap in the §57 list is deliberate rather than an oversight: ADR-003, 005, 006, 007, 008,
009, 010 and 012 are recorded in PROJECT_MASTER §7 and §19 with enough force that a separate
file would restate the master. Where one of them acquired an implementation decision worth
separating, that decision is the ADR that exists (001, 002, 004, 011).
