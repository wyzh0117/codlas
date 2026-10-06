# ADR 目录 —— 内容已汇总到一个 HTML 页

本目录的 19 份 `ADR-0NN-*.md` 已在 2026-10-05 汇总为 **[`ADR-总览.html`](ADR-总览.html)**
并删除（汇总前的原文在 `git show 54cdb5d:docs/adr/<文件名>.md`）。
**决定本身的措辞未作任何修改**，改变的只有载体。页面现在共 **27** 条：ADR-028 到 ADR-035 是介质
变更之后直接在 HTML 里追加的八条，没有对应的 `.md` 原文。

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
| [ADR-013](ADR-总览.html#adr-013) | One `entities` table with a JSON payload | accepted（§49 的许可偏差），**amended 2026-10-06**（它引用的 §52 L2 键已由 ADR-033 改成含 role） |
| [ADR-014](ADR-总览.html#adr-014) | Event identity: store-assigned `seq`, actor 与 status 进 dedup key | accepted（修真缺陷） |
| [ADR-015](ADR-总览.html#adr-015) | Drift rules are lexical in V1, and they only ever propose | accepted |
| [ADR-016](ADR-总览.html#adr-016) | What V1's first round deliberately does not include | accepted，**amended 2026-10-05 + 2026-10-06 ×4**（MCP 已做 → §38.5 入站还欠；V1-E 桌面壳已做 → ADR-029；V1-D 浏览器门已做 → ADR-031；§14 第 7 项已是一只点击） |
| [ADR-017](ADR-总览.html#adr-017) | The system keychain is explicit opt-in | accepted |
| [ADR-018](ADR-总览.html#adr-018) | The full-text index stores prose, not the record's encoding | accepted（修四条 CLI 可见缺陷） |
| [ADR-019](ADR-总览.html#adr-019) | `.projectpack` is a stored zip that says it is a projection | accepted |
| [ADR-020](ADR-总览.html#adr-020) | V1's user surface is the CLI; two test layers, one implementation | accepted，**amended 2026-10-05 + 2026-10-06 ×2 + 2026-10-06（第五轮）**（桌面壳是同一实现的第二扇门；e2e 22 条 + views 层 **23** 条 + `native_messaging` 层 13 条 + 新建 `resolution` 3 条） |
| [ADR-021](ADR-总览.html#adr-021) | An adapter's health has four outcomes | accepted |
| [ADR-022](ADR-总览.html#adr-022) | Watch signals are events with a verb; they produce no verdict | accepted，**amended 2026-10-06**（文件信号确实写 artifact 行，"不改状态"改成"不产生结论"） |
| [ADR-023](ADR-总览.html#adr-023) | A backup is an online copy, both directions | accepted |
| [ADR-024](ADR-总览.html#adr-024) | Level-3 near-duplicates are lexical hints | accepted |
| [ADR-025](ADR-总览.html#adr-025) | The adapter contract answers all seven verbs | accepted，**amended 2026-10-06**（prepare 改问 `fallback()`） |
| [ADR-026](ADR-总览.html#adr-026) | The MCP surface is a second door, and it only opens inward | accepted，**amended 2026-10-06**（另一半见 ADR-028） |
| [ADR-027](ADR-总览.html#adr-027) | A performance budget is a work count first | accepted，**amended 2026-10-06**（manifest 巡检纳入 `apps/desktop`，共 12 份） |
| [ADR-028](ADR-总览.html#adr-028) | §38.5 有两扇门：读出去的那扇已建，读进来的入站采集**还欠着** | accepted（2026-10-06） |
| [ADR-029](ADR-总览.html#adr-029) | 桌面壳是 CLI 同一个 runtime 的第二扇门，唯一的写是 §7.4 的确认 | accepted（2026-10-06），**amended**（§14 第 7 项的那只"手"落在壳里：点一行 → 聚焦的 Context Pack） |
| [ADR-030](ADR-总览.html#adr-030) | 创建事件必须携带整条记录，因为折叠只从日志重建行 | accepted（2026-10-06，修 §7.4 静默失效的真缺陷），**amended**（同一份缓存在 §42.1 选择路径上第二次咬人：编译包之前显式物化） |
| [ADR-031](ADR-总览.html#adr-031) | 浏览器的门是四个动词的推送通道，`open` / `inject` 答的是指令不是动作 | accepted（2026-10-06，第四轮），**amended**（§35 的 origin 检查有两副面孔：`hello` / `open` 可以报扩展自己的 id） |
| [ADR-032](ADR-总览.html#adr-032) | 编译出来的 Context Pack 只留审计行，不建 `context_packs` 表 | accepted（2026-10-06，第五轮完成度清点：§50 最小表清单里唯一没有对应物的名字） |
| [ADR-033](ADR-总览.html#adr-033) | §52 的 L2 是"按 role 内容哈希"：去重层不许删证据 | accepted（2026-10-06，第五轮；migration v4 重建 `messages`，时间窗口留在 L3） |
| [ADR-034](ADR-总览.html#adr-034) | 一轮话的"话"就是它的 text 块：normalizer 跳过思考与工具机械，且**不拍平** | accepted（2026-10-07，第六轮；真会话 215 轮只进 22 轮暴露的读法缺陷） |
| [ADR-035](ADR-总览.html#adr-035) | 宿主二进制不能住在 macOS 受保护目录；坑是静默的，所以 `wp host manifest` 就此警告 | accepted（2026-10-07，第六轮；`~/Documents` 下的二进制被 Chrome 拉起时卡死在 dyld `open()`） |

缺号是刻意的：ADR-003、005、006、007、008、009、010、012 已由 `PROJECT_MASTER.md` §7 / §19
钉得够死，另开一条只会复述总控；其中 ADR-006（Tauri）已在第三轮建成，记在
[ADR-029](ADR-总览.html#adr-029)，ADR-007（浏览器扩展）的"尚未构建"由第四轮的
[ADR-031](ADR-总览.html#adr-031) 结掉。
编号不可回收，下一条从 **ADR-036** 起。

## 新增一条 ADR 的做法

1. 在 `ADR-总览.html` 的「五、全部决定原文」末尾追加一节
   `<h2 id="adr-034">ADR-034 — &lt;标题&gt;</h2>`，带上 Status / Date / Ref 三行 meta，
   再写 Context / Decision / Consequences（正文照上一节的标记风格来）。
2. 在本页索引表和该页「二、索引」表各加一行，并把该页头部的条数徽章与「六、来源与变更」一起改。
3. 决定被后续轮次改变时**就地修订**原条目，Status 标 `amended 日期（改了什么）`，
   不叠第二份说法——见 `docs/RECORDS.md` 2026-10-05 的介质变更记录。
4. `PROJECT_MASTER.md` §20 的 LAST COMPLETED 里引用 `(ADR-0NN)` 即可，不写路径。
