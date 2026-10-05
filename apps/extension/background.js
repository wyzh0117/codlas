/* Codlas 采集 — MV3 service worker (V1_DEVELOPMENT.md §34 / §35).
 *
 * §35 puts schema validation on our side of the wire, so validate() is the only door into
 * native messaging. The extension holds no database and judges no project state (禁止 9):
 * it hands message turns to the core as evidence and carries out the two actions the core
 * answers with — openTab and fillComposer.
 *
 * How "generic web" is distinguished (§34 用户授权读取当前网页会话):
 *   - listed origin (chatgpt.com / *.chatgpt.com / chat.openai.com / claude.ai /
 *     gemini.google.com): host_permissions are permanent, the content script is always there;
 *   - any other origin: refused unless this same click in the popup minted a one-shot consent
 *     token for that exact page origin (mintConsent / consumeConsent). Content scripts never
 *     start a request and pages cannot reach this listener, so every request traces back to a
 *     press of 采集选中 / 导入本会话 / 把包写进输入框 — that click *is* the authorisation.
 */

const HOST = "app.codlas.workbench.host";
const PROTOCOL = "codlas-nm/1";

/* §35 size. The host enforces its own cap too; refusing here keeps an oversized capture off
 * the wire instead of discovering it seconds later as a silent disconnect. */
const MAX_BYTES = 245760;
const MAX_TURNS = 500;
const MAX_PROJECT_CHARS = 200;
const REPLY_TIMEOUT_MS = 30000;
const CONSENT_TTL_MS = 60000;

/* 自检 and 打开目标网页 read no page at all, so they report who is asking rather than where
 * material came from: this extension's own id, which is what Chrome checked before it let the
 * host process be reachable at all. */
function extensionOrigin() {
  return chrome.runtime.getURL("").replace(/\/$/, "");
}

const RFC3339 = /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d{1,9})?([Zz]|[+-]\d{2}:\d{2})$/;

const ALLOWED_COMMANDS = ["hello", "capture", "open", "inject"];
const ALLOWED_SOURCES = ["chatgpt_web", "generic_web"];
const CAPTURE_KEYS = ["source", "conversationId", "title", "messages"];
const MESSAGE_KEYS = ["role", "content", "messageId", "at"];

/* origin -> expiry timestamp. Memory only: it dies with the worker, which is the point. */
const consents = new Map();

function fail(error) {
  return { ok: false, error };
}

function isPlainObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function byteLength(value) {
  return new TextEncoder().encode(JSON.stringify(value)).length;
}

function originOf(url) {
  try {
    return new URL(url).origin;
  } catch (_) {
    return "";
  }
}

function isListedOrigin(origin) {
  let url;
  try {
    url = new URL(origin);
  } catch (_) {
    return false;
  }
  if (url.protocol !== "https:") return false;
  const host = url.hostname.toLowerCase();
  if (host === "chatgpt.com" || host.endsWith(".chatgpt.com")) return true;
  return host === "chat.openai.com" || host === "claude.ai" || host === "gemini.google.com";
}

function mintConsent(origin) {
  if (origin) consents.set(origin, Date.now() + CONSENT_TTL_MS);
}

function hasConsent(origin) {
  const expiry = consents.get(origin);
  if (expiry === undefined) return false;
  if (expiry < Date.now()) {
    consents.delete(origin);
    return false;
  }
  return true;
}

/* One click authorises one request; a second request out of a listed origin needs a new click. */
function consumeConsent(origin) {
  consents.delete(origin);
}

function usableTargetUrl(raw) {
  if (typeof raw !== "string" || raw.trim() === "") return null;
  let url;
  try {
    url = new URL(raw.trim());
  } catch (_) {
    return null;
  }
  const local = url.protocol === "http:" && /^(localhost|127\.0\.0\.1|\[::1\])$/.test(url.hostname);
  if (url.protocol !== "https:" && !local) return null;
  if (url.username || url.password) return null;
  return url.toString();
}

function checkProject(request) {
  if (request.project === undefined || request.project === null || request.project === "") {
    return null;
  }
  if (typeof request.project !== "string") return "project 必须是字符串，或者省略。";
  if (request.project.length > MAX_PROJECT_CHARS) return "project 太长了。";
  if (/[\u0000-\u001f]/.test(request.project)) return "project 里含控制字符。";
  return null;
}

function checkCapture(payload, origin) {
  for (const key of Object.keys(payload)) {
    if (CAPTURE_KEYS.indexOf(key) === -1) return "capture payload 含未知字段：" + key;
  }
  if (ALLOWED_SOURCES.indexOf(payload.source) === -1) {
    return "capture 的 source 只能是 chatgpt_web 或 generic_web。";
  }
  if (typeof payload.conversationId !== "string" || payload.conversationId.trim() === "") {
    return "capture 缺少 conversationId：每条会话都要有自己的标识。";
  }
  if (payload.conversationId.length > 300) return "capture 的 conversationId 太长了。";
  if (payload.title !== undefined && payload.title !== null && typeof payload.title !== "string") {
    return "capture 的 title 只能是字符串或 null。";
  }
  if (!Array.isArray(payload.messages)) return "capture 的 messages 必须是数组。";
  if (payload.messages.length === 0) {
    return "这一页里没有采到任何一条消息轮次，所以什么都没发出去——宁可空手，不编内容。";
  }
  if (payload.messages.length > MAX_TURNS) {
    return "采到的轮次超过 " + MAX_TURNS + " 条，已经超出 V1 的单次上限。";
  }
  if (payload.source === "generic_web" && !isListedOrigin(origin) && !hasConsent(origin)) {
    return "来源 " + origin + " 不在支持列表里，而且没有一次新的「采集」点击为它授权。";
  }
  for (let index = 0; index < payload.messages.length; index += 1) {
    const turn = payload.messages[index];
    const label = "第 " + (index + 1) + " 条消息";
    if (!isPlainObject(turn)) return label + "不是对象。";
    for (const key of Object.keys(turn)) {
      if (MESSAGE_KEYS.indexOf(key) === -1) return label + "含未知字段：" + key;
    }
    if (turn.role !== "user" && turn.role !== "assistant") {
      return label + "的 role 既不是 user 也不是 assistant。";
    }
    if (typeof turn.content !== "string" || turn.content.trim() === "") {
      return label + "的 content 是空的。";
    }
    if (turn.messageId !== undefined && typeof turn.messageId !== "string") {
      return label + "的 messageId 不是字符串。";
    }
    if (turn.at !== undefined && (typeof turn.at !== "string" || !RFC3339.test(turn.at))) {
      return label + "的 at 不是 RFC3339 时间，已拒绝而不是替你猜一个。";
    }
  }
  return null;
}

function validate(request) {
  if (!isPlainObject(request)) return fail("请求不是一个 JSON 对象。");
  if (request.protocol !== PROTOCOL) {
    return fail("协议版本不对：期望 " + PROTOCOL + "，实际是 " + String(request.protocol) + "。");
  }
  if (ALLOWED_COMMANDS.indexOf(request.command) === -1) {
    return fail("不认识的命令：" + String(request.command) + "。V1 只认 hello / capture / open / inject。");
  }
  if (typeof request.origin !== "string" || request.origin === "") return fail("请求没有 origin。");

  const projectError = checkProject(request);
  if (projectError) return fail(projectError);

  if (request.origin === extensionOrigin()) {
    if (request.command !== "hello" && request.command !== "open") {
      return fail(request.command + " 要写明它来自哪个网页，不能用扩展自身的来源。");
    }
  } else if (!isListedOrigin(request.origin) && !hasConsent(request.origin)) {
    return fail("来源 " + request.origin + " 既不在支持列表里，也没有一次刚刚发生的点击为它授权。");
  }

  if (!isPlainObject(request.payload)) return fail("payload 必须是对象。");

  let payloadError = null;
  if (request.command === "hello") {
    if (Object.keys(request.payload).length > 0) payloadError = "hello 的 payload 必须是空的。";
  } else if (request.command === "capture") {
    payloadError = checkCapture(request.payload, request.origin);
  } else if (request.command === "open") {
    for (const key of Object.keys(request.payload)) {
      if (key !== "url") payloadError = "open payload 含未知字段：" + key;
    }
    if (!payloadError && !usableTargetUrl(request.payload.url)) {
      payloadError = "open 的 url 只能是真的网页地址（https，或本机 http），不接受别的协议。";
    }
  } else if (request.command === "inject") {
    for (const key of Object.keys(request.payload)) {
      if (key !== "text") payloadError = "inject payload 含未知字段：" + key;
    }
    if (!payloadError && (typeof request.payload.text !== "string" || payloadTextEmpty(request.payload.text))) {
      payloadError = "inject 的 text 不能是空的。";
    }
  }
  if (payloadError) return fail(payloadError);

  const size = byteLength(request);
  if (size > MAX_BYTES) {
    return fail("这一次请求有 " + size + " 字节，超过 240 KiB（245760 字节）上限，已在本机拒绝。");
  }
  return { ok: true, bytes: size };
}

function payloadTextEmpty(value) {
  return value.trim() === "";
}

function normalizeReply(raw) {
  if (!isPlainObject(raw)) return fail("宿主没有按 { ok, data | error } 的形状回答。");
  if (raw.ok === true) return { ok: true, data: isPlainObject(raw.data) ? raw.data : {} };
  if (raw.ok === false) {
    return { ok: false, error: typeof raw.error === "string" && raw.error ? raw.error : "宿主拒绝了这个请求。" };
  }
  return fail("宿主的回答里 ok 既不是 true 也不是 false。");
}

function callHost(request) {
  return new Promise((resolve) => {
    const check = validate(request);
    if (!check.ok) {
      check.local = true;
      resolve(check);
      return;
    }
    if (!isListedOrigin(request.origin) && request.origin !== extensionOrigin()) {
      consumeConsent(request.origin);
    }

    const started = Date.now();
    let settled = false;
    let port = null;

    const finish = (reply) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (port) {
        try {
          port.disconnect();
        } catch (_) {
          /* the port is already gone */
        }
      }
      const normalized = normalizeReply(reply);
      normalized.bytes = check.bytes;
      normalized.elapsedMs = Date.now() - started;
      resolve(normalized);
    };

    const timer = setTimeout(() => {
      finish(fail("宿主在 " + Math.round(REPLY_TIMEOUT_MS / 1000) + " 秒内没有回答，这条连接已经关掉。"));
    }, REPLY_TIMEOUT_MS);

    try {
      port = chrome.runtime.connectNative(HOST);
    } catch (error) {
      finish(fail("连不上本地宿主：" + textOfError(error)));
      return;
    }

    /* One request, one answer, then the port closes. Anything after that is ignored. */
    port.onMessage.addListener((message) => finish(message));
    port.onDisconnect.addListener(() => {
      const lastError = chrome.runtime.lastError;
      const reason = lastError && lastError.message ? lastError.message : "";
      if (reason.indexOf("Specified native messaging host not found") !== -1) {
        finish(fail("找不到宿主 " + HOST + "。桌面端那份 NativeMessagingHosts 里的 json 装好了吗？"));
      } else {
        finish(fail("宿主断开了连接，没有给出答复。" + (reason ? "（" + reason + "）" : "")));
      }
    });

    try {
      port.postMessage(request);
    } catch (error) {
      finish(fail("宿主刚连上就写不进去：" + textOfError(error)));
    }
  });
}

function textOfError(error) {
  return String(error && error.message ? error.message : error);
}

/* ---- reaching the page ---- */

async function activeTab() {
  const tabs = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  return tabs && tabs[0] ? tabs[0] : null;
}

async function getTab(tabId) {
  return chrome.tabs.get(tabId);
}

async function ensureCollector(tabId) {
  try {
    await chrome.tabs.sendMessage(tabId, { type: "ping" });
    return true;
  } catch (_) {
    /* Tab opened before the extension was loaded: inject the same file once. */
    try {
      await chrome.scripting.executeScript({ target: { tabId }, files: ["content.js"] });
      return true;
    } catch (error) {
      return false;
    }
  }
}

async function askPage(tabId, message) {
  try {
    return { ok: true, reply: await chrome.tabs.sendMessage(tabId, message) };
  } catch (error) {
    return fail("页面那边的脚本没有回答：" + textOfError(error));
  }
}

/* §35 project scope: the id travels on the request and comes back from the host. We carry it,
 * we never decide what project a page belongs to. */
async function rememberedProject() {
  const stored = await chrome.storage.local.get("codlasProject");
  return typeof stored.codlasProject === "string" ? stored.codlasProject : "";
}

async function rememberProject(data) {
  if (!isPlainObject(data)) return "";
  const found = [data.projectId, data.project, data.project_id, data.projectScope].find(
    (value) => typeof value === "string" && value !== ""
  );
  if (!found) return "";
  await chrome.storage.local.set({ codlasProject: found });
  return found;
}

async function attachProject(request) {
  const project = await rememberedProject();
  if (project) request.project = project;
  return request;
}

/* ---- applying what the core answers with ---- */

async function applyOpenTab(url) {
  /* Reuse only an exact-URL match: navigating a tab that sits on another conversation would
   * throw away a page the user is reading. */
  const tabs = await chrome.tabs.query({});
  const match = tabs.find((tab) => tab.url && tab.url === url);
  if (match) {
    await chrome.tabs.update(match.id, { url, active: true });
    return { opened: "标签页 " + match.id + " 已经在 " + url + "，已切到它。" };
  }
  const created = await chrome.tabs.create({ url, active: true });
  return { opened: "已打开新标签页 " + (created && created.id) + "：" + url };
}

async function applyFillComposer(data, tabId) {
  const text = typeof data.text === "string" ? data.text : "";
  if (payloadTextEmpty(text)) return fail("宿主的 fillComposer 没有带上文字，写不进输入框。");

  /* §7.8 发送永远是人的动作：send 在本扩展里只有一个取值。宿主就算回了 send:true，
   * 我们也只是把字写进输入框——不回车、不点发送键，这个拒绝本身就是这条规则的目的。 */
  const hostAskedToSend = data.send === true;

  let tab = null;
  try {
    tab = tabId ? await getTab(tabId) : await activeTab();
  } catch (_) {
    tab = await activeTab();
  }
  if (!tab) return fail("没有可以写入的标签页。");
  if (!(await ensureCollector(tab.id))) return fail("这个标签页里注入不进脚本，写不了输入框。");

  const filled = await askPage(tab.id, { type: "fillComposer", text, send: false });
  if (!filled.ok) return filled;
  const result = filled.reply || {};
  return {
    ok: true,
    filled: !!result.filled,
    composer: result.composer || "",
    reason: result.reason || "",
    send: false,
    note:
      (result.filled
        ? "内容已写进输入框（" + (result.composer || "输入框") + "），发不发由你来按。"
        : "没能写进输入框：" + (result.reason || "页面上找不到可见的输入框。")) +
      (hostAskedToSend ? " 宿主这次带了 send:true，已按 §7.8 忽略。" : "")
  };
}

async function applyHostAction(reply, tabId) {
  if (!reply.ok) return reply;
  const data = reply.data || {};
  /* `capture` answers with what landed, not with an instruction, so there is nothing to carry
   * out — and calling its own reply an unknown action would be a lie the popup prints. */
  if (data.action === "captured") return reply;
  if (data.action === "openTab") {
    const url = usableTargetUrl(data.url);
    if (!url) return Object.assign(reply, fail("宿主给的 openTab 地址不是一个真的网页地址，已忽略。"));
    return Object.assign(reply, await applyOpenTab(url));
  }
  if (data.action === "fillComposer") {
    const applied = await applyFillComposer(data, tabId);
    return Object.assign(reply, applied);
  }
  if (data.action) {
    reply.note = "宿主返回了动作 " + String(data.action) + "，V1 的扩展只认 openTab 与 fillComposer，已忽略。";
  }
  return reply;
}

/* ---- the four commands ---- */

async function runHello() {
  const reply = await callHost(
    await attachProject({ protocol: PROTOCOL, origin: extensionOrigin(), command: "hello", payload: {} })
  );
  if (!reply.ok) return reply;
  reply.project = await rememberProject(reply.data);
  reply.note = "自检完成。";
  return reply;
}

async function runCapture(scope, tabId) {
  let tab = null;
  try {
    tab = await getTab(tabId);
  } catch (_) {
    return fail("找不到那个标签页，可能已经关掉了。");
  }
  const origin = originOf(tab.url || "");
  if (!origin) return fail("这个标签页还没有地址。");
  if (!(await ensureCollector(tabId))) {
    return fail("读不进这个页面：扩展在它身上没有权限。已支持的站点见 README。");
  }

  const collected = await askPage(tabId, { type: "collect", scope });
  if (!collected.ok) return collected;
  const page = collected.reply || {};
  if (!page.ok) return fail(page.error || "页面那边的采集失败了。");

  const messages = Array.isArray(page.messages) ? page.messages : [];
  const request = await attachProject({
    protocol: PROTOCOL,
    origin,
    command: "capture",
    payload: {
      source: page.source,
      conversationId: page.conversationId,
      title: typeof page.title === "string" ? page.title : null,
      messages
    }
  });

  const reply = await callHost(request);
  reply.origin = origin;
  reply.note = String(page.note || "");
  if (reply.ok) {
    reply.turns = messages.length;
    reply.project = await rememberProject(reply.data);
  } else if (messages.length === 0 && page.note) {
    /* The local refusal stays, but the page's own account of what it looked for matters more. */
    reply.error = reply.error + " " + page.note;
  }
  return applyHostAction(reply, tabId);
}

async function runOpen(url) {
  const reply = await callHost(await attachProject({ protocol: PROTOCOL, origin: extensionOrigin(), command: "open", payload: { url } }));
  return applyHostAction(reply, null);
}

async function runInject(text, tabId) {
  let tab = null;
  try {
    tab = await getTab(tabId);
  } catch (_) {
    tab = await activeTab();
  }
  if (!tab) return fail("没有可以写入的标签页。");
  const origin = originOf(tab.url || "");
  if (!origin) return fail("这个标签页还没有地址。");

  const reply = await callHost(await attachProject({ protocol: PROTOCOL, origin, command: "inject", payload: { text } }));
  if (!reply.ok) return reply;
  reply.project = await rememberProject(reply.data);
  return applyHostAction(reply, tab.id);
}

/* ---- the only inbound door: the popup ---- */

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (!isPlainObject(message) || typeof message.action !== "string") return false;
  if (sender.tab || sender.id !== chrome.runtime.id) return false;

  const dispatch = async () => {
    switch (message.action) {
      case "hello":
        return runHello();
      case "capture": {
        const scope = message.scope === "selection" ? "selection" : "session";
        const tab = await activeTab();
        if (!tab) return fail("没有活动的标签页。");
        mintConsent(originOf(tab.url || ""));
        return runCapture(scope, tab.id);
      }
      case "open": {
        const url = usableTargetUrl(message.url);
        if (!url) return fail("地址不是一个真的网页地址（要 https，或本机 http）。");
        return runOpen(url);
      }
      case "inject": {
        if (typeof message.text !== "string" || payloadTextEmpty(message.text)) {
          return fail("还没有选要写进去的包文件，或者文件是空的。");
        }
        const tab = await activeTab();
        if (!tab) return fail("没有活动的标签页。");
        mintConsent(originOf(tab.url || ""));
        return runInject(message.text, tab.id);
      }
      default:
        return fail("扩展内部消息里出现了不认识的 action：" + message.action);
    }
  };

  dispatch()
    .then((reply) => respond(reply || fail("没有结果。")))
    .catch((error) => respond(fail("扩展自己出错了：" + textOfError(error))));
  return true;
});
