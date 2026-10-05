# PROJECT_MASTER — 个人 Vibe Coding 项目开发记录工作台总控

**文件定位：** 项目最高级总控 / 开发边界控制 / 产品地图 / 版本路线 / 跨模型交接入口  
**Master 版本：** V2.0（产品重新定位后）  
**当前产品阶段：** V1 — 开发前设计冻结，准备进入实现  
**最后更新：** 2026-10-02

---

# 0. 本文件的唯一职责

本文件不是具体开发说明书。

它负责长期回答：

1. 我们正在做什么产品？
2. 最初为什么做它？
3. 最终产品应该长什么样？
4. 哪些功能属于核心，哪些属于偏航？
5. 当前架构骨架是什么？
6. 为什么当前这样设计？
7. 未来可能如何升级？
8. 什么时候才应该升级？
9. 当前开发到了哪里？
10. 上一步完成了什么？
11. 下一步应该做什么？

任何模型、Agent 或开发者进入本项目时，应优先阅读本文件。

---

# 1. 产品重新定位

## 1.1 一句话定义

> **这是一个给个人 vibe coding 开发者使用的项目开发记录工作台。它持续记录项目为什么这样做、已经做到了哪里、发生过哪些关键决定、架构准备怎样演进，并在开发越来越复杂时帮助用户始终掌握完整项目脉络，避免项目逐渐脱离最初设计。**

---

## 1.2 核心问题

个人 vibe coding 的主要风险不是“不会写代码”，而是：

- 对话散落在 ChatGPT、Codex、Qoder、Hermes、DSH 等不同 Agent 中；
- 每个 Agent 只看到当前局部；
- 开发过程中不断新增功能；
- 临时方案逐渐替代原始方案；
- 早期架构设计原因被遗忘；
- 原本的边界被一点点突破；
- 几周后连开发者自己也难回答“为什么当时这么做”；
- 新模型进入项目后容易重新设计已经讨论过的问题；
- 项目看似越来越完整，实际却越来越偏离最初目标。

本产品首先解决：

> **Project Continuity + Design Fidelity**

即：

> 项目连续性 + 对最初设计意图的保持。

---

# 2. 产品北极星

产品永远围绕以下六个问题设计：

### 1. Intent
最初到底要做什么？

### 2. Boundary
明确不做什么？哪些边界不能无意识突破？

### 3. State
项目现在真实做到哪里？

### 4. Reason
为什么会变成现在这样？

### 5. Evolution
哪些未来升级是提前规划过的？什么条件下才应该启动？

### 6. Continuity
换模型、换 Agent、换窗口、隔几周回来后，能否继续从正确位置开发？

如果一个新功能不能明显改善以上六项之一，则不应优先进入产品核心。

---

# 3. 产品终点

成熟版本应让个人开发者获得以下体验：

1. 新建项目时记录最初产品目标、架构、边界和成功标准；
2. 将项目关联到本地代码目录；
3. 同步或导入 ChatGPT、Codex、Qoder、Hermes、DSH 等开发对话；
4. 自动识别重要任务、问题、决定、代码变化和架构变化；
5. 自动维护项目当前状态；
6. 自动维护项目时间线；
7. 随时知道“为什么项目现在是这样”；
8. 对重要架构决定保留原因和替代方案；
9. 对未来架构升级保留 AEP；
10. 当开发内容开始偏离最初设计时主动提示；
11. 选中任意问题时，自动准备完整项目上下文给目标 Agent；
12. Agent 完成工作后，结果重新回流项目记录；
13. 用户不用反复向新模型解释整个项目；
14. 项目越做越大时，整体脉络仍然越来越清楚，而不是越来越混乱。

---

# 4. 明确取消的方向

以下能力从产品路线中删除：

- 多人协作；
- 团队成员管理；
- Role / Permission；
- 团队任务分配；
- 企业工作区；
- 组织管理；
- 团队评论系统；
- 协作型云服务；
- 团队工时与绩效；
- 企业审批流；
- 面向组织的 Jira 替代。

本产品是：

> **Single-user first and single-user focused。**

未来即使加入云，也只优先解决：

- 个人多设备同步；
- 个人备份；
- 个人远程访问；

而不是团队协作。

---

# 5. 非目标

本产品也不准备成为：

- IDE；
- 代码编辑器；
- GitHub 替代；
- 全自动 Coding Agent；
- Jira / Linear 克隆；
- 大而全知识库；
- 通用第二大脑；
- 通用笔记软件；
- 企业项目管理平台。

它是：

> **围绕“一个人如何持续掌握一个 vibe coding 项目完整脉络”构建的工作台。**

---

# 6. 永久架构骨架

```text
┌──────────────────────────────────────────┐
│           Project Workbench              │
│ Overview / Map / Timeline / Drift / Plan │
└──────────────────────────────────────────┘
                    ↑ ↓
┌──────────────────────────────────────────┐
│            Context Engine                │
│ Retrieve / Budget / Compile / Route      │
└──────────────────────────────────────────┘
                    ↑ ↓
┌──────────────────────────────────────────┐
│             Project State                │
│ Charter / Scope / Decision / Task /      │
│ Issue / Artifact / Milestone / AEP       │
└──────────────────────────────────────────┘
                    ↑
┌──────────────────────────────────────────┐
│          Guard & State Engine            │
│ Judge / Resolve / Reduce / Drift Check   │
└──────────────────────────────────────────┘
                    ↑
┌──────────────────────────────────────────┐
│               Event Store                │
└──────────────────────────────────────────┘
                    ↑
┌──────────────────────────────────────────┐
│              Adapter Layer               │
│ Web / CLI / MCP / Git / Files / Import   │
└──────────────────────────────────────────┘
                    ↑
┌──────────────────────────────────────────┐
│ ChatGPT / Codex / Qoder / Hermes / DSH   │
└──────────────────────────────────────────┘
```

未来允许更换具体技术实现，但不轻易破坏这些逻辑层。

---

# 7. 长期架构原则

## 7.1 Local-first

本地项目状态、本地索引、原始记录、Decision、AEP、Project Map 默认保存在本地。

---

## 7.2 Event-first

重要变化先成为 Event，再形成当前状态：

```text
Source
  ↓
Event
  ↓
Reducer
  ↓
Project State
```

禁止让一次模型摘要直接覆盖项目事实。

---

## 7.3 Evidence-first

任何关键状态都应尽可能回答：

> 为什么系统认为是这样？

---

## 7.4 Human Override Wins

用户明确确认或锁定的事实具有最高优先级。

---

## 7.5 Repository / Port Boundary

Core 不直接绑定 SQLite、Cloud 或任何外部实现。

必须保持：

```text
Core
 ↓
Repository / Port
 ↓
Concrete Implementation
```

V1：

```text
Project Core
      ↓
Repository
      ↓
SQLite
```

未来如果需要个人云同步，可以演进为：

```text
Project Core
      ↓
Repository
      ↓
Local Repository
      ↓
SQLite
      ↕
Sync Engine
      ↕
Personal Cloud
```

Core 不因存储方式变化而重写。

---

## 7.6 Adapter-first

所有外部 Agent 与平台通过 Adapter 接入。

核心系统不得写死 ChatGPT、Codex 或任何单一平台。

---

## 7.7 Context is Compiled

回弹给 Agent 的上下文必须根据当前问题动态编译，不是简单复制聊天历史。

---

## 7.8 Prepare Before Send

默认：

```text
Build
→ Preview
→ Open Target
→ Prepare Input
→ User Confirm
→ Send
```

---

## 7.9 Architecture-ready, not Feature-heavy

V1 可以为未来留接口，但不能因为“以后可能需要”而提前实现大量复杂功能。

例如：

应该现在存在：

```text
Repository interface
schema_version
AEP
origin metadata
```

不应该现在开发：

```text
复杂云同步
分布式一致性
企业账号系统
```

---

# 8. 防偏航核心模型

重新定位后，以下对象是产品真正核心。

## 8.1 Project Charter

记录项目最初设计。

至少包括：

```text
Original Problem
Target User
Core Value
Must Have
Won't Have
Success Criteria
Architecture Principles
Product Constraints
```

Charter 默认不被 AI 自动修改。

---

## 8.2 Scope Boundary

明确：

```text
In Scope
Out of Scope
Deferred
Forbidden / Must Not
```

任何新增能力都可以与 Scope 对照。

---

## 8.3 Decision

记录：

```text
What
Why
Alternatives
Evidence
Status
```

---

## 8.4 Change Proposal

当项目要发生明显方向变化时，不直接修改基线。

先形成：

```text
Change Proposal

Proposed Change
Reason
Affected Scope
Affected Architecture
Benefit
Cost
Risk
Does it conflict with Charter?
Decision
```

---

## 8.5 AEP — Architecture Evolution Plan

记录：

```text
Current
Limitation
Why Not Now
Trigger
Target
Change Surface
Preserved Core
Benefit / Cost
Status
```

用于保存未来架构升级意图。

---

## 8.6 Drift Alert

系统检测：

```text
Scope Drift
Architecture Drift
Decision Conflict
Feature Creep
Repeated Redesign
Stale Assumption
Unplanned Dependency
```

提醒用户：

> 当前工作可能已经偏离原始设计。

Drift Alert 是提示，不自动阻止开发。

---

# 9. 项目状态对象

除防偏航对象外，维持：

```text
Task
Issue
Artifact
Milestone
Conversation
Message
Event
Evidence
Snapshot
Context Pack
```

---

# 10. PROJECT_MAP 的定位

`PROJECT_MAP.md` 是：

> **项目当前状态与脉络的可读投影。**

它回答：

```text
现在项目是什么？
当前阶段是什么？
做完了什么？
正在做什么？
为什么这样做？
有什么问题？
最近发生了什么？
下一步是什么？
有没有偏离最初设计？
```

它不是数据库。

---

# 11. PROJECT_MASTER、PROJECT_CHARTER、PROJECT_MAP、ADR、AEP 的区别

## PROJECT_MASTER

管理这个软件本身：

> 我们正在开发什么产品，版本路线是什么，当前开发在哪。

## PROJECT_CHARTER

针对一个用户项目：

> 这个项目最初为什么存在，什么必须做，什么不做。

## PROJECT_MAP

针对一个用户项目：

> 当前实际开发到哪，项目现在长什么样。

## ADR

> 已经正式做出的架构决定及原因。

介质（2026-10-05 起）：全部 ADR 汇总在 **`docs/adr/ADR-总览.html`** 一个页面里，
逐条带锚点（`#adr-0NN`），页内同时挂每一轮的任务报告链接。原 `docs/adr/ADR-0NN-*.md`
已删除，决定措辞未改；新增决定直接在该页追加一节，编号继续从 ADR-029 排（ADR-028 已于 2026-10-06
页内追加，记的是 §38.5 入站 MCP 采集还欠着），不再单独建 .md。
`docs/adr/README.md` 只留索引与追加规则。总控正文引用决定时写 `(ADR-0NN)`，不写路径。

## AEP

> 已设计但尚未启动的架构演进路线及触发条件。

---

# 12. 产品版本路线

整个产品只分两次大型迭代：

```text
V1
Project Continuity Core
        ↓
V2
Project Intelligence Workbench
```

不再继续拆成 V3/V4 大版本。

后续优化在 V2.x 内持续进行。

---

# 13. V1 — Project Continuity Core

## 13.1 V1 目标

首先做到：

> **无论开发对话分散在哪，用户都能持续掌握项目主线，并把正确上下文交给下一个 Agent。**

核心闭环：

```text
Project Charter
      ↓
Chat / Files / Git / Agent
      ↓
Event Store
      ↓
Project State
      ↓
Decision / AEP / Scope
      ↓
PROJECT_MAP
      ↓
Drift Check
      ↓
Context Pack
      ↓
Any Agent
```

---

## 13.2 V1 必须完成

### 项目建立

- Create Project
- Bind Repository / Folder
- Project Charter
- Scope Boundary
- Initial Architecture Baseline

### 记录层

- Source
- Conversation
- Message
- Event Store
- Evidence
- Timeline

### 项目状态

- Task
- Issue
- Decision
- Artifact
- Milestone
- Change Proposal
- AEP

### 防偏航

- Scope Comparison
- Decision Conflict
- Basic Drift Alert
- User Lock
- Change Review

### Project Map

- 自动编译
- Snapshot
- Diff

### 输入

- 手动导入
- Browser Extension 基础采集
- 至少一个 Local Agent Adapter
- Git / File 基础监听

### Context

- Selected Context
- Retrieval
- Context Pack
- Preview
- Agent Select
- Open / Inject / Copy Fallback

### UI

- Overview
- Project Map
- Timeline
- Decisions
- Evolution
- Drift / Inbox
- Search
- Context Panel

### 可靠性

- Incremental Sync
- Dedup
- Audit
- Adapter Health
- Migration
- Backup / Export

---

# 14. V1 完成判定

V1 只有满足以下完整场景才算完成：

1. 用户创建项目并填写 Charter；
2. 用户绑定代码目录；
3. 系统获取至少两个不同来源的开发记录；
4. 系统形成 Task / Decision / Issue / Artifact；
5. 系统自动生成 PROJECT_MAP；
6. 系统能指出至少一种潜在偏航或冲突；
7. 用户点击某个 Issue；
8. 系统生成包含 Charter、Decision、相关文件和最近讨论的 Context Pack；
9. 成功准备到另一个 Agent；
10. Agent 的后续结果能够重新进入项目时间线。

---

# 15. V2 — Project Intelligence Workbench

V2 不再扩展团队能力。

目标是：

> **把 V1 的“项目连续性”升级为“个人开发驾驶舱”。**

---

## 15.1 V2 核心能力

### 更强的 Drift Intelligence

- Feature Creep Detection
- Architecture Drift
- Scope Drift
- Decision Contradiction
- Forgotten Constraint
- Duplicate Solution
- Repeated Redesign
- Stale Assumption

### Architecture Intelligence

- Architecture Map
- Module Relationship
- Architecture Diff
- Decision → Code Impact
- AEP Trigger Watch

### Development Storyline

- 自动生成阶段故事线
- “为什么走到这里”
- 关键转折点
- 重大设计变化
- 当前风险

### Advanced Retrieval

- Hybrid Search
- Semantic Retrieval
- Entity Graph
- Code / Conversation Cross-link

### Personal Planning Views

这些视图只是同一 Project State 的不同呈现：

- Lightweight Kanban
- Roadmap
- Gantt-lite
- Dependency View
- Milestone View

不开发复杂企业排期系统。

### Branch / Experiment Tracking

- Experiment
- Prototype
- Abandoned Path
- Why Abandoned
- Compare Alternatives

### Personal Automation

- Automatic Snapshot
- Daily Project Brief
- Weekly Project Review
- Stale Task Reminder
- AEP Trigger Reminder
- Drift Watch

### Personal Cloud（可选）

只有触发条件成立后才进入 V2.x：

- 多设备成为高频需求；
- 本地备份不足；
- 用户需要远程访问。

目标仅限：

- 个人备份；
- 个人多设备同步；
- 个人恢复。

不引入团队协作。

---

# 16. V2 明确不做

即使进入 V2，也不做：

- Team Workspace
- Role
- Permission
- Assignment to People
- Organization
- Enterprise Admin
- Team Chat
- Team Collaboration Server
- Enterprise SSO

---

# 17. 当前技术基线

```text
Desktop:
Tauri 2

Frontend:
React + TypeScript

Core:
Rust

Storage:
SQLite

Database Mode:
WAL

Search:
FTS5

Browser:
Chrome / Chromium MV3 Extension

Browser ↔ Local:
Native Messaging

Agent Integration:
Adapter SDK + MCP

Architecture:
Event Store
+ Materialized Project State
+ Evidence
+ Repository / Port
+ Context Compiler
```

---

# 18. Architecture Evolution Registry

总控只保存索引。

详细演进计划进入：

```text
docs/evolution/
```

当前建议建立：

```text
AEP-001 Local-only → Personal Local + Cloud Sync
AEP-002 FTS5 → Hybrid Semantic Retrieval
AEP-003 Built-in Adapter → Plugin Adapter Runtime
AEP-004 Basic Drift Rules → Intelligent Drift Engine
AEP-005 Flat State → Entity / Architecture Graph
```

状态：

```text
Dormant
Watching
Triggered
In Progress
Completed
Superseded
```

---

# 19. 开发边界

以下行为默认视为偏航。

## 禁止 1
重新加入多人协作、团队权限或企业能力。

## 禁止 2
把产品做成普通 Todo / Kanban 工具。

## 禁止 3
让项目管理功能与 Context / Charter / Decision 脱节。

## 禁止 4
维护两套 Project State。

## 禁止 5
让 LLM 自由重写 Charter。

## 禁止 6
让模型一次摘要直接覆盖项目事实。

## 禁止 7
核心直接依赖 SQLite、Supabase 或具体 Agent。

## 禁止 8
为了未来可能性提前实现大量未触发功能。

## 禁止 9
把聊天记录当成最终项目记忆。

## 禁止 10
新增重大功能却没有说明它与原始目标的关系。

---

# 20. 当前项目位置

```text
PRODUCT:
Personal Vibe Coding Project Workbench

CURRENT MASTER:
V2.0

CURRENT LARGE ITERATION:
V1

CURRENT DEVELOPMENT STATE:
V1 Core Loop Implemented / CLI Workbench Runnable / Hardening Done / MCP Read Surface Shipped /
Desktop Shell Shipped (ADR-029) / Browser Extension + Native Messaging Owed /
MCP Inbound Capture Owed (ADR-028)

LAST COMPLETED:
- Rust monorepo：`crates/domain|core|storage|state-engine|guard-engine|context-engine|ingestion|security|search|mcp-server` + `apps/cli`
- Repository Ports：十一个 trait 收在 `vibe_core::ports::Repositories`；rusqlite 只存在于 `crates/storage`（INV-001 由 drift 规则与 Cargo 依赖扫描两处验证，后者已是测试）
- SQLite Schema v1→v3 + Migration + FTS5 + WAL；Event Store 以 `seq` 确定性重放，`dedup_key` / `content_hash` 保证采集幂等
- Charter / Scope / Architecture Baseline：版本化、仅用户可写，非用户写入得到 `CoreError::Locked`；变更走 Change Proposal
- Reducer + Materialized State + PROJECT_MAP Compiler（§31 的 13 个 section，空 section 自己声明为空）+ Snapshot / Diff
- Guard：六条词法 drift 规则 + Inbox + §27 State Judge（阈值 0.90 / 0.65，强标记 +0.12）+ 实体解析 + §7.4 用户确认即锁定
- Context Engine：§40 三层检索、§42 选取阶梯、§43 分 section 预算、§53 Policy Filter、§54 Audit、§44 路由建议、`prepare` 复制兜底（§7.8：发送永远是人的动作）
- Ingestion：Adapter SDK + Normalizer + Dedup + 手动/粘贴/transcript/CLI-session 采集 + `git log` 只读采集 + 文件树扫描
- §53 Secrets：值不进 SQLite 明文，库内只有 `secret_ref`；Keychain 需显式环境变量同意，否则落到 0600 文件库
- §55 Export **与 Backup**：六个导出 + `.projectpack`；`wp backup create|status|verify|restore` 走 SQLite 在线备份 API，`PRAGMA integrity_check` 出结论，restore 需 `--yes` 并先快照当前库（ADR-023）
- §17 / §45 **证据腿可读**：`wp evidence <kind:id>` 打开一条记录引用的证据行——端口与 fold 早就在写它，缺的只是入口
- Stage H **Adapter Health**：`AdapterRepository` + 四种结局（ok / unavailable / degraded / failed）+ `wp adapters`；`unavailable ≠ failed`（ADR-021）
- §39 **监听**：`wp watch` 一次 pass 相减出分支、脏集合与文件出现/改动/消失；新增 `GIT_BRANCH_CHANGED` / `GIT_STATUS_CHANGED`；监听信号不折进 Project State（ADR-022）
- §52 **L3 近似重复**：`wp duplicates` 只出提示、不合并、不删除；词法 Jaccard + CJK 字符 bigram；读取窗口补 `rowid DESC` 保证确定性（ADR-024）
- §58 **Adapter Contract**：七个动词齐（含 `inject` / `fallback`），`fallback` 是唯一不能不答的动词；契约测试对每个 adapter 跑同一份 review 并带负控（ADR-025）
- §38.5 **MCP 基础接口（出向那一半）**：`crates/mcp-server` + `wp-mcp`，stdio JSON-RPC 2.0，六个只读工具全部经现有端口与 service，`context_pack` 过同一个 Policy Filter 并留 §54 audit（ADR-026）
- §38.5 **MCP 基础接口（入向那一半）＝还欠着**：工作台主动去读 agent 那边的会话目前没有实现——`SourceType::Mcp` 与 `"mcp"` 线串已在词汇表里，`wp import transcript … --source-type mcp` 也收文件，但工作区里没有任何代码向外开 MCP 连接。触发条件、`McpClientAdapter` 七个动词怎么映射、以及"必须 stdio 阻塞、不许引 async runtime"这条由 §60 决定的约束，全部记在 **ADR-028**；§26 的 V1-D 行因此从 ✅ 改回 ◐
- §58 **prepare 改问 `fallback()`**：`vibe_ingestion::fallback_for()` 成为那句兜底话的唯一出处，trait 默认实现与 `wp context prepare` 都走它，`--json` 多带 `fallback` 对象，人类输出点名 `--out` 写出的那份 payload（ADR-025 就地修订）
- §38 **V1-E 桌面壳**：`apps/desktop`（Tauri 2）。十二个命令里十一个是只读视图，唯一的写是 `apply_pending`，
  而它逐条走 §7.4 早就存在的 `accept_change_proposal` / Inbox 判定；壳不建项目也不替你猜项目——db 路径与
  当前项目都由 `vibe_cli::app::App`（CLI 自己的 runtime）给出，所以窗口和终端不可能各有一套项目状态
  （ADR-029，禁止 4 / 禁止 7）。lib/bin 切开：视图构造器在 lib，`#[tauri::command]` 壳在 bin，
  于是 `apps/desktop/tests/views.rs`（12 条）不启动 webview 就能驱动整条读路径（ADR-020 就地修订）。
  §21 给 V1-E 列的八个屏幕——Overview / Timeline / Project Map / Decisions / Evolution / Drift·Inbox /
  Search，加上本轮定的第一屏 diff——全部有对应命令与视图构造器
- 桌面壳第一屏就是用户拍定的那条流程：**变动（diff + 等你决定的条目）→ 决定哪些生效 → 项目地图**，
  再往两侧摊开总览、决策、演进（AEP）、时间线、检索、上下文预览、健康与备份。待确认条目每行带自己能做的动词
  （`actions`）而不是一个含义不明的"同意"勾选；被拒绝的行点名退回原因；没有 adapter 能注入时，
  复制粘贴作为整条路径显示出来，而不是一枚看起来像发送的按钮（§7.8、ADR-025）
- **§14 第 7 项"用户点击某个 Issue"补上了那只手**：偏航屏与检索屏的每一行有「带它去上下文 →」，上下文屏多一个
  `kind:id` 聚焦框；点了之后不需要再写一句问话——**引用本身就是意图**，桌面壳按 CLI 同样的方式把它写成
  `context for issue:I-1`，并且复用 `vibe_cli::context::parse_ref`（同一套引用语法，不另写第二套）。
  包会显式说自己聚焦在哪一条（`focus`），没问话也没点选时是**拒绝**，不是一份空包
  （测试 `a_click_on_a_record_focuses_the_pack_without_a_sentence`）
- 同一件事在 CLI 与 MCP 读面上各暴露出 ADR-030 的第二种后果：§42.1 的"用户显式选择"是经**物化出来的实体行**
  解析的，而那些行是折叠的缓存。所以刚 `wp event` 追加一条 Issue、还没跑过 `wp map` 时，
  `wp context plan --select issue:I-1` 会答"not found in this project"。修法是编译包之前先做一次
  `project_state` 写穿（`wp map` 与快照本来就这么做，不是第二套状态），CLI `context dispatch` 与
  `wp-mcp` 的 `context_pack` 各一行；CLI 侧新增 `a_selected_record_travels_at_the_top_of_the_ladder`
  把 `--select` 的插队位置（priority 1 / kept）与"少了 kind 就拒绝"钉住——这个 flag 之前只有引擎层测试覆盖
- **真缺陷修复（ADR-030）**：`CHANGE_PROPOSAL_CREATED` 的 payload 原先只带 `proposalId` + `proposedChange`，
  而 `Projector` 从**空状态**折叠、`Reducer::project_state` 会把重建出来的行写回缓存——于是 `reason` 与
  `delta` 在物化时被抹掉，§7.4 的"确认即锁定"勾完什么也没动，却回报已经确认。修在源头：**创建事件的
  payload 就是那条记录**；回归测试 `a_proposal_survives_materialization_with_the_delta_it_was_opened_with`
  放在 state-engine 自己的套件里（没有窗口的机器上同样会红）。guard-engine 那条老测试之所以一直没发现，
  是因为它 accept 的是本次调用刚返回的内存对象，从没让折叠重建过它
- `wp diff --snapshot <id>` 的参数含义有了一张 CLI 测试守着：它是**基线**，不是项目
  （`a_snapshot_argument_is_the_baseline_and_never_the_project`）；不存在的基线是拒绝，不是"跟空比"
- ADR 介质变更：19 份 `docs/adr/ADR-*.md` 汇总为单页 `docs/adr/ADR-总览.html`（措辞未改，逐行核验 669 行内容无缺失、19 锚点、35 处交叉链接、0 断链），此后 ADR 直接在该页追加，任务报告也在该页「四、任务报告」挂链
- §59 / §60 **预算**：纯折叠 ≤250 ms（实测 ~40 ms）、整条链 ≤20 s 的 hang/O(n²) tripwire（实测 ~6 s）；其余是工作量断言；成本规则由 12 个 manifest 的依赖扫描钉住（ADR-027，2026-10-06 起把 `apps/desktop` 也算进去——GUI 想开一个网络从此是一条失败测试而不是 review 意见）
- 测试：`cargo test --workspace` = 265 passed / 0 failed / 1 ignored（Keychain 需 opt-in），构建零 warning。
  三层：库（golden + 各 crate 套件）、真二进制（`cli_e2e` 19 条）、桌面壳视图（`apps/desktop/tests/views.rs` 12 条）

CURRENT BLOCKER:
None. V1-E shipped in the round after it was queued, and it found a defect in the event chain rather
than in itself — ADR-030 is that record. The next round (V1-D browser extension + Native Messaging)
needs a browser session and a host registration, which is the first time `inject` / `open` get a real
implementation instead of the `Ok(None)` default; nothing in the core blocks it, and §35's five
validations (origin / command / payload / size / project scope) are the round's own scope.

NEXT IMMEDIATE TARGET:
V1-D Capture 的剩下两件（§34 浏览器扩展 MV3 + §35 Native Messaging host，`apps/extension` 新 workspace
成员）。这一轮才是 `AgentAdapter::open` / `inject` 第一次真实存在的时候：§37 的能力模型只要求读到
"能注入"这个声明，§38 要求 Generic Web 与 ChatGPT Web 两个面都能收；入站方向用现成的
`IngestPipeline::ingest_messages`，即聊天内容落地成 Event 与证据，**不直接写面板、map 或 Project State**
（禁止 4 / 禁止 9、ADR-022、ADR-028 同一套说法）。§35 的校验是这一轮的验收点，不是可选项。
还挂在账上的一件：ADR-028 的入站 MCP 采集（工作台主动去读 agent 那边的会话）。§14 第 7 项"点击某个 Issue"
本轮已经有了：偏航屏与检索屏的行可以直接带进上下文屏，`--select` 在 CLI 也第一次有了自己的测试。
§14 判定：第 1、4、5、7、8、9、10 项在核心、CLI、MCP 读面与桌面壳上都已具备；第 2、3、6 项可从 CLI 走到；
"发送进另一个 Agent"仍缺真实传输（V1-D 这一轮补）。

完整阶段记录：docs/RECORDS.md
```

---

# 21. V1 内部推进阶段

## V1-A Foundation

- Monorepo
- Tauri
- Rust Core
- SQLite
- Migration
- Repository Ports
- Event Store

## V1-B Project Baseline

- Project
- Charter
- Scope
- Architecture Baseline
- User Lock

## V1-C State Engine

- Task
- Issue
- Decision
- Artifact
- Milestone
- Change Proposal
- AEP
- Evidence
- Reducer

## V1-D Capture

- Manual Import
- Browser Extension
- Native Messaging
- Conversation Normalizer
- Git / File Watcher
- Adapter SDK

## V1-E Workbench

- Overview
- Timeline
- Project Map
- Decisions
- Evolution
- Drift / Inbox
- Search

## V1-F Context Engine

- Retrieval
- Budget
- Compile
- Preview
- Route
- Agent Prepare

## V1-G Guard

- Scope Check
- Decision Conflict
- Basic Drift Alert
- Change Review

## V1-H Hardening

- Backup
- Export
- Audit
- Adapter Health
- Golden Test
- E2E
- Performance

---

# 22. 每次模型接手时必须读取

顺序：

```text
1. PROJECT_MASTER.md
2. CURRENT PROJECT POINTER
3. 当前版本开发文档
4. 相关 ADR / AEP
5. Git / Tests / Actual Code
6. 当前 PROJECT_MAP
7. 再开始工作
```

---

# 23. 每次重要开发结束必须留下

```text
LAST COMPLETED
FILES CHANGED
TEST STATUS
DECISIONS MADE
AEP IMPACT
KNOWN ISSUES
DRIFT CHECK
NEXT IMMEDIATE TARGET
```

---

# 24. 总控更新原则

本文件只保存长期方向与当前指针。

不要写入每天的开发流水。

每次明显阶段结束，只更新：

```text
CURRENT DEVELOPMENT STATE
LAST COMPLETED
CURRENT BLOCKER
NEXT IMMEDIATE TARGET
```

重大方向改变必须同时：

- 更新本文件；
- 写 ADR；
- 如涉及未来演进，更新 AEP。

---

# 25. 最终产品验收图景

成熟产品应让用户可以在任何时刻打开一个项目，立即看到：

```text
最初想做什么
明确不做什么
当前做到了哪里
最近发生了什么
为什么做出这些决定
哪些设计发生了变化
哪些变化是计划内演进
哪些变化可能已经偏航
下一步最合理的开发位置
```

然后用户选择任意问题：

```text
→ 系统自动准备项目上下文
→ 发给任意 Agent
→ 继续开发
→ 结果回流项目记录
```

最终体验：

> **Vibe coding 可以很快，但项目不能失忆。**

---

# 26. 当前执行指令

从现在开始：

> **停止继续扩展产品概念，进入 V1 实现。**

第一步：

```text
建立正式仓库结构
→ Repository Port
→ SQLite Schema
→ Migration
→ Event Store
→ Project Charter
→ Minimal Project State
→ PROJECT_MAP Compiler
```

这一步已经完成（§20 LAST COMPLETED）。执行指令不变，只是位置前进：

```text
V1-A Foundation ✅
V1-B Project Baseline ✅
V1-C State Engine ✅
V1-D Capture ◐（核心 + CLI + §38.5 的出向 MCP 只读接口已做完；
              还欠两半：浏览器扩展与 Native Messaging，以及**入站** MCP 采集——
              工作台主动去读 agent 那边的会话。触发条件与届时形状记在 ADR-028，
              上一版把这行整体标成 ✅ 是把 §38.5 只读了一种）
V1-E Workbench ✅（`apps/desktop`：Tauri 2 只做已编译视图的展示，第一屏 diff →
               用户挑哪些变动生效 → map。§21 列的八个屏幕全部有对应命令与视图构造器；
               webview 本身的渲染仍是手工核对，写在 ADR-029 的后果里）
V1-F Context Engine ✅
V1-G Guard ✅
V1-H Hardening ✅（Export / Backup+Restore / Audit / Adapter Health / 监听 / 性能预算 /
Golden Test / CLI E2E；本轮补掉的正是上一版标"未做"的 Backup 与性能预算）
```

仍然有效的边界：**界面是核心逻辑的展示层，不是第二套逻辑。**
任何一轮如果让 Tauri 或扩展自己解释事件、自己拼状态，就是在违反 禁止 4 与 禁止 7。

V1 完成后进入 V2：

> **增强项目智能，而不是增加团队管理。**
