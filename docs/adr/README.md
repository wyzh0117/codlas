# ADR 目录 —— 内容已汇总到一个 HTML 页

本目录的 19 份 `ADR-0NN-*.md` 已在 2026-10-05 汇总为 **[`ADR-总览.html`](ADR-总览.html)**
并删除（汇总前的原文在 `git show 54cdb5d:docs/adr/<文件名>.md`）。
**决定本身的措辞未作任何修改**，改变的只有载体。页面现在共 **20** 条：ADR-028 是介质变更之后
直接在 HTML 里追加的第一条，没有对应的 `.md` 原文。

ADR = Architecture Decision Record，架构决策记录：一条只回答"为什么代码今天长这样"，
固定 Context / Decision / Consequences / Ref 四块。定义与职责见 `PROJECT_MASTER.md` §11；
§57 的起始清单见 `V1_DEVELOPMENT.md` §57。前瞻性意图不属于这里，属于 `docs/evolution/` 的 AEP。

## 索引

| 编号 | 决定 | 状态 |
| --- | --- | --- |
| [ADR-001](ADR-总览.html#adr-001) | Local-first storage | accepted，已实现 |
| [ADR-002](ADR-总览.html#adr-002) | Event-first, with a disposable projection | accepted，已实现 |
| [ADR-004](ADR-总览.html#adr-004) | Repository ports, and what sits on each side | accepted，已实现 |
| [ADR-011](ADR-总览.html#adr-011) | PROJECT_MAP is compiled; empty sections announce themselves | accepted，已实现 |
| [ADR-013](ADR-总览.html#adr-013) | One `entities` table with a JSON payload | accepted（§49 的许可偏差） |
| [ADR-014](ADR-总览.html#adr-014) | Event identity: store-assigned `seq`, actor 与 status 进 dedup key | accepted（修真缺陷） |
| [ADR-015](ADR-总览.html#adr-015) | Drift rules are lexical in V1, and they only ever propose | accepted |
| [ADR-016](ADR-总览.html#adr-016) | What V1's first round deliberately does not include | accepted，**amended 2026-10-05 + 2026-10-06**（MCP 已做 → §38.5 入站方向记为还欠） |
| [ADR-017](ADR-总览.html#adr-017) | The system keychain is explicit opt-in | accepted |
| [ADR-018](ADR-总览.html#adr-018) | The full-text index stores prose, not the record's encoding | accepted（修四条 CLI 可见缺陷） |
| [ADR-019](ADR-总览.html#adr-019) | `.projectpack` is a stored zip that says it is a projection | accepted |
| [ADR-020](ADR-总览.html#adr-020) | V1's user surface is the CLI; two test layers, one implementation | accepted，**amended 2026-10-05**（两个表面） |
| [ADR-021](ADR-总览.html#adr-021) | An adapter's health has four outcomes | accepted |
| [ADR-022](ADR-总览.html#adr-022) | Watch signals are events with a verb; they move no state | accepted |
| [ADR-023](ADR-总览.html#adr-023) | A backup is an online copy, both directions | accepted |
| [ADR-024](ADR-总览.html#adr-024) | Level-3 near-duplicates are lexical hints | accepted |
| [ADR-025](ADR-总览.html#adr-025) | The adapter contract answers all seven verbs | accepted，**amended 2026-10-06**（prepare 改问 `fallback()`） |
| [ADR-026](ADR-总览.html#adr-026) | The MCP surface is a second door, and it only opens inward | accepted，**amended 2026-10-06**（另一半见 ADR-028） |
| [ADR-027](ADR-总览.html#adr-027) | A performance budget is a work count first | accepted |
| [ADR-028](ADR-总览.html#adr-028) | §38.5 有两扇门：读出去的那扇已建，读进来的入站采集**还欠着** | accepted（2026-10-06） |

缺号是刻意的：ADR-003、005、006、007、008、009、010、012 已由 `PROJECT_MASTER.md` §7 / §19
钉得够死，另开一条只会复述总控；其中 ADR-006（Tauri）与 ADR-007（浏览器扩展）的"尚未构建"状态
记在 ADR-016 里。编号不可回收，下一条从 **ADR-029** 起。

## 新增一条 ADR 的做法

1. 在 `ADR-总览.html` 的「五、全部决定原文」末尾追加一节
   `<h2 id="adr-029">ADR-029 — &lt;标题&gt;</h2>`，带上 Status / Date / Ref 三行 meta，
   再写 Context / Decision / Consequences（正文照上一节的标记风格来）。
2. 在本页索引表和该页「二、索引」表各加一行，并把该页头部的条数徽章与「六、来源与变更」一起改。
3. 决定被后续轮次改变时**就地修订**原条目，Status 标 `amended 日期（改了什么）`，
   不叠第二份说法——见 `docs/RECORDS.md` 2026-10-05 的介质变更记录。
4. `PROJECT_MASTER.md` §20 的 LAST COMPLETED 里引用 `(ADR-0NN)` 即可，不写路径。
