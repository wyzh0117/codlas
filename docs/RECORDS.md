# 开发记录

`PROJECT_MASTER.md` §23 要求每轮重要开发留下记录，§24 又要求总控文件本身不写流水。
所以完整记录留在这里，总控只留 §20 的四个块和一个指针。

倒序，最新在上。

---

## 2026-10-06 · V1 第五轮：不按上一轮的自报验收，把整个 V1 重查一遍（ADR-032 / ADR-033，就地修订 ADR-013 / ADR-016 / ADR-020 / ADR-022）

这一轮的输入不是"下一步做什么"，而是一个问题：**前四轮的账，有没有哪一行是拿自报当证据记上去的？**
所以做法只有一条：把 `PROJECT_MASTER §21` 的八段五十四个条目、`V1_DEVELOPMENT §14` 的十条验收、
`§62` 的七道发布门逐条回到代码与测试里核对，**不看报告，只看文件**。核对结果分三类：
真缺（补）、已做但没被任何测试声称过（补测试）、账上写过头（改账）。

### 一、查出来的四处，以及为什么只有一处是"没做"

- **§50 的 `context_packs` 表不存在**——账上从没提过这一笔。这不是漏建，是一个需要写下来的决定：
  包是**按需编译**的，编译一次留一行 `audit_logs`（`produced_ref` / `read_refs` / `redacted_refs` / 目标 agent），
  成品以文件形式出去（`wp context build --out`、`.projectpack`）。把包再存成一张表，等于把项目事实
  抄第二份（禁止 4）。记为 **ADR-032**，并写清重看的触发条件：一旦要求"改完之后逐字节重放同一份包"，
  那时的形状是 `evidence` 里一份内容地址化的工件，不是新的一列。
- **§52 的 L2 键把 role 漏了**——真缺陷，而且是**会吃掉证据**的那种：`(conversation_id, content_hash)`
  意味着用户说"继续"、助手也答"继续"时，第二条被静默丢弃；同一句话一小时后说第二遍，也丢。
  第一轮还有一条测试把这个行为**钉成了正确的**（`duplicate_messages_are_dropped_on_reingest`）。
  修法与代价记为 **ADR-033**：role 进键（migration **v4** `messages_dedup_key_includes_role`），
  **时间窗仍然不进键**——§51 要求重推幂等，而来源时间戳可能缺失或抖动；"同一句话后来又说了一遍"
  归 L3 提示，提示不删除。SQLite 不肯 drop 内联 UNIQUE 背后的自动索引，所以与 ADR-013 的 v3 同一副
  骨架：**重建表**而不是改表。老测试里那句断言同轮改掉——它当时锁住的是缺陷，不是需求。
- **§46 总览屏与 §47 演进屏是空壳**：`views.rs` 里两个视图构造器存在、命令也注册了，
  于是 §26 的 V1-E 那行看起来是 ✅；但 §46 要求的六个问题没有一个被回答，§47 要求的五项里
  只有 AEP 一项有数据。**"有命令"不等于"有屏幕"**——这一条正是本轮要抓的那类自报。
- **§29 的七种 Inbox 里有两种没有持久生产者**：merge candidate 只活在 `IngestReport` 的那一次运行里
  （那一轮本身已经作为事件留在日志），unverified claim 出现在地图与快照里而不是队列里。
  给它们各建一张表需要 §16 没有列的事件类型，而禁止 8 不许在没有触发条件时先建——
  所以本轮**记下这个缺口**（写进 ADR-016 的 Deferred 清单），而不是把它做出来。

### 二、已做但没有测试声称过的路径

`MILESTONE_CREATED` / `MILESTONE_REACHED` 在目录、折叠、物化、渲染四条路上都在，唯独没有一条测试
说"planned 变成 reached"。§28 的解析阶梯（1.0 显式 id / 0.95 同标题 / 0.70 词重叠 / 0.90 自动合并阈值）
只有 guard-engine 侧的判定测试，没有阶梯本身。§14 第 10 项"结果回到时间线"此前是被**采集幂等**测试
从反面证明的（每轮 import 测试断言的都是"再来一次什么都没新增"），从来没有一次**真的推一批新材料**
上去看时间线、地图与 diff 是否动了。§39 的 `file_deleted` 同理——`ScanKind::Deleted` 在报告里存在，
但没有任何测试让一个文件真的消失过。

### 三、改掉的账（三处，都是写过头）

- **ADR-022** 被自己的第 2/3/4 条绊倒：`payload.change` 里写了 `deleted` 却从不发出（删除走
  `ARTIFACT_DELETED`）、引用的 `FileScan::diff` 其实叫 `FileScan::run`、而"这些事件一律不折叠"
  只对 git 那两个信号成立——`FILE_CHANGED` 确实 upsert artifact 行，`ARTIFACT_DELETED` 确实标记它，
  而 artifacts 在 `state_hash_of` 里。**标题因此从"move no state"改成"produce no verdict"**：
  监听信号不能触碰的是**结论**（任务、问题、决策、提案、偏航一个都不许动），文件行是它对文件的观察本身。
- **ADR-013** 引用 §52 键的那句约束随 ADR-033 改写；**ADR-016** 补两条（§29 的两种类别按设计无持久
  生产者；§14 第 10 项现在真有测试）；**ADR-020** 的测试层清单加上新落的 `resolution.rs` 与扩充后的
  `cli_e2e` / `watch` / `views`。

### 四、这一轮的测试

新增或改写：`crates/storage/tests/integration.rs` 三条（同文本双角色=两条、同角色重复=一条、
旧库升级后键真的换了）、`crates/ingestion/tests/resolution.rs` 三条（新建，解析阶梯本身）、
`crates/state-engine/tests/map_and_snapshot.rs` 一条（milestone planned→reached，且 reached 不许改名）、
`crates/ingestion/tests/watch.rs` 一条（真目录里真删一个文件：一个移除信号、行被标记而非删除、
第三遍安静、折叠不产生任何结论）、`apps/cli/tests/cli_e2e.rs` 两条（§14 第 10 项的完整回流、
`wp watch` 表面的消失信号）。桌面壳的 §46/§47 两屏连同其视图层测试同轮补齐。

`cargo test --workspace` = **300 passed / 0 failed / 1 ignored**（Keychain 那条仍需显式环境变量），构建零 warning。
四层里变化最大的是桌面壳视图：`apps/desktop/tests/views.rs` 从 12 条到 **23 条**，`cli_e2e` 从 20 到 **22**，
`integration` 到 **15**，`watch` 到 **7**，新建的 `resolution` **3**，`map_and_snapshot` **10**。
第 23 条是这一轮跑完之后再补的一条线：`the_new_answers_reach_the_webview_under_the_keys_it_reads` 不检查渲染，
它把两个新视图序列化出来的 camelCase 键名钉死（`nextActions`、`originalProblem`、`replaceableParts`、
`whyNotNow`……），Rust 一侧改个名就会红——webview 拿到 `undefined` 这种事因此在 Rust 这一侧被拦住了。
写这一段时我先把它说成"与 `app.js` 读的键逐个对上"，去读测试本身才发现它并不解析 JS 文件：
JS 一侧单方面改名仍然只有人打开窗口才会看见，那正是 ADR-029 已经点名的人工核对，不是这一层能声称的东西。
一个测试的名字描述得比它做的事多，和被测的界面描述得比它做的多，是同一类账。

这个 300 不是重新数一遍的结果，而是**同一轮里第二次跑**的结果：第一次跑完之后桌面壳那侧还落了最后一批
测试，我先把上一轮记的 299 / views 22 当成结论写进了四份文档，才去比对文件时间戳，发现
`tests/views.rs` 的写入时间晚于那次 `cargo test` 的开始时间——于是重跑、把四个数字改回来。
账面数字的来源必须是"跑过的那一次"，不能是"我以为跑过的那一次"。

第四、五轮两次都没提交的改动合成一个提交（`bd88311`），连同第三轮 `b80669a` 一起推到私有仓库；
共享文档（总控、RECORDS、ADR 页）里两轮的决定互相引用，拆开提交会让每个提交都指向一个当时并不存在的页面。

### 五、V1 到底完成了没有

按 §21 的八段回看：V1-A / B / C / F / G / H 六段没有一个条目缺代码；V1-E 缺的是这一轮补掉的两屏内容；
V1-D 仍然欠着 **ADR-028 的入站 MCP 采集**，另外两处不是代码欠项而是**人的核对**：桌面壳的 webview 渲染
（ADR-029）与扩展在真 Chrome 里的表现（ADR-031）。§14 的十条验收现在逐条有测试名对应，
§62 的七道发布门也逐道有（报告页里列成表）。所以"V1 全部完成"这句话**仍然不能写**——
能写的是：除了入站采集与其依赖的真实 agent 会话源，V1 没有任何一段留着没写的代码，
也没有任何一行账是拿"应该做了"当证据记上去的。


## 2026-10-06 · V1-D 的浏览器门落地：`wp host` 与 MV3 扩展对着写成（ADR-031，就地修订一次）

同一天里的第四轮。上一轮清账时剩下三件欠着的，这一轮做掉第一件（§34 浏览器扩展 + §35 Native Messaging），
并且是在**两侧同时写、互相对着校验**的前提下做的——通道的一头是 Rust 宿主，另一头是 JS 扩展，
任何一侧单方面定义协议都会在下一个人手里第一次运行时散架。

### 一、宿主这一侧：`crates/ingestion/src/native_messaging.rs` + `apps/cli/src/host.rs`

四个动词、一问一答、永不监听：`hello` / `capture` / `open` / `inject`。帧是四个小端字节的长度前缀加
UTF-8 JSON，声明超过 1 MiB 的帧在**解析任何 JSON 之前**被拒（§35 的 `size` 唯一说得通的位置）。
`open` 与 `inject` 答的是**给扩展的指令**（`{action:"openTab",url}` / `{action:"fillComposer",text,send:false}`），
`send:false` 是协议里一个写死的字段而不是可商量的默认值——§7.8 从此不依赖任何一方的自觉。
§35 的五项检查各占一个拒绝前缀（`origin` / `command` / `payload` / `size` / `project scope rejected`），
测试按前缀断言，所以一个检查失效不会被另一个的失败掩盖；一条坏帧只结束那一帧，会话继续（§62）。

浏览器推来的线程走 `IngestPipeline::ingest_messages`，**不是**第八个适配器：适配器是读者，
活的会话在浏览器那一侧是被推的一方，给它造一个"主动读"的 trait 只会把"谁来发起"这件事说假。
游标一条没动，幂等仍由 §52 的去重保证；`payload.source` 只允许 `chatgpt_web` / `generic_web`，
身份与 Source 行由 pipeline 决定。

宿主是 `wp` 的子命令而不是第十三个 workspace 成员（§60 的 12 份 manifest 巡检因此不必改写）。
`wp host manifest` 写**两**个文件：Chrome 的记录，和记录所指向的 wrapper 脚本——Chrome 不传参数，
所以 `--db` 与 `--project` 只能烘焙进脚本，路径逐单引号转义（macOS 上带空格的路径是常态）。
`serve` 期间 stdout 属于协议，人类可读的那一行走 stderr：这是全 CLI 唯一一处这样的命令。

### 二、扩展这一侧：`apps/extension/`（MV3，零依赖、零构建步骤）

`manifest.json` / `background.js`（service worker + §35 前置校验）/ `content.js`（读页面、写输入框）/
`popup.html` + `popup.js`（全部中文界面，无外部资源、无内联脚本）。
校验放在 native messaging **之前**，也就是 §35 要求的那一侧：协议版本、origin 白名单、已知 command、
每个 command 的 payload 形状（未知字段拒绝、role 只认 user/assistant、`at` 不是 RFC3339 就拒绝而不是猜）、
project 字段形状，以及序列化后 245760 字节的硬上限。
generic web 的"用户授权"是弹窗那一次点击签发的一次性同意凭据：60 秒、用过即废、只存在 worker 内存里，
worker 一睡就没了；content script 从不自己发起请求，页面也够不着 `chrome.runtime.onMessage`
（没开 `externally_connectable`），所以每一条请求都能追回到一次人的按键。
填输入框时只写文本 + 派发 `input` / `change`：**不**派发 Enter、不 `requestSubmit`、不点发送键；
宿主即使回了 `send:true` 也照 §7.8 忽略并在状态行说明。读不出角色的 DOM 块跳过并报数量，
输入框已有草稿时追加而不覆盖。

### 三、两边对接时抓出来的两处

1. **§35 的 origin 检查有两副面孔。** 四个动词里有两个字面意义上没有页面可报：`capture`（读哪个页面）
   与 `inject`（往哪个页面打字）必须报白名单里的 `https://<host>`，而 `hello`（自检）与 `open`（把某页带到前台）
   不经手页面材料，扩展只能报自己的 `chrome-extension://<id>`——那枚 id 正是 Chrome 按 `allowed_origins`
   已经核过的东西，所以这不是让宿主相信一句新的自述；`open` 的目标 URL 另有它自己的白名单检查。
   扩展原先用的 `codlas-extension://popup` 是一个 Rust 一侧永远解析不过的自造 scheme。
2. **`capture` 的成功答复被扩展当成"不认的动作"**：`{action:"captured"}` 是结果不是指令，
   但 `applyHostAction` 的兜底分支会给它写一句"V1 的扩展只认 openTab 与 fillComposer，已忽略"——
   正常路径上的一句假话。

两处都就地修订进 ADR-031（Status 标 amended），`apps/extension/README.md` 的协议表与安装章节同轮改写：
它原先描述的是"打印一份 json 让你粘贴"，而实际命令是写两个文件并打印 `mkdir` / `cp` 两行。

### 四、§58 的 E2E 那一行第一次真跑通，§53 顺路被看见一次

`cli_e2e` 第 20 条 `a_browser_session_round_trips_through_the_native_host` 用**自己重写的一份编解码**驱动
真 `wp host serve`——不复用 crate 的 codec，共享 helper 会替宿主隐瞒它自己发出去的字节。
链路走完 Browser → Extension → Native → Event → State → Project Map → Select Issue → Context Pack →
Browser Prepare，并在同一轮撞上 §53：这个 fixture 项目 `allowCloudModel=false`，所以 `--to chatgpt` 的包
被**整包拒绝**，链路上用的是"不声明去处"的那份包；把包搬进浏览器的人是 §7.8 里那个动手的人，不是策略的漏洞。
最后一帧之后又送一帧坏 origin，拒答之后 `state show` 的 Issue 数没变——通道的失败不改变项目事实。

### 五、测试与构建

`cargo test --workspace` = **279 passed / 0 failed / 1 ignored**（Keychain 仍需 opt-in；上一轮 265），
`cargo build` 零 warning。四层：库（golden + 各 crate 套件，新增 `native_messaging` **13** 条）、
真二进制（`cli_e2e` **20** 条）、桌面壳视图（`views.rs` 12 条）、扩展侧只有 `node --check`——
V1 不为它装 JS 测试框架（§60 的成本规则），协议的断言全部落在 Rust 那一侧。
本轮另外三处手工验证，都是跑出来的不是推出来的：`popup.html` 的 12 个 id 与 `popup.js` 的
`getElementById` 逐个对上（缺失 0、冗余 0）；`wp host manifest` 在临时库上落出的记录与 wrapper 逐字看过
（绝对路径、`0700`、`--db` / `--project` 烘焙进脚本）；真 `wp host serve` 收一帧
`chrome-extension://…` 的 `hello`，回了能力清单与 `autoExecute:false`。

### 六、V1 边界内还欠着的（清掉一件，还剩一件 + 一次手工核对）

1. **§38.5 入站方向的 MCP 采集**：工作台主动去读 agent 那边的会话。形状、七个动词的映射与触发条件在 ADR-028。
   这是 V1 账上最后一件有名字的代码欠项。
2. **在真 Chrome 里装一次这个扩展**：`wp host manifest` 落出的那两行 `mkdir` / `cp` 走到底，
   看 `capture` 在一个真的 ChatGPT / Claude 会话上采到几轮、`inject` 把包写进了哪个输入框。
   这与 ADR-029 给桌面壳留的那条（webview 渲染手工核对）是同一类残留。

不算欠账：§14 第 9 项"发送进另一个 Agent"这一轮有了真实的**输送**通道（`open` / `inject` 到浏览器，
永远 `send:false`），而 `AgentAdapter::inject()` 对已交付适配器仍返回 `Ok(None)`——ADR-031 明写这一次是
结论不是缺口（CLI 进程退出后不留通道，要注入就得先有一个常驻进程，那是 §37 的 `AUTO_EXECUTE` 与 禁止 10 的门口）。
§40 的中文分词仍是记录在案的限制；语义/向量检索、云同步、插件 adapter、智能偏航、实体图谱归 AEP-002…005；
§5 技术树里的 `crates/agent-runtime` 与 `packages/*` 没有一条 V1 条款要求它们。

### 七、§62 七门现状

Continuity ✅ · Fidelity ✅（ADR-030）· Traceability ✅ · Drift ✅ · Recovery ✅ ·
Degradation ✅（`fallback`、"没输入就拒绝"，本轮再加上"一条坏帧不结束会话"）·
**Context ✅**——上一轮那个 ◐ 缺的正是把包"送进去"的那条写面，这一轮它有了：`inject` 填进输入框，
代发永远不做。

### 八、回写

`PROJECT_MASTER` §20 四块（V1-D 三条新要点 + 对接修订一条 + E2E 一条 + 测试计数 265→279、
CURRENT DEVELOPMENT STATE 把 "Browser Extension + Native Messaging Owed" 换成 "Shipped (ADR-031)"、
BLOCKER 换成"只剩入站 MCP + 一次真浏览器手工核对"、NEXT TARGET 换成 ADR-028 那一件）、
§26 的 V1-D 仍是 ◐，但欠的东西换了名字；`docs/adr/ADR-总览.html` 新增 ADR-031 并就地修订其 origin 检查
（本页 23 条），`docs/adr/README.md` 加索引行、计数改 23、"下一条从 ADR-032 起"、
ADR-007 的"尚未构建"由 ADR-031 结掉；任务报告：`V1第四轮开发报告.html`。

---

## 2026-10-06 · V1-E 桌面壳落地，八屏齐了；§14 第 7 项补上那只手（ADR-029 / ADR-030 各修订一次）

同一天里的第三轮。上一轮只改了账，这一轮把账上第一件——桌面壳——做完，并且在做的过程中又找到一个真缺陷。

### 一、壳本身：`apps/desktop`（Tauri 2）

十二个命令，十一个是只读视图，唯一的写是 `apply_pending`，它逐条走 §7.4 早就存在的
`accept_change_proposal` / Inbox 判定。db 路径与当前项目都由 `vibe_cli::app::App`（CLI 自己的 runtime）给出，
窗口与终端不可能各有一套项目状态（禁止 4 / 禁止 7）。lib/bin 切开：视图构造器在 `src/views.rs`（lib），
`#[tauri::command]` 壳在 `src/commands.rs`（bin），`src/main.rs` 只剩注册。
于是 `apps/desktop/tests/views.rs`（12 条）不启动 webview 就能驱动整条读路径。

第一屏就是用户拍定的流程：**变动（diff + 等你决定的条目）→ 决定哪些生效 → 项目地图**，
两侧再摊开总览、决策、演进（AEP）、时间线、检索、上下文预览、健康与备份。
§21 给 V1-E 列的八个屏幕（Overview / Timeline / Project Map / Decisions / Evolution / Drift·Inbox / Search
+ 本轮定的第一屏 diff）全部有对应命令与视图构造器，所以 §26 的 V1-E 第一次名实相符地打上 ✅。

写这一轮的时候对着 §21 逐屏点过一遍：上一轮"桌面壳做完"的说法其实是 7/8——总览、决策、演进三块没有屏幕。
这一轮补齐，且三块都是**薄展示**：`overview_view` 只是把已接受事件折叠出来的记录按类型数一遍，
并写明"这些数字不是第二套状态"；`decisions_view` 明确"这一屏不提供编辑"；
`evolution_view` 把 AEP 显示成还没被触发的提案（`dormant`），而不是在建工程（禁止 8）。

### 二、§14 第 7 项：缺的那只"手"，以及缓存第二次咬人

上一轮的清单里写着"缺手势不缺逻辑"。这一轮补手势：偏航屏与检索屏每行一个「带它去上下文 →」，
上下文屏多一个 `kind:id` 聚焦框，`ContextView.focus` 让包自己说聚焦在哪一条。三条约束是设计本身：

- **引用就是意图**。点了之后不需要再写一句问话，壳按 CLI 完全相同的方式把它写成 `context for issue:I-1`；
  两者复用同一个 `vibe_cli::context::parse_ref`（把 `parse_ref` 从 CLI 私有改成 pub，而不是抄第二套引用语法）。
- **没问话也没点选＝拒绝**，不是一份空包（§62 不允许工作台替你猜）。
- 编译包之前先做一次 `Reducer::project_state` 写穿。**这是 ADR-030 的第二种后果**：§42.1 的"用户显式选择"
  是经 `repos.entities` 解析的，而那些行是折叠的缓存；刚 `wp event` 追加一条 Issue、还没跑过 `wp map` 时，
  `wp context plan --select issue:I-1` 会答"not found in this project"。CLI 的 `context dispatch`、
  `wp-mcp` 的 `context_pack`、桌面壳的 `context_view` 各补一行——map 编译与快照本来就这么写，不是第二套状态。

顺带把 `--select` 在 CLI 层的覆盖补上（`a_selected_record_travels_at_the_top_of_the_ladder`：
priority 1 / kept / 少了 kind 就拒绝）。此前这个 flag 只有引擎层测试，CLI 的 flag→request 那条线没人守。

### 三、测试与构建

`cargo test --workspace` = **265 passed / 0 failed / 1 ignored**（Keychain 仍需 opt-in），`cargo build` 零 warning，
`cargo build -p wp-desktop --bins` 通过。三层：库（golden + 各 crate 套件）、真二进制（`cli_e2e` 19 条）、
桌面壳视图（`views.rs` 12 条）。webview 的像素仍是手工核对——ADR-029 的后果里明说了"12 条测试通过"
不等于"渲染正确"。

### 四、V1 边界内还欠着的（这一轮清掉两件，还剩三件）

1. **V1-D 浏览器扩展 + Native Messaging**（§34 / §35 / §38.2 / §38.3 / §58 E2E）：`open` 与 `inject`
   至今只有 trait 里的默认 `Ok(None)`。这一轮才是它们第一次真实存在的时候，§35 的五条校验
   （origin / command / payload / size / project scope）是这一轮的验收点，不是可选项。
2. **§38.5 入站方向的 MCP 采集**：工作台主动去读 agent 那边的会话。形状与触发条件记在 ADR-028。
3. **§14 第 9 项"发送进另一个 Agent"**：payload 与复制兜底都有，缺的仍是真实传输，与第 1 项同轮。

不算欠账的两条：§40 的中文分词是**记录在案的限制**（`unicode61` 不切 CJK，测试
`full_text_matches_a_whole_token_not_a_piece_of_a_chinese_run` 钉着；V1 只要求 FTS5），
超出边界的语义/向量检索、云同步、插件 adapter、智能偏航、实体图谱分别归 AEP-002…005；
§5 技术树里的 `crates/agent-runtime` 与 `packages/*` 没有一条 V1 条款要求它们。

### 五、§62 七门现状

Continuity ✅ · Fidelity ✅（ADR-030 之后创建事件带整条记录）· Traceability ✅ · Drift ✅ · Recovery ✅ ·
Degradation ✅（`fallback` 与"没输入就拒绝"都在测试里）· **Context ◐**——能编译、能在桌面上点一行聚焦、
能被 agent 通过 `wp-mcp` 读走，缺的仍是把包"送进去"的那条写面（V1-D）。

### 六、回写

`PROJECT_MASTER` §20 四块（V1-E 三条新要点 + 测试计数 + NEXT TARGET 去掉 §14 第 7 项那件债）、
§26 的 V1-E 从 ⏳ 改为 ✅（V1-D 仍是 ◐，两半都欠着）；
`docs/adr/ADR-总览.html` 就地修订 ADR-029（那只手的形状与"点一行＝意图"）与 ADR-030（缓存第二次咬人，
以及"哪些读依赖显式物化"），索引行、meta、README 三处计数同步（e2e 19 / views 12）；
任务报告：`V1第三轮开发报告.html`。

---

## 2026-10-06 · V1 缺口清点：§38.5 的另一半改记为"还欠着"（ADR-028）

这一节不动产品逻辑，只做两件事：把上一轮的判断错误改正过来，以及把"V1 还剩什么"对着文档核一遍。

### 为什么要改

用户的实际目标是"软件通过获取 agent 那边和用户的聊天内容，以此为根据更新软件的进度面板"。
§38.5 的"MCP 基础接口"有两个方向：**别人（agent）通过 MCP 读我的工作台**，和**工作台通过 MCP 去读别人**。
第二轮把这一条整体判成完成，靠的是前者（`crates/mcp-server` + `wp-mcp`，ADR-026），后者一行代码都没有。
用户拍定按"还欠着"记，于是 ADR-028 把欠的部分、触发条件、届时的形状（`McpClientAdapter` 七个动词怎么映射）
和一条由 §60 依赖禁令推出的硬约束（只能 stdio 阻塞，不许为它引 async runtime）写死；
§26 的 V1-D 行从 ✅ 退回 ◐，ADR-016 / ADR-026 各就地修订一次。

### V1 边界内、确实还没做的（四件里清掉一件，还剩三件）

1. **V1-E 桌面壳**（§26 下一站 / Stage G 的界面腿）：`apps/desktop` 不存在。第一屏的流程用户已经拍定：
   **渲染 diff → 用户浏览哪些模块发生了变动 → 决定哪些变动生效（§7.4 确认即锁定）→ 再进 map**。
2. **V1-D 浏览器扩展 + Native Messaging**（§34 / §35 / §38.2 / §38.3）：`open` 与 `inject` 到现在为止
   只有 trait 里的默认 `Ok(None)`，没有任何一个真实实现——§58 的 review 之所以带负控，正是为了不把这个
   空档说成能力。
3. **§38.5 入站方向**：见上，ADR-028。
4. ~~**`wp context prepare` 的兜底文案**~~ —— **本轮已清掉**。它原来打的是 `PreparedPayload.copy_fallback`
   旁边那句写在使用处的话（`apps/cli/src/context.rs`），而不是 §58 适配器合同的 `fallback()`。
   现在句子只有一个出处：`vibe_ingestion::fallback_for()`，trait 的默认实现与 CLI 都走它，
   `--json` 在 `copyFallback` 之外多带一个 `fallback` 对象，人类输出会把 `--out` 的文件名说出来
   （兜底要是说不清要携带哪一份 payload，它的 `payloadPath` 字段就是空着说谎）。
   ADR-025 当初许的是"一行改动、行为无差别"，实测不止，所以那条 ADR 就地修订。

### 半成品（不是遗漏，是被人的一半）

- **§14 第 7 项"用户点击某个 Issue"**：机器动作齐了（`wp map` → `wp drift` → `wp context build --select kind:id`），
  缺的是一只可点的手。归 V1-E。
- **§14 第 9 项"成功准备到另一个 Agent"**：`prepare` 出 payload、复制兜底可用；"发送"按 §7.8 与 §62
  Degradation 本来就必须是人的动作。真正缺的是任何 adapter 的 `inject`。归 V1-D。
- **§40 L2 的中文检索**：`unicode61` 不把 CJK 切词，一整段中文是一个 token，所以查 `"上下文"`
  命中不了 `"上下文引擎保持词法实现"`（实测：sqlite3 3.51 下 `MATCH '状态'` = 0 行，整串 = 1 行）。
  这不是新发现的窟窿——`crates/search/tests/retrieval.rs:372`
  `full_text_matches_a_whole_token_not_a_piece_of_a_chinese_run` 已经把它钉成记录在案的限制，注释里
  写明了出路（trigram）。V1 的条款只要求 "SQLite FTS5"，所以按字面它是完成的；要不要补中文分词是
  产品决定，bigram / trigram 都不需要新依赖，也不需要违反 §60。

### §62 七门现状

Continuity ✅ · Fidelity ✅ · Traceability ✅ · Drift ✅ · Recovery ✅ · Degradation ✅（`fallback` 是唯一
不能不答的动词，契约测试带负控）· **Context ◐**——能编译、能复制、能被 agent 通过 `wp-mcp` 读走，
缺的是把包"送进去"的那条写面（V1-D）。

### 超出 V1 边界的，不算欠账

语义/向量检索 → AEP-002；本地→云同步 → AEP-001；内置→插件 adapter → AEP-003；基础偏航→智能偏航 → AEP-004；
扁平状态→实体/架构图谱 → AEP-005。另外 `PROJECT_MASTER` §5 的技术树里画了 `crates/agent-runtime` 和
`packages/{schemas,adapter-sdk,shared,ui}`，但 §61 / §62 / §14 没有一条要求它们，而 §5 自己写着
"未来允许更换具体技术实现"——把树上的方框当 V1 欠账，就是 禁止 8 的提前实现。

### 一条更正（不改历史，只追加）

提交 `54cdb5d` 的信息里写契约测试"放过了一个声称自己答不出动词的 adapter"，这句是反的。
真实缺陷是：**默认 trait 方法让一个什么都不做的 adapter 也能通过 review**——它没声称任何能力，
却拿到了通过；负控（把一份已知违例喂进同一套 review，并要求它报出违例）才是让这份 review
说得出人话的东西。RECORDS 2026-10-05 第二轮一节与 `V1第二轮开发报告.html` 已带正确版本，
本轮另起一条更正提交说明它，不去改写已推送的历史。

---

## 2026-10-05 · ADR 介质变更：19 份 .md 汇总为一个 HTML 页

不改代码、不改决定，只改载体（用户指令）。

- **做了什么**：`docs/adr/ADR-001…027` 共 19 份 markdown 逐条渲染进
  `docs/adr/ADR-总览.html`（单页、自包含 CSS、每条一个锚点、ADR/AEP 交叉引用变成页内跳转），
  然后删除全部 `.md`。`docs/adr/README.md` 保留为指针页：索引表 + 缺号说明 + 追加新条目的步骤。
- **为什么这样切**：ADR 的数量已经多到在 markdown 里读不动（19 条约 57 KB，最长一条 4.6 KB 散文），
  而它的价值恰恰是"被人读到"。决定本身一个字没动——变的是承载它的文件。
- **忠实性怎么证的**：脚本把页面纯文本抽取出来（丢弃 markdown 语法字符、列表符号、行折叠），
  逐行核对 669 个内容行（≥24 个字母数字/CJK 字符的行）全部出现在页面里；
  `missing content lines: 0`；锚点 19 个、页内交叉链接 35 条、悬空 fragment 0、
  相对链接指向的文件全部存在。脚本是一次性的，未入库。
- **引用重指**：`V1第二轮开发报告.html` 16 处 `docs/adr/ADR-0NN-*.md` → `ADR-总览.html#adr-0NN`；
  `crates/core/src/ports.rs` 的文件头注释同改；`PROJECT_MASTER.md` §11 的 ADR 条目补介质约定
  （新增决定从 ADR-028 起，直接在页内追加，不再建 .md）。
- **任务报告的挂法**：该页第四节「任务报告」是每轮 HTML 报告的落点，已挂第一轮（指向本页记录）
  与第二轮 `V1第二轮开发报告.html`，之后每轮在此追加一行。
- **未做**：`docs/evolution/` 的 5 份 AEP 仍是 markdown（用户只要求汇总 ADR）；
  页面是生成的，但此后**手工维护**——原 .md 已删，不再保留再生成路径。
- **恢复原文**：`git show 54cdb5d:docs/adr/<文件名>.md`（汇总前的提交，已在远端）。
- **紧随其后**：同一批清点里补了 §38.5 的第二读法，产出 ADR-028（见上一节）。

---

## 2026-10-05 · V1 第二轮：把第一轮留下的半成品与缺口做完（Stage D/E/F/G/H + §38 MCP）

### LAST COMPLETED

第一轮把 §7.2 那条链跑通了，本轮把 V1_DEVELOPMENT 里**未完成与完成一半**的八项补齐，
没有新增产品概念（§19 禁止 8）：

1. **`wp evidence <kind:id>`（§17 / §45，Stage E 验收）**——一条记录引用的证据终于有读取入口。
   之前 `EvidenceRepository::for_subject` 已经写好了，但只有 golden 测试能调用它；
   现在人能读，机器 `--json` 也能读。
2. **Backup / verify / restore（§55 备份侧 + §62 Recovery，Stage H）**——`wp backup
   create|status|verify|restore`。用 SQLite Online Backup API **两个方向都走同一条路**
   （`fs::copy` 会冻结未 checkpoint 的 WAL；换文件等于换掉一个本进程还开着的库）。
   报告里带 `PRAGMA integrity_check` 的原话；`restorable(known_version)` 挡住比本 build
   更新 schema 的文件；restore 需要 `--yes`，拒绝把自己恢复到自己，并且先把当前库快照一份
   （快照失败只 warn——Recovery 不能被安全网卡住）。秘密值按 ADR-017 不在库里，所以备份只带
   `secret_ref`。见 ADR-023。
3. **Adapter Health（Stage H + §62 Degradation）**——`AdapterRepository`（第十一个端口）＋
   `AdapterHealth { ok | unavailable | degraded | failed }`。表 `adapter_states` 早就在 schema v1
   里（§49 冻结），冻的是表不是词，本轮把词定下来并让 pipeline 每次运行都留一句判词。
   `unavailable ≠ failed` 是这条记录的关键：来源不在这台机器上不是 bug。`wp adapters` 读它。见 ADR-021。
4. **§39 监听事件**——`wp watch [--dir] [--repo-id]` 一次 pass：分支、脏文件集合、以及相对
   上一次 pass 出现/改动/消失的文件。V1 没有守护进程，"watch" 的定义就是**两次读取相减**，
   上次快照存在 `settings` 一行里。新增 `GIT_BRANCH_CHANGED` / `GIT_STATUS_CHANGED` 两个事件类型
   （藏进 `GIT_COMMIT` 会把"工作树变脏"说成"有一个 commit"）；文件三种变化共用 `FILE_CHANGED`
   ＋ payload `change ∈ {created, modified, deleted, baseline}`。projector 把两类 git 信号
   与 `TEST_*` / `BUILD_*` 一起放进"记录但不改状态"的分组——**监听是证据，永远不是完成**（§19）。见 ADR-022。
5. **§59 / §60 预算（Stage H Performance）**——`crates/state-engine/tests/budgets.rs`（6 条）＋
   `crates/search/tests/budgets.rs`（2 条）。只有两条计时断言：纯折叠 1200 事件 ≤250 ms
   （实测 ~40 ms），整条链含 SQLite ≤20 s（实测 ~6 s，是 hang/O(n²) 的 tripwire 而不是 benchmark，
   失败时打印分段耗时）。其余全部是**工作量断言**：读了几行、拉了几条事件、扫了几个候选。
   §60 的成本规则最终落在 `no_manifest_in_the_workspace_reaches_a_network_or_a_model`——
   扫 10 个 crate ＋ apps/cli 共 11 份 Cargo.toml，禁网络/模型/async 依赖，rusqlite 只允许出现在
   `crates/storage`。见 ADR-027。
6. **§52 L3 近似重复：只提示，不删除**——`crates/ingestion/src/near_duplicate.rs` ＋
   `wp duplicates [--conversation] [--threshold] [--window] [--limit]`。词法 Jaccard，默认阈值
   0.8，窗口 240，CJK 按字符 bigram 比较（一整串汉字在 unicode61 里是一个 token，否则两条中文
   复述永远得 0 分）；`similarity_upper_bound` 先按集合大小剪掉不可能达标的对，把平方级压住。
   每条提示自带 `conversationId` 与那句 note（"两条都还在库里，没有合并、删除或改写"），
   所以提示可以按会话复查，也可以被证伪。语义版留给 AEP-002。见 ADR-024。
7. **§58 adapter 契约补齐 `inject` / `fallback` ＋契约测试**——七个动词现在全在
   `AgentAdapter` 上；写侧默认 `Ok(None)`，`fallback` 是唯一**不能不答**的动词，返回
   `Fallback { adapterId, instruction, deepLink, payloadPath }`。`crates/ingestion/tests/contract.rs`
   对每个内置 adapter 跑同一份 review，并且 review 有反向对照（`an_adapter_may_not_answer_a_verb_
   it_did_not_advertise`）——没有负控，默认方法体会让一个空 adapter 也看起来合规。V1 不发布任何
   真会往 agent 里写字的 adapter（§37 从不声明 `AUTO_EXECUTE`）。见 ADR-025。
8. **§38.5 MCP 基础接口**——新 crate `crates/mcp-server` ＋ `wp-mcp` 二进制。stdio
   换行分隔 JSON-RPC 2.0：`initialize` / `ping` / `tools/list` / `tools/call` ＋
   `notifications/initialized`（必须静默）。六个工具全是读：`project_state`、`project_map`、
   `project_timeline`、`search`、`open_evidence`、`context_pack`，每一个都走 CLI 已经在用的
   service 与 `Repositories` 端口——第二扇门，不是第二套实现。`context_pack` 过同一个 Policy
   Filter、留同一条 §54 audit。项目解析顺序与 `wp` 一致，两个项目又不指名时报错并**列出两个
   名字**而不是猜。见 ADR-026。

### FILES CHANGED

```text
apps/cli/src/backup.rs                  新：§55/§62 备份四个子命令（含 --yes 与自恢复守卫）
apps/cli/src/app.rs                     App 增加 maintenance 句柄（不属于 Repositories）
apps/cli/src/views.rs                   +Evidence 子命令 +Duplicates 子命令（约 180 行）
apps/cli/src/capture.rs                 +Watch 子命令；git/file 两条 pass 接到 CLI
apps/cli/src/main.rs / lib.rs           注册 backup 模块与命令
apps/cli/tests/cli_e2e.rs               11 → 17 条：evidence / backup / adapters / watch(×2) /
                                        near-duplicate / 追加一轮 adapter 判词
crates/core/src/ports.rs                +AdapterRepository（record_state/get_state/list_states）
                                        +DatabaseMaintenance（backup_to/integrity_check/inspect/
                                         restore/schema_version）+BackupReport::restorable
crates/domain/src/agent.rs              +AdapterHealth 四值 +AdapterState +Fallback（§58 最后动词）
crates/domain/src/event_catalog.rs      +GIT_BRANCH_CHANGED / GIT_STATUS_CHANGED，进 ALL
crates/storage/src/conversation.rs      AdapterRepository 实现；recent_messages 补 rowid DESC  tie-break
crates/storage/src/backup.rs            新：在线备份 / 校验 / 恢复，两个方向同一 API
crates/storage/src/lib.rs               注册 backup 模块
crates/storage/src/schema.rs            （沿用 v1 已冻结的 adapter_states 表，无新 migration）
crates/state-engine/src/projector.rs    两类 git 信号进入"记录不改状态"分组；证据类型归 Git
crates/state-engine/tests/budgets.rs    新：6 条 §59/§60 预算，含 workspace manifest 扫描
crates/search/tests/budgets.rs          新：2 条检索工作量预算（不随库变大而变贵／只要所要的）
crates/ingestion/src/adapter.rs         §58 七个动词齐；inject / fallback 默认实现
crates/ingestion/src/near_duplicate.rs  新：L3 词法近似重复提示（7 条单测）
crates/ingestion/src/pipeline.rs        每次运行写 AdapterState；游标失败 → degraded
crates/ingestion/src/git_facts.rs       +watch_workspace（分支/脏集合与上次 pass 相减）
crates/ingestion/tests/health.rs        新：5 条判词（ok/unavailable/degraded/failed/覆盖）
crates/ingestion/tests/watch.rs         新：6 条监听（含"状态一模一样"）
crates/ingestion/tests/near_duplicate.rs 新：7 条提示与只读性
crates/ingestion/tests/contract.rs      新：10 条 §58 契约 review，含负控与手写 fallback
crates/mcp-server/**                    新 crate：lib.rs（协议与调度）/ tools.rs（六个读）/
                                        main.rs（wp-mcp）/ tests/mcp.rs（12 条，含真二进制 spawn）
Cargo.toml                              workspace 加 members 与 vibe-mcp-server 依赖
docs/adr/ADR-021..027                   新：七个决定
docs/adr/ADR-016                        修订：MCP 从"未做"移到"已做"
docs/adr/ADR-020                        修订：两个表面（wp 给人 / wp-mcp 给 agent），规则不变；e2e 11 → 17
docs/evolution/AEP-002                  补：§52 L3 已有词法版本，触发条件不变
docs/adr/README.md                      索引补 ADR-021…027，并把两条 amended 标出来
```

### TEST STATUS

`cargo test --workspace` = **247 passed / 0 failed / 1 ignored**（上一轮 187/0/1）。
ignored 仍是需要显式 `VIBE_SECURITY_ALLOW_KEYCHAIN=1` 的 Keychain 测试。构建零 warning。

新增分布：storage backup 6、state-engine budgets 6、search budgets 2、ingestion health 5、
ingestion watch 6、ingestion near_duplicate 7、ingestion contract 10、mcp-server 12、
CLI e2e +6。

### DECISIONS MADE

- ADR-021：adapter 健康只有四种结局，且按 adapter id 存一行、最后一次运行覆盖前一次；
  历史属于事件日志与 §54 audit，不在这里再造一个 append 表。
- ADR-022：监听信号是事件；文件三种变化共用 `FILE_CHANGED` ＋ payload 动词，git 两个信号
  各得自己的类型；reducer 不为它们改任何 Project State。
- ADR-023：备份与恢复都走 SQLite 的在线备份 API；维护能力是**独立端口**，不是 `Repositories`
  的第 12 个字段——项目逻辑没有一处需要把数据库当文件读。
- ADR-024：L3 只做词法近似、只出提示；提示带会话 id 与自我声明的 note。顺带修了一个真缺陷：
  `recent_messages` 的窗口只按 `observed_at` 排，批量导入的每行时间戳相同，于是同库两次扫描
  可能给出不同提示——加 `rowid DESC` 后窗口确定。
- ADR-025：`fallback` 是必须能答的动词；契约 review 以"能力诚实"为核心并有负控。
- ADR-026：MCP 只有 stdio、只有读；工具名不得写成写动词的样子；三类失败分清
  （协议错误 / 客户端参数错误 / workbench 的 `isError` 结果）。
- ADR-027：预算优先是可复现的工作量，计时只留两条并附上实测值；§60 用依赖声明钉死。
- 修订 ADR-016 与 ADR-020 而不是新开一条：MCP 落地让"V1 只有一个表面"这句过时了，但它没有改变
  决定本身——两个表面（`wp` 给人、`wp-mcp` 给 agent）仍然都不许藏逻辑，界面仍然是展示层。
  ADR 记录的是今天为真的东西，所以就地修订并标 `amended`，不叠第二份说法。

### AEP IMPACT

- AEP-002（FTS → 混合检索）：新增一段说明——§52 L3 的词法版已经存在，形态（候选对＋分数＋
  note＋没有写路径）就是语义版要接的形状；触发条件不变，仍然是"用户自己项目里错过的证据"。
- AEP-003（内置 → 插件 adapter）：`contract.rs` 的 review 现在可以对外部 adapter 复用，
  这是插件化的前置形状；本轮仍未提前做插件加载（禁止 8）。
- AEP-004（词法规则 → 智能 drift）：未触发。
- AEP-001（本地 → 云同步）：未触发；`adapter_states` 依旧按 adapter id 全局一行，没有加项目维度。
- AEP-005（平面 state → graph）：未触发；`entity_links` 仍是 L3 关系层的地基。

### KNOWN ISSUES

1. **`wp watch` 是手动一次 pass**，不是常驻监听。没有 notify、没有定时器——V1 的验收只要求
   "改动能成为证据"，一条守护进程要命的生命周期与配置文件本轮没人需要（ADR-022 的 3）。
2. **MCP 只有 stdio，没有 prompts / resources / sampling / 工具注解**。§38.5 说的是"基础接口"，
   超出的部分等真实客户端要求再说（ADR-026）。
3. **`inject` 对所有内置 adapter 仍返回 `None`**。契约齐了，能写字的传输一个都没发布：
   §7.8 发送是人的动作，浏览器扩展与真 app 注入是下一轮的事。
4. **备份不含秘密值**，这是设计而不是遗漏（ADR-017）；反过来说：换机器时 `secret_ref` 指向的
   文件必须一起搬，restore 之后策略仍然要求 Keychain 或那个 0600 文件在场，否则读秘密会失败。
5. **性能计时在 debug 构建上跑**（`profile.dev.debug = 0`，无 LTO）。相对关系稳定，绝对值
   不代表 release；所以两条计时预算都留了很大余量，真正的门是工作量断言。
6. **中文检索的分词盲区与 feature_creep 刷屏**（第一轮遗留）仍在：前者等 trigram 或 AEP-002，
   后者等界面聚合。本轮没顺手改，因为两者都不在 V1 验收项上。
7. **V1-E 桌面界面与 V1-D 浏览器扩展仍未做**，理由与边界见 ADR-016（本轮已修订）。

### DRIFT CHECK

- `crates/*/Cargo.toml` 与 `apps/*/Cargo.toml` 的依赖扫描已经变成测试
  （`no_manifest_in_the_workspace_reaches_a_network_or_a_model`），本轮新增的 `crates/mcp-server`
  已在扫描名单里：它没有网络依赖、没有模型依赖、没有 rusqlite，rusqlite 仍然只在 `crates/storage`。
- 对自己的新代码跑自己的规则：`wp watch` 一轮之后 `wp state show --json` 的 `stateHash` 不变
  （`watch_signals_fold_into_no_project_state_at_all` 钉住），监听写出的每一条都能被
  `wp timeline` 指认——证据腿（§17）和状态腿（§20）没有互相冒充。
- `wp duplicates` 的只读性有两条独立证明：库内消息计数在扫描前后不变（模块没有写路径），
  以及同一批材料再导入一次仍报同样数量的 duplicate 而不是更少（CLI e2e）。
- MCP 的六个工具全部经端口读；`context_pack` 走 `ContextCompiler`，测试断言 §54 audit 里
  出现了这条读取——"通过管道读项目"没有变成"读项目而不留痕"。
- 本轮没有把任何逻辑从 crate 搬回 CLI；CLI 只做参数解析、调 service、排版输出（禁止 4）。

### NEXT IMMEDIATE TARGET

V1-E Workbench：Tauri 2 桌面壳承载已经编译好、也已经钉上测试的视图
（Overview / Timeline / Project Map / Decisions / Drift + Inbox / Search / Context Preview），
再加上本轮新出现的三个面板原料：`wp adapters` 的判词、`wp duplicates` 的提示、`wp backup status`
的完整性结论。界面自己不解释事件、不拼状态，一切读回现有端口。
之后是 V1-D 的浏览器扩展 + Native Messaging——那才是 ADR-025 里 `inject` / `open` 第一次
真正被实现的时刻。

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
