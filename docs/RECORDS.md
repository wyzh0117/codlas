# 开发记录

`PROJECT_MASTER.md` §23 要求每轮重要开发留下记录，§24 又要求总控文件本身不写流水。
所以完整记录留在这里，总控只留 §20 的四个块和一个指针。

倒序，最新在上。

---

## 2026-10-03 · V1 第一轮：核心闭环可运行（Stage A/B/C/D/F/G/H，CLI 表面）

### LAST COMPLETED

九个 crate + 一个 CLI 组成的 Rust monorepo，把 §7.2 的那条链完整跑通：
`Source → Event → Reducer → Project State → PROJECT_MAP → Context Pack → Agent`。

- **A Foundation**：Cargo workspace；`vibe_core::ports::Repositories` 十个 trait；SQLite schema v1→v3 +
  migration + FTS5 + WAL；Event Store 按 store 分配的 `seq` 确定性重放；`events.dedup_key UNIQUE` 与
  `messages UNIQUE(conversation_id, content_hash)` 让重复采集写不出第二件事。
- **B Baseline**：Project / Charter / Scope / Architecture Baseline 全部版本化，只有 `ActorType::User` 能写，
  其他 actor 得到 `CoreError::Locked`；非用户想改走 Change Proposal（§9）。
- **C State Engine**：Reducer 折叠 accepted log 成 Materialized State；`state_hash` 排除 `built_at` 与只读
  baseline 段，因此同一日志重放两次字节一致；PROJECT_MAP Compiler 输出 §31 的 13 个 section，空 section
  写 `_none recorded_` 而不是被删掉；Snapshot + Diff。
- **D Capture**：Adapter SDK（`AgentAdapter` trait + `IngestPipeline`）；手动粘贴 / 手动文件 / transcript
  （JSON 数组、ChatGPT `mapping`、JSONL）/ 本地 CLI session 目录；`git log` 只读采集（子进程白名单只有
  read-only 子命令，无法改仓库）；文件树扫描带 size/binary 过滤与 hash 比对。
- **F Context Engine**：§40 三层检索 → §42 选取阶梯 → §43 分 section 预算 → §53 Policy Filter →
  `render_markdown` → §54 Audit → §44 路由建议 → `prepare` 复制兜底。
- **G Guard**：六条词法 drift 规则；alert id 内容寻址，`check()` 重复运行不产生第二份；规则只报警，
  永不改 baseline；§27 State Judge 阈值 0.90 / 0.65，强标记 +0.12，`user_only` 规则不匹配非用户 actor。
- **H Hardening**：§55 六个导出 + `.projectpack`；§54 audit CLI；§53 secret store；§58 Golden Test +
  11 条 CLI 端到端测试。

### FILES CHANGED

本轮（CLI 与端到端闭环）新增或改动：

```text
apps/cli/src/lib.rs                       +context / +export / +policy 模块声明
apps/cli/src/main.rs                      Context/Policy/Secret/Export 接进 Command
apps/cli/src/context.rs                   新增：§40-§44 与 §59 的 build / plan / prepare / route
apps/cli/src/export.rs                    新增：§55 六个导出 + .projectpack
apps/cli/src/policy.rs                    新增：§53 policy show/set/check + secret put/list/get/rm
apps/cli/src/views.rs                     search 改走三层 pipeline + 新增 reindex
apps/cli/src/capture.rs                   新增 import git / import files（绑定目录即来源）
apps/cli/src/app.rs                       write_file / pointer 辅助
apps/cli/tests/golden.rs                  §58 四条固定输入重放整条闭环（7 条）
apps/cli/tests/cli_e2e.rs                 新增：驱动真实 wp 二进制的端到端（11 条）
crates/storage/src/entity.rs              searchable_text：索引记文本，不记编码
crates/storage/src/event.rs               事件正文同样只取 payload 里的文本
crates/storage/src/infra.rs               related() 的参数绑定与 rank 列修正
crates/storage/src/schema.rs              migration v3：entities 主键改为 (kind, id)
crates/storage/src/lib.rs                 SCHEMA_VERSION 由 MIGRATIONS 推出
crates/storage/tests/{integration,write_paths}.rs   各加一条锁死上述缺陷
crates/search/src/service.rs              find_in()：kind 过滤贯穿 L1/L2/L3
crates/search/src/catalog.rs              L1 命中若无正文时回落到 title
crates/search/tests/retrieval.rs          §40 三层真实 SQLite 测试（10 条）
crates/context-engine/src/retrieve.rs     title-only 决策不再渲染出悬空的 " — "
crates/context-engine/src/request.rs      预算 / 窗口 / 选择的 builder 收口
crates/context-engine/tests/{budget_pack.rs,support/mod.rs}
docs/adr/                                 ADR-013/016 修订 + ADR-017..020
docs/evolution/AEP-002.md                 补进 unicode61 不切中文这个事实
```

### TEST STATUS

```text
cargo test --workspace   →  187 passed / 0 failed / 1 ignored
cargo build --workspace  →  0 warning
```

唯一 ignored 是 `crates/security/tests/keychain_opt_in.rs` 中真正读写 macOS Keychain 的那条：
它需要 `VIBE_SECURITY_ALLOW_KEYCHAIN=1`，不配就不该碰用户钥匙串。

按 crate：domain 2 · core 5 · storage 22 · state-engine 30 · guard-engine 16 · context-engine 36 ·
ingestion 14 · security 34 · search 10 · cli 18（golden 7 + e2e 11）。

### DECISIONS MADE

写进了 ADR：

- ADR-013（修订）：`entities` 主键从全局 `id` 改成 `(kind, id)`。旧 schema 下不同 kind 撞同一个 id 会
  让一行携带一个 kind 的名字和另一个 kind 的正文——一条确认决策的主题 id 若与某条 issue 相同，那条
  issue 就被静默改写。migration v3 前滚，不改基线 v1。
- ADR-017：§53 说"使用系统 Keychain"，本轮把它做成**显式 opt-in**。没有 `VIBE_SECURITY_ALLOW_KEYCHAIN=1`
  就拒绝写钥匙串并说明如何开启，否则一次普通导入就擅自动用了用户的系统凭据存储。默认落 0600 文件库；
  SQLite 里永远只有 `secret_ref`。
- ADR-018：全文索引记的是记录**说了什么**，不是它怎么编码。之前 `wp search` 的摘要列输出整段 JSON，
  字段名（`evidenceRefs`）和 64 位内容 hash 都成了可匹配词。
- ADR-019：`.projectpack` 用 stored（不压缩）zip，manifest 带 `stateHash` 并写明"这是 accepted log 的投影，
  不是第二个真源"（禁止 4）。
- ADR-020：本轮的 V1 用户表面是 CLI。`apps/cli/tests/golden.rs` 走库、`cli_e2e.rs` 走真实二进制，
  两者共用同一实现，不存在"测试里的那套 CLI"和"用户用的那套 CLI"。

### AEP IMPACT

- AEP-002（FTS → 混合检索）：补进一条具体的触发证据——unicode61 不做中文分词，一串汉字是一个 token，
  所以短查询命中长句里的片段会失败。trigram tokenizer 是比向量索引便宜得多的中间档，已写进该 AEP。
- AEP-005（平面 state → graph）：`entity_links` 已经是 L3 的地基，本轮没提前做图。
- AEP-001（本地 → 云同步）：未触发。`ProjectEvent.originNodeId` / `SyncCursor.lastContentHash` 仍是预留位。
- AEP-003 / AEP-004：未触发。

### KNOWN ISSUES

1. **中文检索是分词盲区**。`full_text_matches_a_whole_token_not_a_piece_of_a_chinese_run` 把这个限制钉成
   测试，而不是留成使用中的意外。解法是 trigram（见 AEP-002）。
2. **feature_creep 规则在大仓库上会刷屏**。三文件的临时仓库里，每个未连到 Charter mustHave 的 artifact
   都单独报一条 info。规则本身符合 §26，但界面之前需要聚合或阈值，否则 Drift 面板不可读。
3. **`import git` 读的是绑定目录**。若绑定的根目录本身不是仓库而其祖先是，git 会去读祖先的。
   报错原样透传 git 的 stderr，前缀是 `io failure:`，措辞可以再准，但不掩盖原因。
4. **§53 的 Redact 发生在 pack 边界**。本地读视图（`wp search` / `wp timeline` / 六个导出）仍显示用户
   自己记录过的原文——按 §53 的措辞（"Context Pack 编译前执行 Policy Filter"）这是故意的：策略管的是
   离开本机的内容，不是让人读不到自己的项目记录。
5. **Backup（§55 的备份一侧）与性能预算（§61 H）未做**。导出已经能当备份用，但没有 restore 路径。
6. **V1-E 界面、V1-D 浏览器扩展、MCP server 本轮不做**，理由与边界见 ADR-016。

### DRIFT CHECK

对自己的代码跑自己的规则：

- `grep rusqlite crates/*/Cargo.toml apps/*/Cargo.toml` 只命中 `crates/storage`——INV-001
  （"Core 不直接依赖 SQLite 实现"）在真实依赖图里成立，不只靠一条 drift 规则。
- 临时仓库里放一个 `crates/core/src/db.rs` 引用 rusqlite，`wp import files` 之后 `wp drift check` 报出
  `architecture_drift high · Artifact "crates/core/src/db.rs" sits in layer "core" which invariant
  "Core 不直接依赖 SQLite 实现" constrains`，PROJECT_MAP 的 Drift / Conflicts 段同一句话。
- Golden 测试钉住：drift 报警**没有**改动 Charter 的 `wont_have`（§26 的规则只提示，不改基线）。
- 本轮新增的 `apps/cli` 与 `crates/search` 依赖方向都在 §7.5 的层次之内；没有为了让 CLI 跑通而把逻辑
  从 crate 搬回 CLI——CLI 只做解析、借文件、打印。

### NEXT IMMEDIATE TARGET

V1-E Workbench：Tauri 2 桌面壳，承载已经编译好并已被测试过的视图
（Overview / Timeline / Project Map / Decisions / Drift + Inbox / Search / Context Preview）。
核心逻辑不再新增；界面出现的每个数字都必须能从某个 port 的读取解释出来。
之后补 V1-D 的浏览器扩展 + Native Messaging，把 ChatGPT 实时会话变成第二个来源。
