# Codlas 采集 · 浏览器扩展（V1-D Capture 的前端半边）

Chrome MV3 扩展，对应 `V1_DEVELOPMENT.md` §34（Browser Extension）与 §35（Native Messaging）。
它做两件方向相反的事：

- **交出去**：把当前网页里的 AI 会话（消息轮次、选中文本所在的那几轮）作为**证据**交给本地工作台；
- **收回来**：按工作台的指令打开目标网页，把 Context Pack 的文字**写进输入框**。

边界与 §34 一致：扩展**没有本地数据库**、**不发任何网络请求**（代码里没有 `fetch` / `XMLHttpRequest`，
也没有远端代码，CSP 保持 MV3 默认，没有 `unsafe-eval`）、**不判断项目状态**（禁止 9：聊天记录只是证据，
最终的项目记忆在 Rust 核心那边折叠出来）。扩展只递材料和执行命令，一切解释归核心。

文件：`manifest.json` / `background.js`（service worker + 校验）/ `content.js`（读页面、写输入框）/
`popup.html` + `popup.js`（全部界面，中文，无外部资源）/ `README.md`。零依赖、零构建步骤。

---

## 一、装载扩展

1. Chrome 打开 `chrome://extensions/`；
2. 右上角开启**开发者模式**；
3. 点**加载已解压的扩展程序**，选择本目录 `apps/extension/`；
4. 记下卡片上的**扩展 ID**（形如 `abcdefghijklmnop...`），装宿主要用它。

没有图标文件，列表里会是默认灰色图标，不影响功能。

## 二、装 Native Messaging host

Host 名字固定为 `app.codlas.workbench.host`，由同一个仓库里的 Rust 半边提供（`wp host`，`wp` 就是
本仓库构建出来的那个工作台 CLI）。先按上一节装载扩展、记下扩展 ID，然后跑：

```sh
wp --db /绝对路径/workbench.db --project <项目 id> host manifest --extension-id <扩展 ID>
```

它在 `<db 所在目录>/host/`（`--out` 可改）里写两个文件：

- `app.codlas.workbench.host.sh` —— Chrome 真正启动的**包装脚本**，绝对路径、`0700`。Chrome 不给
  native host 传参数，所以「哪个库、哪个项目」都写死在它里面
  （`exec '<wp>' --db '<db>' --project '<项目 id>' host serve`）；
- `app.codlas.workbench.host.json` —— Chrome 的 host 记录，`path` 指向上面那个脚本，
  `allowed_origins` 是 `["chrome-extension://<扩展 ID>/"]`。

命令随后打印出 macOS 上该放这份 json 的目录和两行照抄即可的命令：

```sh
mkdir -p "$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
cp "<它写出的 json 路径>" "$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts/app.codlas.workbench.host.json"
```

- `--extension-id` 不给就报错并说明去 `chrome://extensions` 抄 ID；可以重复，多个扩展共用一个 host；
- `--print` 只把两份内容打到标准输出，不落盘；
- `path` 必须是**绝对路径**且脚本可执行，`allowed_origins` 里的 ID 末尾那个 `/` 不能省（Chrome 的写法）；
- 移动 json 别动脚本 —— 记录里的 `path` 是绝对路径；
- 其它平台：Linux 放 `~/.config/google-chrome/NativeMessagingHosts/`，
  Windows 按 Chrome 文档注册 `HKCU\Software\Google\Chrome\NativeMessagingHosts\app.codlas.workbench.host`
  并指向同样的 json 文件；
- 改完 json 不需要重启 Chrome；多装了一个扩展（新 ID）就重跑一次 `wp host manifest` 覆盖这份记录。

弹窗里第一个按钮**连接自检**发 `hello`，宿主没装好时它会直接把原因说出来。手工试协议不必经 Chrome：
给 `wp host serve` 的 stdin 写一条带长度前缀的帧即可，`--allow-origin https://…` 能临时加白名单
（注意它的标准输出只写帧，人类可读的那一行在 stderr）。

## 三、报文协议（`codlas-nm/1`）

帧：Chrome native messaging 自己处理 4 字节小端长度前缀。扩展用
`chrome.runtime.connectNative("app.codlas.workbench.host")`，一次请求一个端口，等到**唯一一条**
回答就关闭。JSON only。

请求（扩展 → 宿主）：

```json
{ "protocol": "codlas-nm/1", "origin": "https://chatgpt.com",
  "command": "hello|capture|open|inject", "project": "可选的项目 id", "payload": { } }
```

| command | payload |
| --- | --- |
| `hello` | `{}` |
| `capture` | `{ "source": "chatgpt_web"\|"generic_web", "conversationId": "页面自己的会话 id，找不到时用本标签页稳定兜底 id", "title": "页面标题或 null", "messages": [{ "role": "user\|assistant", "content": "文本", "messageId": "DOM 里读得到的稳定 id（有就带）", "at": "RFC3339（DOM 自己写了才带）" }] }` |
| `open` | `{ "url": "https://chatgpt.com/" }` |
| `inject` | `{ "text": "准备好的上下文包" }` |

回答（宿主 → 扩展）：`{ "ok": true, "data": { … } }` 或 `{ "ok": false, "error": "英文原因" }`。

`origin` 有两副面孔，宿主按 command 区分：`capture`（读的是哪个页面）和 `inject`（往哪个页面打字）
必须报白名单里的 `https://<host>`；`hello`（自检）和 `open`（把某个页面带到前台）不经手任何页面材料，
可以报扩展自己的 `chrome-extension://<id>`。后者不是让宿主“相信”扩展自报的身份 —— Chrome 早就按
host 记录里的 `allowed_origins` 决定过哪个扩展能够连上这个进程，`wp host manifest` 写的就是那份名单。
`open` 的目标 URL 另有它自己的白名单检查，两回事。

- `capture` → `data: { "action": "captured", "sourceId": "…", "messages": { "inserted": 2, "duplicate": 0 }, "events": { "appended": 1, "pending": 1, "candidate": 0 }, "nearDuplicates": 0 }`：
  同一会话推第二次必然全是 `duplicate`，那是 §52 的去重在替用户说话；扩展自己不数项目状态，只把这几个数打印出来；
- `open` → `data: { "action": "openTab", "url": "…" }`，**由 service worker 执行** `chrome.tabs.create` /
  `chrome.tabs.update`（只在 URL 完全相同时复用已有标签页，免得把用户正在读的那一页导航走）；
- `inject` → `data: { "action": "fillComposer", "text": "…", "send": false }`，由 service worker 找到目标
  标签页，让 `content.js` 填输入框；
- 除 `hello` 之外，每条回答都带 `project`：那正是这一条落进去的项目 id，`open` / `inject` 也带，
  好让弹窗说清「写的是哪个项目的输入框」；
- 宿主回答里出现的 `error` 句子在弹窗状态区**逐字显示**，不翻译、不改写。§35 的五道检查各自的拒绝分别以
  `origin rejected` / `command rejected` / `payload rejected` / `size rejected` / `project scope rejected`
  开头，所以一句拒答就能看出是哪道门把它拦下的。

校验在 §35 要求的位置，也就是**我们这边、native messaging 之前**：协议版本、origin 白名单、
已知 command、每个 command 的 payload 形状（含未知字段拒绝、role 只认 user/assistant、
`at` 不是 RFC3339 就拒绝而不是猜）、project 字段形状，以及序列化后 **245760 字节（240 KiB）** 的硬上限。
宿主自己也有一份同样的上限，以及它自己的五道检查。

## 四、Generic Web 怎么和 ChatGPT Web 区分

- **在册来源**（`chatgpt.com`、`*.chatgpt.com`、`chat.openai.com`、`claude.ai`、`gemini.google.com`）：
  `manifest.json` 里有永久的 `host_permissions`，`content_scripts` 一直在页面上等着；
- **其它任何来源**：不在册，service worker 一律拒绝，**除非**这次请求对应的一次弹窗点击刚刚给
  这个 origin 签发过一次性同意凭据（`mintConsent` / `consumeConsent`，60 秒内、用过即作废、只存在
  worker 内存里，worker 一睡就没了）。

区分点是**点击**：所有 `capture` / `inject` 请求都由弹窗里「采集选中」「导入本会话」「把包写进输入框」
这三个按钮发起，发起时才给当前标签页的 origin 盖章。Content script 从不自己发起请求，网页脚本也够不着
`chrome.runtime.onMessage`（没开 `externally_connectable`），所以「用户授权读取当前网页会话」这条
在代码里就是这枚凭据。`activeTab` 权限也是同一次点击才生效，这正好是 generic web 唯一能读进页面的通道。

协议里 `payload.source` 只有 `chatgpt_web` 与 `generic_web` 两个值，所以 Claude / Gemini 的页面在 source
上记作 `generic_web`；真实站点看请求的 `origin` 字段，那才是 §34「识别来源」的落点。

## 五、弹窗里有什么

| 按钮 | 做什么 |
| --- | --- |
| 连接自检 | `hello`，顺便把宿主报的项目 id 记下来 |
| 采集选中 | 读 `window.getSelection()`，只把**选区落在其中的那几轮**整轮消息发出去 |
| 导入本会话 | 按站点已知的选择器数出消息轮次发出去；认不出结构时照实说明，不编内容 |
| 打开目标网页 | `open`，URL 可在输入框里改（默认 `https://chatgpt.com/`，会记住） |
| 把包写进输入框 | 用户用 `<input type=file>` 自己选的本地文件，`FileReader` 读成文本 → `inject` → 填进输入框。这是 §34 的「准备输入内容」，不是高权限文件操作：路径由用户给，只当纯文本读 |

顶部会显示当前页面 origin；宿主任何一次回答里带了项目 id，就显示在「项目：」那一行，并在后续请求的
`project` 字段上带回去（扩展只是转递，不解释它）。

## 六、V1 永不发送

`send` 在扩展里只有一个取值。填输入框时：

- 只写文本，随后 `dispatchEvent` 一个 `input` 与一个 `change`，让站点自己的框架知道内容变了；
- **不**派发 Enter / `keydown`，**不**调用 `form.requestSubmit()`，**不**点任何发送按钮；
- 宿主即使回了 `send: true`，service worker 也只是把字写进框里，并在状态行说明这次被按 §7.8 忽略了。

理由就是 `PROJECT_MASTER.md` §7.8「发送永远是人的动作」：Build → Preview → Open Target → Prepare Input →
**User Confirm** → Send，扩展停在 Prepare Input 这一步，Confirm 那格留给人的手。§37 的能力模型里
V1 也不要求 `AUTO_EXECUTE`。

另外两处"宁缺毋造"：读不出角色的 DOM 块会被跳过并在状态里报数量；输入框里已有草稿时是**追加**而不是覆盖，
因为那是用户正在写的东西。

## 七、已知限制

- 站点改版会让选择器落空。这时扩展不会猜，而是把"试过哪些选择器、为什么没采到"照实打印；
  需要按新版 DOM 往 `content.js` 的 `SITES` 里加一条选择器。
- 单次 `capture` 最多 500 轮（超出留到下一次点击），240 KiB 字节上限。
- 只支持 http(s) 页面：`chrome://`、Web Store、PDF 查看器这类页面读不进去，会直接说明。
- 会话 id 兜底是扩展自己生成的标签页级 id（存在该页 `sessionStorage`，被禁用时退化为文档级），
  不是页面内容，因此不算编造。
