# V1_DEVELOPMENT — 个人 Vibe Coding 项目开发记录工作台 V1 开发文档

**版本：** V1.0  
**日期：** 2026-10-02  
**对应总控：** `PROJECT_MASTER.md` Master V2.0  
**大型迭代：** V1 — Project Continuity Core  
**状态：** Design Ready / Implementation Pending

---

# 1. V1 的目标

V1 只解决一个核心问题：

> **个人 vibe coding 开发者即使跨多个模型、Agent、聊天窗口和代码修改，也能够持续掌握项目最初目标、当前状态、关键决定、架构演进与下一步，并在需要继续工作时把正确上下文交给任意 Agent。**

V1 不追求功能数量。

V1 必须先建立一套不会随着项目变复杂而失控的项目记忆与状态底座。

---

# 2. V1 核心闭环

```text
Project Charter
      ↓
Chat / Agent / Files / Git
      ↓
Normalize
      ↓
Event Store
      ↓
State Judge
      ↓
Project State
      ↓
Decision / Scope / AEP / Evidence
      ↓
PROJECT_MAP
      ↓
Drift Check
      ↓
Context Pack
      ↓
Target Agent
      ↓
New Result
      ↓
Event Store
```

V1 的验收不是“页面做完”。

而是以上闭环可以真实运行。

---

# 3. V1 明确不做

V1 不开发：

- 多人协作；
- Team / Member；
- Role / Permission；
- 团队 Assignment；
- 企业工作区；
- 团队评论；
- 复杂甘特图；
- 复杂排期；
- 组织级审批；
- 完整云同步；
- 企业账号；
- 向量数据库强依赖；
- 全自动 Agent 执行；
- 通用 IDE；
- 代码编辑器。

---

# 4. 技术架构

## 4.1 推荐技术栈

```text
Desktop:
Tauri 2

UI:
React + TypeScript

Core:
Rust

Storage:
SQLite

SQLite:
WAL

Full-text:
FTS5

Browser:
Chrome / Chromium Manifest V3

Browser ↔ Native:
Native Messaging

Agent Integration:
Adapter SDK
MCP Server
CLI / Web fallback
```

---

# 5. 代码结构

推荐 Monorepo：

```text
vibe-project-workbench/
│
├── apps/
│   ├── desktop/
│   │   ├── src/
│   │   └── src-tauri/
│   │
│   └── browser-extension/
│
├── crates/
│   ├── core/
│   ├── domain/
│   ├── storage/
│   ├── ingestion/
│   ├── state-engine/
│   ├── guard-engine/
│   ├── context-engine/
│   ├── search/
│   ├── mcp-server/
│   ├── agent-runtime/
│   └── security/
│
├── packages/
│   ├── schemas/
│   ├── adapter-sdk/
│   ├── shared/
│   └── ui/
│
├── adapters/
│   ├── chatgpt-web/
│   ├── generic-web/
│   ├── generic-cli/
│   └── local-agent/
│
├── docs/
│   ├── adr/
│   ├── evolution/
│   └── architecture/
│
└── tests/
    ├── fixtures/
    ├── golden/
    ├── adapter-contract/
    └── e2e/
```

---

# 6. Repository / Port 边界

这是 V1 必须实现的架构约束。

Core 不允许直接调用 SQLite。

定义 Ports：

```text
ProjectRepository
EventRepository
EntityRepository
ConversationRepository
ArtifactRepository
SnapshotRepository
SearchIndex
SettingsRepository
```

当前实现：

```text
Project Core
      ↓
Repository Port
      ↓
SQLite Repository
```

未来如果出现个人多设备需求：

```text
Project Core
      ↓
Repository Port
      ↓
Local Repository
      ↓
SQLite
      ↕
Sync Engine
      ↕
Personal Cloud
```

Core 无需因此改写。

---

# 7. 数据模型总览

V1 一级对象：

```text
Project
ProjectCharter
ScopeBoundary
ArchitectureBaseline

Source
Conversation
Message

ProjectEvent
Evidence

Task
Issue
Decision
Artifact
Milestone

ChangeProposal
ArchitectureEvolutionPlan
DriftAlert

ProjectSnapshot
ContextPack
UserOverride
AuditLog
```

---

# 8. Project

```ts
type Project = {
  id: string
  name: string
  description?: string
  rootPath?: string

  status:
    | "active"
    | "paused"
    | "archived"

  currentPhase?: string

  createdAt: string
  updatedAt: string
}
```

---

# 9. Project Charter

Charter 是 V1 最重要的数据之一。

```ts
type ProjectCharter = {
  id: string
  projectId: string

  originalProblem: string
  targetUser?: string
  coreValue: string

  mustHave: string[]
  wontHave: string[]

  successCriteria: string[]
  architecturePrinciples: string[]
  constraints: string[]

  version: number

  locked: boolean

  createdAt: string
  updatedAt: string
}
```

规则：

- 初始化后允许用户修改；
- AI 不能直接覆盖；
- AI 只能提出 Change Proposal；
- 修改后保留旧版本；
- Drift Engine 以 Active Charter 为基线。

---

# 10. Scope Boundary

```ts
type ScopeBoundary = {
  id: string
  projectId: string

  inScope: string[]
  outOfScope: string[]
  deferred: string[]
  forbidden: string[]

  version: number
}
```

用途：

- 判断 Feature Creep；
- 判断任务是否与当前版本相关；
- Context Pack 中提供必要边界；
- Project Map 显示当前开发范围。

---

# 11. Architecture Baseline

```ts
type ArchitectureBaseline = {
  id: string
  projectId: string

  summary: string
  layers: ArchitectureLayer[]
  invariants: string[]
  replaceableParts: string[]

  version: number
  active: boolean
}
```

例如：

```text
Invariant:
Core 不直接依赖数据库实现。

Replaceable:
SQLiteRepository 可被 Local+Sync Repository 替换。
```

---

# 12. Source

```ts
type Source = {
  id: string
  projectId: string

  type:
    | "chatgpt_web"
    | "codex"
    | "qoder"
    | "hermes"
    | "dsh"
    | "generic_web"
    | "generic_cli"
    | "file"
    | "git"
    | "manual"
    | "mcp"

  externalId?: string
  displayName: string

  metadata: Record<string, unknown>
}
```

---

# 13. Conversation

```ts
type Conversation = {
  id: string
  projectId: string
  sourceId: string

  externalConversationId?: string
  title?: string

  startedAt?: string
  lastSeenAt?: string
}
```

---

# 14. Message

```ts
type Message = {
  id: string
  conversationId: string

  role:
    | "user"
    | "assistant"
    | "system"
    | "tool"

  content: string

  externalMessageId?: string
  contentHash: string

  createdAt?: string
  observedAt: string
}
```

原始 Message 只负责忠实记录，不承担项目判断。

---

# 15. Event Store

所有重要项目变化首先形成 Event。

```ts
type ProjectEvent = {
  id: string
  projectId: string

  type: string
  schemaVersion: number

  actorType:
    | "user"
    | "agent"
    | "system"

  actorId?: string

  sourceId?: string
  conversationId?: string
  messageId?: string

  originNodeId?: string

  causationId?: string
  correlationId?: string

  subjectType?: string
  subjectId?: string

  payload: Record<string, unknown>

  confidence: number
  evidenceRefs: string[]

  status:
    | "accepted"
    | "pending_review"
    | "rejected"
    | "superseded"

  occurredAt?: string
  observedAt: string
}
```

`originNodeId` V1 可为空，但保留用于未来个人多设备同步。

---

# 16. 核心 Event 类型

V1 至少支持：

```text
PROJECT_CREATED

CHARTER_CREATED
CHARTER_CHANGE_PROPOSED
CHARTER_UPDATED

SCOPE_ADDED
SCOPE_REMOVED
SCOPE_CONFLICT_DETECTED

TASK_CREATED
TASK_STARTED
TASK_BLOCKED
TASK_COMPLETED_CLAIM
TASK_COMPLETED_CONFIRMED
TASK_CANCELLED

ISSUE_FOUND
ISSUE_RESOLVED

DECISION_PROPOSED
DECISION_CONFIRMED
DECISION_SUPERSEDED
DECISION_REVERSED

ARTIFACT_CREATED
ARTIFACT_CHANGED
ARTIFACT_DELETED

MILESTONE_CREATED
MILESTONE_REACHED

CHANGE_PROPOSAL_CREATED
CHANGE_PROPOSAL_ACCEPTED
CHANGE_PROPOSAL_REJECTED

AEP_CREATED
AEP_TRIGGER_DETECTED
AEP_STARTED
AEP_COMPLETED

DRIFT_DETECTED
DRIFT_DISMISSED
DRIFT_ACCEPTED

FILE_CHANGED
GIT_COMMIT
TEST_PASSED
TEST_FAILED
BUILD_PASSED
BUILD_FAILED
```

---

# 17. Evidence

```ts
type Evidence = {
  id: string
  projectId: string

  type:
    | "message"
    | "file"
    | "git"
    | "test"
    | "build"
    | "manual"
    | "system"

  refId?: string
  summary?: string

  createdAt: string
}
```

核心对象状态应能连接 Evidence。

---

# 18. State Reducer

```text
Event Store
   ↓
State Reducer
   ↓
Materialized Project State
```

Reducer 必须 deterministic。

同一组 Accepted Events 应生成同一 Project State。

AI 不参与 Reducer 本身。

AI 只负责：

- 提议 Event；
- 分类；
- 识别潜在冲突；
- 辅助 Entity Resolution。

---

# 19. Task

```text
Todo
Doing
Blocked
Done
Cancelled
```

Agent 说“完成”：

```text
TASK_COMPLETED_CLAIM
```

不直接等于 Done。

如果存在：

```text
Git change
Test passed
User confirmation
```

则可形成：

```text
TASK_COMPLETED_CONFIRMED
```

---

# 20. Decision

```ts
type Decision = {
  id: string
  projectId: string

  title: string
  decision: string
  rationale?: string

  alternatives?: string[]

  status:
    | "active"
    | "superseded"
    | "reversed"

  locked: boolean

  evidenceRefs: string[]
}
```

Active Decision 应优先进入 Context Pack。

---

# 21. Issue

```text
Open
Investigating
Blocked
Resolved
Won't Fix
```

---

# 22. Artifact

Artifact 包括：

- Source File
- Config
- Document
- Database
- Prompt
- Diagram
- Generated Output
- Test
- Script

Artifact 不复制所有文件内容到数据库。

数据库主要保存：

```text
path
hash
type
version
relationship
```

---

# 23. Change Proposal

任何明显影响 Charter、Scope 或架构基线的变化，应先进入 Change Proposal。

```ts
type ChangeProposal = {
  id: string
  projectId: string

  proposedChange: string
  reason: string

  affectedScope: string[]
  affectedArchitecture: string[]

  benefit?: string
  cost?: string
  risk?: string

  charterConflict: boolean

  status:
    | "open"
    | "accepted"
    | "rejected"
    | "superseded"
}
```

---

# 24. AEP

```ts
type ArchitectureEvolutionPlan = {
  id: string
  projectId: string

  title: string

  current: string
  limitation: string
  whyNotNow: string
  trigger: string[]
  target: string

  changeSurface: string[]
  preservedCore: string[]

  benefit?: string
  cost?: string

  status:
    | "dormant"
    | "watching"
    | "triggered"
    | "in_progress"
    | "completed"
    | "superseded"
}
```

---

# 25. Drift Alert

```ts
type DriftAlert = {
  id: string
  projectId: string

  type:
    | "scope_drift"
    | "architecture_drift"
    | "decision_conflict"
    | "feature_creep"
    | "repeated_redesign"
    | "stale_assumption"
    | "unplanned_dependency"

  severity:
    | "info"
    | "warning"
    | "high"

  summary: string

  baselineRefs: string[]
  evidenceRefs: string[]

  status:
    | "open"
    | "accepted_change"
    | "dismissed"
    | "resolved"
}
```

---

# 26. Drift Engine V1

V1 不追求复杂 AI 推理。

采用：

```text
Rules
+
Structured Comparison
+
Optional LLM Judge
```

优先实现以下规则。

## 26.1 Scope Drift

新 Task / Feature 与：

```text
Out of Scope
Forbidden
```

高度相关时提示。

---

## 26.2 Decision Conflict

新建议与 Active Decision 冲突时提示。

---

## 26.3 Architecture Drift

代码或讨论开始绕过 Architecture Invariant 时提示。

例如：

```text
Invariant:
Core 不直接访问 SQLite。

Detected:
Core module imports sqlite storage directly.
```

---

## 26.4 Repeated Redesign

同一对象被反复重新设计且没有明确 Decision Supersede 时提示。

---

## 26.5 Feature Creep

新增 Feature 没有对应：

```text
Charter
Scope
Change Proposal
AEP
```

时进入 Drift Inbox。

---

# 27. State Judge

State Judge 判断新输入是否形成 Project Event。

流程：

```text
Message / Git / File
       ↓
Rule Filter
       ↓
Candidate Event
       ↓
LLM Judge（必要时）
       ↓
Confidence
       ↓
Accepted / Inbox / Candidate
```

建议阈值：

```text
>= 0.90
自动接受

0.65–0.89
进入 Inbox

< 0.65
保存候选，不修改状态
```

用户反馈后允许调参。

---

# 28. Entity Resolution

解决：

```text
auth
authentication
login module
用户认证
```

是否同一对象。

依据：

1. explicit ID；
2. file relation；
3. conversation；
4. name；
5. recent context；
6. semantic similarity。

不确定时不自动合并。

---

# 29. Project Inbox

Inbox 接收：

```text
Low-confidence Event
Decision Proposal
Change Proposal
Drift Alert
Merge Candidate
Agent Claim Without Evidence
Conflict
```

示例：

```text
Codex:
“建议直接让 Core 调 SQLite，减少一层抽象。”

系统：
与 Architecture Baseline 冲突。

[保持现有架构]
[建立 Change Proposal]
[忽略]
```

---

# 30. PROJECT_MAP Compiler

Project Map 由结构化状态编译。

禁止：

```text
LLM 每次自由重写整个 Project Map
```

流程：

```text
Structured State
      ↓
Deterministic Template
      ↓
Optional LLM wording polish
      ↓
PROJECT_MAP.md
```

---

# 31. PROJECT_MAP 默认结构

```markdown
# Project Map

## Original Intent

## Current Phase

## Current Status

## Scope

## Architecture Baseline

## Active Decisions

## Active Tasks

## Open Issues

## Key Artifacts

## Architecture Evolution

## Recent Changes

## Drift / Conflicts

## Next Actions
```

---

# 32. Snapshot

关键阶段保存：

```text
Project State Snapshot
Project Map Snapshot
Charter Version
Decision State
AEP State
Artifact References
```

用途：

- 阶段回顾；
- Diff；
- 判断偏航；
- 恢复；
- Context Pack。

---

# 33. Project Diff

用户重新打开项目时显示：

```text
Since Last Snapshot

Added
+ 3 Tasks
+ 1 Decision

Changed
Auth: Doing → Done

Architecture
Repository interface added

Evolution
AEP-002 still Dormant

Drift
1 new Scope Drift warning

Next
Implement Context Compiler
```

---

# 34. Browser Extension

职责：

- 用户授权读取当前网页会话；
- 获取选中文本；
- 识别来源；
- 导入新消息；
- 接收 Context Pack；
- 打开目标网页；
- 准备输入内容。

不负责：

- 本地数据库；
- 高权限文件操作；
- 项目状态判断。

---

# 35. Native Messaging

链路：

```text
Content Script
   ↓
Extension Service Worker
   ↓
Schema Validation
   ↓
Native Messaging
   ↓
Desktop Core
```

必须验证：

```text
origin
command
payload
size
project scope
```

---

# 36. Adapter SDK

```ts
interface AgentAdapter {
  id: string

  detect(): Promise<boolean>

  capabilities(): Promise<AgentCapabilities>

  ingest?(
    cursor?: SyncCursor
  ): Promise<NormalizedConversationBatch>

  open?(
    target?: AgentTarget
  ): Promise<void>

  prepare?(
    pack: ContextPack
  ): Promise<PreparedPayload>

  inject?(
    payload: PreparedPayload
  ): Promise<InjectResult>

  deepLink?(
    target: AgentTarget
  ): Promise<string | null>
}
```

---

# 37. Capability Model

```text
READ_HISTORY
READ_CURRENT_THREAD
OPEN_APP
OPEN_THREAD
INJECT_TEXT
READ_LOCAL_SESSION
MCP
FILE_CONTEXT
STREAM_RESULT
```

V1 默认不要求 `AUTO_EXECUTE`。

---

# 38. V1 Adapter 范围

必须支持：

1. Manual Import
2. Generic Web
3. ChatGPT Web
4. Generic CLI 或一个 Local Agent
5. MCP 基础接口

Qoder、Hermes、DSH 等可逐步增加，不应阻塞 Core。

---

# 39. Git / File Watcher

监听：

```text
file_created
file_modified
file_deleted
git_commit
git_branch_changed
git_status_changed
```

可选接入：

```text
test_result
build_result
```

文件变化只是 Evidence，不自动代表功能完成。

---

# 40. Search

V1：

### L1
Structured Search

### L2
SQLite FTS5

### L3
Relation Search

```text
same task
same decision
same issue
same artifact
same conversation
```

语义向量检索留到 V2。

---

# 41. Context Pack

用户选中：

```text
“为什么同步还是重复？”
```

系统编译：

```markdown
# Project Context

## Original Intent
...

## Current Phase
...

## Selected Context
...

## Relevant Scope
...

## Active Decisions
...

## Architecture Constraints
...

## Related Issue
...

## Relevant Artifacts
...

## Recent Evidence
...

## Related AEP
...

## Drift Warnings
...

## Request
...
```

---

# 42. Context Retrieval 优先级

```text
1. Selected Context
2. Charter / Relevant Scope
3. Active Decision
4. Direct Entity
5. Relevant Artifact
6. Current Issue
7. Recent Events
8. AEP
9. Related Conversation
10. General Project Map
```

---

# 43. Context Budget

避免把整个项目塞给 Agent。

建议初始比例：

```text
10% Original Intent / Charter
15% Decisions / Constraints
20% Relevant State
25% Files / Artifacts / Evidence
15% Recent Discussion
5% AEP / Drift
10% Current Request
```

只作为策略，不硬编码。

---

# 44. Agent Routing

V1 可以提供推荐：

```text
Codex
适合：repo 内代码修改

ChatGPT
适合：架构、产品、分析

Generic CLI Agent
适合：本地执行
```

系统只推荐，不替用户决定。

---

# 45. Workbench UI

V1 不以 Kanban 为首页。

建议：

```text
┌──────────────┬────────────────────────┬──────────────┐
│ Navigator    │ Project Workspace      │ Context      │
│              │                        │ Panel        │
│ Overview     │ Original Intent        │              │
│ Project Map  │ Current Phase          │ Selected     │
│ Timeline     │ Current Status         │ Context      │
│ Decisions    │                        │              │
│ Evolution    │ Recent Changes         │ Ask with     │
│ Drift/Inbox  │                        │ ChatGPT      │
│ Search       │ Drift / Conflicts      │ Codex        │
│              │                        │ Other Agent  │
│              │ Next Actions           │              │
└──────────────┴────────────────────────┴──────────────┘
```

---

# 46. Overview

必须首先回答：

```text
最初目标是什么？
当前阶段是什么？
最近发生了什么？
当前最大问题是什么？
有没有偏航？
下一步是什么？
```

---

# 47. Evolution View

专门展示：

```text
Architecture Baseline
Active ADR
AEP Registry
Triggered AEP
Completed Evolution
```

目的：

> 未来架构走向不能继续埋在聊天记录里。

---

# 48. Timeline

Timeline 来源于 Event Store。

示例：

```text
18:42 Codex
修改 reconnect logic

18:45 System
Issue I-018 状态仍为 Open
原因：tests failed

19:12 User
确认继续保留 Repository abstraction

19:13 System
Decision D-011 locked
```

---

# 49. 数据库建议表

```text
projects
project_charters
scope_boundaries
architecture_baselines

sources
conversations
messages

events
evidence
evidence_links

entities
entity_versions
entity_links

tasks
issues
decisions
artifacts
milestones

change_proposals
architecture_evolution_plans
drift_alerts

snapshots
project_map_snapshots

sync_cursors
adapter_states

context_packs
context_pack_items

user_overrides
audit_logs

schema_migrations
```

---

# 50. 最小数据库版本

如果第一阶段需要进一步收缩：

```text
projects
project_charters
scope_boundaries

sources
conversations
messages

events
entities
evidence

architecture_evolution_plans
drift_alerts

snapshots
context_packs

user_overrides
schema_migrations
```

Task / Decision / Issue 可先作为 Entity subtype。

---

# 51. Sync Engine

每个 Source 独立 Cursor。

```ts
type SyncCursor = {
  adapterId: string
  sourceId: string

  cursor?: string
  lastSeenAt?: string
  lastContentHash?: string
}
```

要求：

```text
Incremental
Idempotent
Retry
Resume
Partial Failure
Dedup
```

---

# 52. 去重

### L1
External ID

### L2
Content Hash + Role + Time Window

### L3
Semantic Near-duplicate

L3 只提示，不直接删除。

---

# 53. Security

## Secrets

API Key、Token 不进 SQLite 明文。

使用系统 Keychain。

数据库只保存：

```text
secret_ref
```

---

## Project Policy

每个 Project 可设置：

```text
Local Only
Allow Cloud Model
Never Send Files
Never Send Raw Conversation
Redact Secrets
```

Context Pack 编译前执行 Policy Filter。

---

# 54. Audit

记录：

```text
何时
什么组件
读取什么
生成什么 Context Pack
准备到哪个 Agent
哪些内容被 Redact
```

---

# 55. Backup / Export

V1 必须支持：

```text
Export Project State
Export PROJECT_MAP
Export Charter
Export Decisions
Export AEP
Export Timeline
```

推荐后期形成：

```text
.projectpack
```

V1 可以先使用 zip/json/md 组合实现。

---

# 56. AEP Registry

V1 开发本身建议先建立以下 AEP：

```text
AEP-001
Local-only → Personal Local + Cloud Sync

AEP-002
FTS5 → Hybrid Semantic Retrieval

AEP-003
Built-in Adapter → Plugin Adapter Runtime

AEP-004
Basic Drift Rules → Intelligent Drift Engine

AEP-005
Flat Project State → Entity / Architecture Graph
```

---

# 57. ADR 起始清单

```text
ADR-001 Local-first
ADR-002 Event-first
ADR-003 Evidence-first
ADR-004 Repository Ports
ADR-005 SQLite
ADR-006 Tauri
ADR-007 Browser Extension + Native Messaging
ADR-008 Adapter-first
ADR-009 Context Pack
ADR-010 Charter is User-controlled
ADR-011 Project Map is Compiled View
ADR-012 No Team Collaboration
```

---

# 58. 测试策略

## Unit

重点：

```text
Reducer
Repository Contract
Dedup
Project Map Compiler
Drift Rules
Context Budget
Policy Filter
```

---

## Golden Test

固定输入：

```text
Conversation
Git Events
Files
Charter
```

预期：

```text
Events
Decisions
Tasks
Drift Alerts
Project Map
```

---

## Adapter Contract

每个 Adapter：

```text
detect
capabilities
ingest
open
prepare
inject
fallback
```

---

## E2E

至少覆盖：

```text
Browser
→ Extension
→ Native
→ Event
→ State
→ Project Map
→ Select Issue
→ Context Pack
→ Browser Prepare
```

---

# 59. 性能原则

禁止每次打开项目：

```text
把全部聊天重新发送给模型。
```

必须采用：

```text
Incremental Sync
Incremental Event Extraction
Materialized State
Cached Project Map
Indexed Search
Snapshot
```

---

# 60. 成本原则

### Zero-cost

```text
Hash
Rules
SQLite
FTS
Git
Reducer
Diff
```

### Cheap Model

```text
Classification
Event significance
Entity candidate
Basic drift judgement
```

### Strong Model

```text
Complex conflict
Deep architecture comparison
Context compilation
Large project storyline analysis
```

---

# 61. V1 内部开发顺序

## Stage A — Foundation

```text
Monorepo
Tauri
Rust Core
SQLite
Migration
Repository Ports
Event Store
```

验收：

```text
Create Project
Append Event
Replay Events
Idempotent Write
```

---

## Stage B — Project Baseline

```text
Project Charter
Scope Boundary
Architecture Baseline
User Lock
```

验收：

```text
建立一个项目
保存 Charter
修改 Charter 产生版本
AI 无法直接覆盖 Lock
```

---

## Stage C — State Engine

```text
Entity
Task
Issue
Decision
Artifact
Milestone
Change Proposal
AEP
Evidence
Reducer
```

验收：

```text
Event → State deterministic
Decision supersede works
Evidence trace works
```

---

## Stage D — Capture

```text
Manual Import
Browser Extension
Native Messaging
Conversation Normalizer
Sync Cursor
Dedup
Git / File Watch
```

验收：

```text
至少两个来源可进入同一个项目
重复同步不生成重复 Message / Event
```

---

## Stage E — Project Map & Workbench

```text
Overview
Project Map
Timeline
Decisions
Evolution
Inbox
Search
```

验收：

```text
Project Map 自动编译
Timeline 可追溯
Decision 可打开 Evidence
```

---

## Stage F — Context Engine

```text
Select
Retrieve
Budget
Compile
Preview
Route
Prepare
Fallback
```

验收：

```text
选中 Issue
→ Context Pack
→ 准备到另一个 Agent
```

---

## Stage G — Guard

```text
Scope Drift
Decision Conflict
Architecture Drift
Feature Creep
Repeated Redesign
```

验收：

```text
至少四类测试样本能稳定触发预期 Alert
误报可以 Dismiss
```

---

## Stage H — Hardening

```text
Backup
Export
Audit
Adapter Health
Golden Regression
E2E
Performance
```

---

# 62. V1 Release Gate

V1 发布前必须同时满足：

### Continuity
项目跨两个来源可连续记录。

### Fidelity
Charter / Scope / Decision 不会被 AI 静默覆盖。

### Traceability
关键状态可追溯 Evidence。

### Drift
至少具备基础偏航检测。

### Context
能把正确上下文准备给另一个 Agent。

### Recovery
项目可以导出、备份和恢复。

### Degradation
某个 Adapter 失效时仍能 Copy Context Pack 继续工作。

---

# 63. 完整示例

项目 Charter：

```text
目标：
构建本地优先的个人知识工具。

Must Have：
本地项目记录。
跨 Agent 上下文。

Won't Have：
团队协作。
```

后续 Codex 提出：

```text
建议增加团队 Workspace 和成员权限。
```

State Judge：

```text
DECISION_PROPOSED
```

Guard Engine：

```text
DRIFT_DETECTED
type = scope_drift

Reason:
Proposal conflicts with Charter.wontHave
```

Workbench：

```text
Potential Drift

“团队 Workspace”
与项目最初边界冲突。

[保持边界]
[建立 Change Proposal]
[忽略提示]
```

用户选择：

```text
保持边界
```

系统记录：

```text
DRIFT_DISMISSED? No
DECISION_REJECTED
```

几天后用户点击当前架构问题：

```text
为什么还保留 Repository 层？
```

Context Engine 自动加入：

```text
Architecture Baseline
ADR-004
AEP-001
Relevant Code
Previous Decision
```

然后准备给 ChatGPT：

```text
请基于当前项目的架构意图解释 Repository 层是否仍有必要。
不得重新设计整个 Core。
```

这就是本产品 V1 应实现的核心体验。

---

# 64. V1 最终判断

V1 成功的标准不是：

> 做出很多项目管理页面。

而是：

> **项目开发越来越多时，用户反而越来越清楚这个项目为什么存在、为什么这样设计、现在在哪、下一步去哪。**

如果这个目标没有实现，即使已经有 Timeline、搜索、Agent 接入和大量 UI，V1 仍然不算完成。
