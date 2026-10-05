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
Desktop Shell Shipped (ADR-029) / Browser Host + MV3 Extension Shipped (ADR-031) /
V1 Re-audited Item-by-Item (ADR-032, ADR-033) / MCP Inbound Capture Owed (ADR-028)

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
- §34/§35 **浏览器门**：`crates/ingestion/src/native_messaging.rs`（帧编解码 + `HostPolicy` + `NativeHost` +
  `HostManifest`）与 `apps/cli/src/host.rs`（`wp host serve` / `wp host manifest`）。四个动词
  `hello` / `capture` / `open` / `inject`，一问一答、不监听、不开端口；`open` 与 `inject` 答的是**给扩展的指令**
  （`{action:"openTab"}` / `{action:"fillComposer",text,send:false}`），于是 §7.8 写在协议里而不是写成一个可商量的默认值。
  §35 的五项检查各占一个拒绝前缀（`origin` / `command` / `payload` / `size` / `project scope rejected`），
  一条坏帧只结束那一帧（§62：静默才是失败）。浏览器推来的线程走 `IngestPipeline::ingest_messages`——
  不是第八个适配器：适配器是**读者**，活的会话在浏览器那一侧是被推的一方（ADR-031）
- §34 的**扩展半边**：`apps/extension/`（MV3，零依赖、零构建步骤，**不是** workspace 成员，所以 §60 的
  12 份 manifest 巡检不必改写）。`background.js` 在 native messaging **之前**做完 §35 要求的校验
  （协议版本、origin 白名单、已知 command、payload 形状含未知字段拒绝、245760 字节硬上限），
  `content.js` 只读页面与写输入框、永不提交（不派发 Enter、不 `requestSubmit`、不点发送键），
  `popup.js` 的状态区逐字显示宿主的拒绝句子。generic web 的"用户授权"是弹窗那次点击签发的一次性同意凭据
  （60 秒、用过即废、只存在 worker 内存里）
- 两边对接时改掉的两处：§35 的 origin 检查有**两副面孔**（`capture` / `inject` 必须报页面 origin；
  `hello` / `open` 不经手页面材料，可以报扩展自己的 `chrome-extension://<id>`——那枚 id 正是 Chrome 按
  `allowed_origins` 已经核过的东西），以及扩展把 `capture` 的 `{action:"captured"}` 误报成"不认的动作"。
  两处都就地修订进 ADR-031，`apps/extension/README.md` 的协议表同轮改写（那条维护费是 ADR-031 自己写的）
- §58 的 E2E 那一行第一次真的跑通：`cli_e2e` 第 20 条 `a_browser_session_round_trips_through_the_native_host`
  用**自己重写的一份编解码**驱动真二进制（不复用 crate 的 codec——共享 helper 会替宿主隐瞒它自己发出去的字节），
  走完 Browser → Native → Event → State → Map → Pack → open / inject，并在同一轮里撞上 §53：fixture 项目
  `allowCloudModel=false`，所以 `--to chatgpt` 的包被**整包拒绝**，链路上用的是"不声明去处"的那份包
- **第五轮：不按前四轮的自报验收，把 §21 八段 / §14 十条 / §62 七道门逐条回代码重查**。结果分三类——
  真缺的补、已做却没有测试声称过的补测试、账上写过头的**改账**。查出并修掉一个会吃证据的真缺陷
  （下一条），补掉两个"有命令不等于有屏幕"的空壳屏（再下一条），其余四处是文档说过头（见 ADR-013/016/020/022 的
  就地修订）。**本轮不新增任何 V1 之外的功能**：§21 的条目之外一件都没做，禁止 8 同样管着"补开发"这一类轮次
- §52 **L2 的键少了 role（ADR-033，migration v4）**：`(conversation_id, content_hash)` 让"用户说继续、
  助手也答继续"吞掉第二条，同一句话隔一小时再说一遍也吞——去重层在做它不该做的事：**删证据**。
  role 进键（`idx_messages_l2`），**时间窗仍然不进键**：§51 要求重推幂等，而来源时间戳可能缺失或抖动，
  "同一句话后来又说了一遍"归 L3 提示、提示永不删除（ADR-024）。SQLite 不肯 drop 内联 UNIQUE 背后的自动索引，
  所以与 ADR-013 的 v3 同骨架：重建表而不是改表。第一轮那条把缺陷钉成需求的断言同轮改掉
- §50 **`context_packs` 表确实不存在，而这是一个决定（ADR-032）**：包按需编译，编译一次留一行 `audit_logs`
  （`produced_ref` / `read_refs` / `redacted_refs` / 目标 agent），成品以文件出去（`wp context build --out`、
  `.projectpack`）。再存一张表等于把项目事实抄第二份（禁止 4）；重看的触发条件写在 ADR-032 里，
  届时的形状是 `evidence` 里一份内容地址化的工件，不是新的一列
- **§46 总览与 §47 演进两屏这次真的有内容**：上一轮账上写"八个屏幕全部有对应命令与视图构造器"，
  核对下来那两个构造器是空壳——§46 要求的六个问题没人回答，§47 要求的五项里只有 AEP 有数据。
  现在总览逐问作答（意图 / 阶段 / 最近三条 / 最大问题 / 有无偏航 / 下一步取自编译好的地图），
  **答不出的那一问由 `gaps` 自己说出来**（ADR-011 的老规矩）， kinds 只做计数不做重排；
  演进屏给出架构基线与按状态分组的 AEP（触发 / 完成 / 休眠）。**ADR 那一行不建表**：它是
  `docs/adr/` 里的文档，屏幕指过去并在 `gaps` 里说明——为它新建持久生产者就是第二个状态来源（禁止 4，
  与 ADR-032 同一套理由）
- **四处"已做但没被测试声称过"补上声称**：milestone 的 planned→reached（且 reached 不许顺手改名）、
  §28 解析阶梯本身（1.0 / 0.95 / 0.70 与 0.90 的自动合并阈值，新建 `crates/ingestion/tests/resolution.rs`）、
  §14 第 10 项"结果回到时间线"（`a_second_agent_turn_re_enters_the_timeline`：真推一批**新**材料，
  断言 seq 越过旧的高水位、时间线/地图/diff/重新编译的包都动了——此前所有 import 测试证明的都是反面"重推不新增"）、
  §39 的 `file_deleted`（真目录里真删一个文件：一个 `ARTIFACT_DELETED`、行被标记而非删除、第三遍安静、
  不产生任何结论）
- **改掉的账（三处写过头）**：ADR-022 的 `payload.change` 从不发 `deleted`（删除走 `ARTIFACT_DELETED`）、
  引用的 `FileScan::diff` 其实是 `FileScan::run`、"这些事件一律不折叠"只对 git 那两个信号成立——
  文件信号确实写 artifact 行而 artifacts 在 `state_hash_of` 里，**所以该条标题从"move no state"改成
  "produce no verdict"**：监听信号不许触碰的是结论，不是文件行。另：ADR-013 引用 §52 键的句子随 ADR-033 改写，
  ADR-016 记下 §29 七种 Inbox 里两种**按设计**没有持久生产者（merge candidate 属于那一次 ingest 运行，
  unverified claim 在地图里说话），给它们建表需要 §16 没列的事件类型，禁止 8 不许无触发地先建
- `crates/core/src/ports.rs` 的 `insert_messages` 契约文档同轮改为"content hash **per role**"——
  端口注释也是契约的一部分，它比实现慢一拍就会误导下一个实现者
- 测试：`cargo test --workspace` = 300 passed / 0 failed / 1 ignored（Keychain 需 opt-in），构建零 warning。
  四层：库（golden + 各 crate 套件，`native_messaging` 13 条、新建 `resolution` 3 条、`map_and_snapshot` 10 条、
  `integration` 15 条）、真二进制（`cli_e2e` 22 条）、桌面壳视图（`apps/desktop/tests/views.rs` **23 条**，
  比上一轮翻倍：§46 六问与 §47 五段各自有断言，末一条 `the_new_answers_reach_the_webview_under_the_keys_it_reads`
  把 Rust 载荷的键与 `app.js` 读的键逐个对上——V1-D 对接时找出的两处不一致就是这类断言该拦住的东西）、
  扩展侧只有 `node --check`——V1 不为它装 JS 测试框架（§60 的成本规则），协议的断言全部在 Rust 那一侧
- 本轮收尾两处：上一轮记的 **299 / views 22** 是桌面壳 agent 最后一批测试落地**之前**的数字，
  按跑完的那一次改定为 **300 / 23**（第 23 条是载荷键与 `app.js` 读取键的对照，不是新增断言主题）；
  第四、五轮两次未提交的改动合成一个提交 `bd88311`，连同第三轮 `b80669a` 一起推到私有仓库，
  工作树干净、`main` 与 `origin/main` 一致。

CURRENT BLOCKER:
None. V1-D 的那扇门这一轮建完了，两侧是对着写的：Rust 宿主 + MV3 扩展，对接时找出的两处不一致已就地
修订进 ADR-031。剩下的唯一 V1 欠项是 ADR-028 的入站 MCP 采集——它不缺代码位置，缺的是一个真的、值得
工作台主动去读的 agent 会话源；§13.2 没要求 V1 去连第三方进程，禁止 8 也不允许在没有触发条件时先装一个
async runtime。仍未由机器保证的只有浏览器那一侧的**渲染**：扩展没有装进真 Chrome 跑过（本轮验证到的范围是
`node --check` 过三份 JS、`popup.html` 的 12 个 id 与 `popup.js` 逐个对上、`wp host manifest` 落出的记录与
wrapper 逐字看过、以及真 `wp host serve` 收到一帧 `chrome-extension://` 的 `hello` 并回了能力清单）。
MV3 的页面结构识别、输入框定位与 §7.8 的"填而不发"要在人的浏览器里再看一次——这与 ADR-029 给桌面壳留的
那条（webview 渲染仍是手工核对）是同一类残留，不是新的缺口。

NEXT IMMEDIATE TARGET:
V1 只剩一件挂在账上的事：**ADR-028 的入站 MCP 采集**——工作台主动去读 agent 那边的会话，读回来的仍然只是
证据，落地成 Event 再由 fold 更新面板（禁止 4 / 禁止 9，与这一轮的浏览器推送同一套说法）。它的触发条件、
`McpClientAdapter` 七个动词的映射，以及"必须 stdio 阻塞、不许为它引 async runtime"这条由 §60 决定的约束，
都写在 ADR-028 里；除它之外 §7.1 的 V1-A…V1-H 八段没有一个还留着没写的代码。
另一件不需要新代码、但下一轮该做的：**在真 Chrome 里装一次这个扩展**，把 §二 那两行 `mkdir` / `cp` 走到底，
看 `capture` 在一个真的 ChatGPT / Claude 会话上采到几轮、`inject` 把包写进了哪个输入框。桌面壳留下的那类残留
（ADR-029：webview 渲染手工核对）在扩展这一侧一模一样。
§14 判定：第 1、4、5、7、8、9、10 项在核心、CLI、MCP 读面与桌面壳上都已具备；第 2、3、6 项可从 CLI 走到；
"发送进另一个 Agent"这一轮有了真实的**输送**通道（`open` / `inject` 到浏览器，永远 `send:false`），
而 `AgentAdapter::inject()` 对已交付适配器仍返回 `Ok(None)`——ADR-031 把这一次记成结论而不是缺口。

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
V1-D Capture ◐（核心 + CLI + §38.5 的出向 MCP 只读接口 + §34/§35 的浏览器门（`wp host serve`
              与 `apps/extension` 的 MV3 扩展，ADR-031）已做完；还欠的是**入站** MCP 采集——
              工作台主动去读 agent 那边的会话。触发条件与届时形状记在 ADR-028。
              上一版把这行整体标成 ✅ 是把 §38.5 只读了一种；这一轮之后，欠的那半换了名字，
              不再是"扩展没建"。
              第五轮在这段里修掉一个会吃证据的缺陷：§52 的 L2 键原来只有 content hash，
              于是"用户说继续 / 助手也答继续"会被吞掉一条——role 现在进键（ADR-033，migration v4，
              重建表），时间窗仍然不进键（§51 的幂等靠身份而不是靠时间戳，"同一句话后来又说一遍"
              归 L3 提示）。同轮补上 §39 的删除信号测试：真目录里真删一个文件，
              一个 `ARTIFACT_DELETED`、行被标记而非删除、第三遍安静、不产生任何结论）
V1-E Workbench ✅（`apps/desktop`：Tauri 2 只做已编译视图的展示，第一屏 diff →
               用户挑哪些变动生效 → map。§21 列的七个屏幕全部有内容而不只是有命令——第五轮核对时发现
               Overview 与 Evolution 的构造器是空壳（"有命令不等于有屏幕"），§46 的六问与 §47 的五项
               现在逐条作答，答不出的那一问由屏幕自己在 `gaps` 里说原因（ADR-011 的规矩）。
               ADR 那一项**指出去而不建表**：它是 `docs/adr/` 的文档，为它造持久生产者就是第二个状态来源
               （禁止 4，与 ADR-032 同一套理由）。webview 本身的渲染仍是手工核对，写在 ADR-029 的后果里）
V1-F Context Engine ✅
V1-G Guard ✅
V1-H Hardening ✅（Export / Backup+Restore / Audit / Adapter Health / 监听 / 性能预算 /
Golden Test / CLI E2E；本轮补掉的正是上一版标"未做"的 Backup 与性能预算）
```

仍然有效的边界：**界面是核心逻辑的展示层，不是第二套逻辑。**
任何一轮如果让 Tauri 或扩展自己解释事件、自己拼状态，就是在违反 禁止 4 与 禁止 7。

V1 完成后进入 V2：

> **增强项目智能，而不是增加团队管理。**
