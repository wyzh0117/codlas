/* Codlas 采集 — content script (§34: 识别来源 / 获取选中文本 / 导入新消息 / 准备输入内容).
 *
 * It answers the service worker and nothing else: it never opens native messaging itself,
 * never starts a capture, and decides no project state (禁止 9). When a page's structure is
 * unknown it says so in the note the popup prints, instead of guessing at turns — a turn whose
 * role cannot be read off the DOM is dropped, never invented.
 *
 * §7.8 is why fillComposer has no submit path at all: no Enter keydown, no requestSubmit,
 * no click on a send button. Writing is the whole job; sending stays a human action.
 */

const MAX_TURNS = 500;
const DEEP_QUERY_BUDGET = 8000;
const FALLBACK_KEY = "codlas.conversation.fallback";

let documentFallbackId = "";

/* One CSS string per entry, so querySelectorAll keeps document order for the whole turn list.
 * Ordered by how much we trust it: first entry that yields a turn wins, later ones are fallbacks.
 * `source` follows the wire protocol, which only has chatgpt_web / generic_web — the real site
 * travels in the request's origin field, so Claude and Gemini read as generic_web here. */
const SITES = [
  {
    kind: "chatgpt",
    match: (host) => host === "chatgpt.com" || host.endsWith(".chatgpt.com") || host === "chat.openai.com",
    source: "chatgpt_web",
    conversation: [/^\/c\/([0-9a-f]{8,}[-0-9a-f]*)/i, /(?:\?|&)prompt_id=([^&#]+)/i],
    turnSets: ["article[data-message-id]", "[data-message-author-role]", "[data-message-id]"],
    roles: [
      { css: '[data-message-author-role="user"]', role: "user" },
      { css: '[data-message-author-role="assistant"]', role: "assistant" }
    ]
  },
  {
    kind: "claude",
    match: (host) => host === "claude.ai" || host.endsWith(".claude.ai"),
    source: "generic_web",
    conversation: [/^\/chat\/([A-Za-z0-9_-]{8,})/, /\/c\/([0-9a-f]{8,}[-0-9a-f]*)/i],
    turnSets: [
      '[data-test-id="user-message"], [data-test-id="assistant-message"]',
      '[data-testid="user-message"], [data-testid="assistant-message"]',
      "[data-message-id]"
    ],
    roles: [
      { css: '[data-test-id*="user"], [data-testid*="user"]', role: "user" },
      { css: '[data-test-id*="assistant"], [data-testid*="assistant"]', role: "assistant" }
    ]
  },
  {
    kind: "gemini",
    match: (host) => host === "gemini.google.com" || host.endsWith(".gemini.google.com"),
    source: "generic_web",
    conversation: [/^\/app\/([A-Za-z0-9_\-]{8,})/, /(?:\?|&)c=([^&#]+)/],
    turnSets: [
      "user-query, model-response",
      '[data-testid="user-query"], [data-testid="model-response"]',
      "[data-message-id]"
    ],
    roles: [
      { css: "user-query, [data-testid='user-query'], [data-test-id='user-query']", role: "user" },
      { css: "model-response, [data-testid='model-response'], [data-test-id='model-response']", role: "assistant" }
    ]
  }
];

/* Pages we have no rule for (§38.2) still get the three attributes that mean "message"
 * anywhere: an id, an author role, a role. Nothing else is attempted, because a guess here
 * would be a fabricated turn. */
const GENERIC_SITE = {
  kind: "generic",
  source: "generic_web",
  conversation: [],
  turnSets: ["[data-message-author-role], [data-message-id], [data-role]"],
  roles: [
    { css: '[data-message-author-role="user"], [data-role="user"]', role: "user" },
    { css: '[data-message-author-role="assistant"], [data-role="assistant"]', role: "assistant" }
  ]
};

const TEXT_HINTS = [
  ".markdown",
  '[data-testid="markdown-content"]',
  '[data-test-id="message-content"]',
  '[data-testid="markdown"]',
  ".prose",
  "p"
];

/* Nodes we never treat as message text: copy buttons, token counts, edit boxes. */
const SKIP_TEXT = "button,[role='button'],textarea,select,style,script,svg,noscript,template,[contenteditable='true']";

const USER_HINTS = [/\buser\b/i, /用户/, /\bhuman\b/i, /\byou\b/i, /^你/];
const ASSISTANT_HINTS = [/\bassistant\b/i, /\bmodel\b/i, /\brespond/i, /助手/, /chatgpt/i, /gemini/i, /claude/i, /\bbot\b/i];

const COMPOSER_PROBES = [
  "#prompt-textarea",
  '[data-testid="composer"] textarea',
  '[data-testid="composer-root"] textarea',
  '[data-test-id="chat-input"] textarea',
  'rich-textarea textarea',
  'rich-textarea [contenteditable="true"]',
  '[role="textbox"][contenteditable="true"]',
  '[contenteditable="true"]',
  "textarea"
];

const NON_COMPOSER = /search|搜索|filter|筛选|密码|password/i;

function siteFor(hostname) {
  const host = String(hostname || "").toLowerCase();
  for (let i = 0; i < SITES.length; i += 1) {
    if (SITES[i].match(host)) return SITES[i];
  }
  return null;
}

function deepQueryAll(root, css, state) {
  let found = [];
  try {
    found = Array.prototype.slice.call(root.querySelectorAll(css));
  } catch (_) {
    found = [];
  }
  const hosts = [];
  try {
    hosts.push.apply(hosts, Array.prototype.slice.call(root.querySelectorAll("*")));
  } catch (_) {
    /* a scope whose element list we cannot read contributes no shadow roots */
  }
  for (let i = 0; i < hosts.length; i += 1) {
    if (state.visited > DEEP_QUERY_BUDGET) return found;
    state.visited += 1;
    const shadow = hosts[i].shadowRoot;
    if (shadow) found = found.concat(deepQueryAll(shadow, css, state));
  }
  return found;
}

function queryAll(css) {
  return deepQueryAll(document, css, { visited: 0 });
}

function readAttribute(node, names) {
  for (let i = 0; i < names.length; i += 1) {
    const value = node.getAttribute(names[i]);
    if (value) return String(value);
  }
  return "";
}

function roleFromAttributes(node) {
  const declared = readAttribute(node, ["data-message-author-role", "data-role", "data-role-type"]).toLowerCase();
  if (declared === "user" || declared === "human" || declared === "you") return "user";
  if (declared === "assistant" || declared === "ai" || declared === "bot" || declared === "model") return "assistant";
  const hints = [
    readAttribute(node, ["data-test-id", "data-testid"]),
    readAttribute(node, ["aria-label", "class", "id"])
  ].join(" ").trim();
  for (let i = 0; i < ASSISTANT_HINTS.length; i += 1) {
    if (ASSISTANT_HINTS[i].test(hints)) return "assistant";
  }
  for (let i = 0; i < USER_HINTS.length; i += 1) {
    if (USER_HINTS[i].test(hints)) return "user";
  }
  return "";
}

function roleOf(node, site) {
  for (let i = 0; i < site.roles.length; i += 1) {
    let hit = false;
    try {
      hit = node.matches(site.roles[i].css);
    } catch (_) {
      hit = false;
    }
    if (hit) return site.roles[i].role;
  }
  return roleFromAttributes(node);
}

function scopeForText(node) {
  for (let i = 0; i < TEXT_HINTS.length; i += 1) {
    let inner = [];
    try {
      inner = Array.prototype.slice.call(node.querySelectorAll(TEXT_HINTS[i]));
    } catch (_) {
      inner = [];
    }
    for (let j = 0; j < inner.length; j += 1) {
      const text = extractText(inner[j]);
      if (text) return text;
    }
  }
  return extractText(node);
}

function extractText(scope) {
  const walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT, null);
  const parts = [];
  let node = walker.nextNode();
  while (node) {
    const parent = node.parentElement;
    if (parent && !parent.closest(SKIP_TEXT)) {
      const value = String(node.nodeValue || "");
      if (value.trim() !== "") parts.push(value);
    }
    node = walker.nextNode();
  }
  return normalizeText(parts.join(""));
}

/* Whitespace is tidied only where it cannot carry meaning: line-end blanks, blank runs, the
 * edges. Intra-line spaces and indentation survive, because a code block's alignment *is* the
 * evidence, and 禁止 9 forbids handing the core a prettified substitute for it. */
function normalizeText(value) {
  return value
    .replace(/\r\n?/g, "\n")
    .replace(/\u00a0/g, " ")
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function messageIdOf(node) {
  const declared = node.getAttribute("data-message-id");
  if (declared) return declared;
  if (node.id) return node.id;
  return undefined;
}

/* Only timestamps the DOM actually states. Everything else leaves `at` off entirely. */
function atOf(node) {
  let stamp = "";
  try {
    const time = node.querySelector("time[datetime]");
    if (time) stamp = time.getAttribute("datetime") || "";
  } catch (_) {
    stamp = "";
  }
  if (isValidAt(stamp)) return stamp;
  const raw = readAttribute(node, ["data-timestamp", "data-created-at", "data-time"]);
  if (raw && /^\d{10}(\d{3})?$/.test(raw)) {
    const millis = raw.length === 13 ? Number(raw) : Number(raw) * 1000;
    const date = new Date(millis);
    if (!isNaN(date.getTime())) return date.toISOString();
  }
  return undefined;
}

function isValidAt(value) {
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}/.test(value);
}

function collectTurns(site) {
  let skipped = 0;
  for (let i = 0; i < site.turnSets.length; i += 1) {
    const nodes = inDocumentOrder(queryAll(site.turnSets[i]));
    const turns = [];
    const seenIds = {};
    for (let j = 0; j < nodes.length; j += 1) {
      const node = nodes[j];
      const role = roleOf(node, site);
      if (!role) {
        skipped += 1;
        continue;
      }
      const content = scopeForText(node);
      if (!content) continue;
      const messageId = messageIdOf(node);
      if (messageId !== undefined) {
        /* Dedupe by the id the page itself states — overlapping selectors can list one turn
         * twice, and the core keys its evidence on this id. */
        if (seenIds[messageId]) continue;
        seenIds[messageId] = true;
      }
      const turn = { node, role, content };
      if (messageId !== undefined) turn.messageId = messageId;
      const at = atOf(node);
      if (at !== undefined) turn.at = at;
      turns.push(turn);
    }
    if (turns.length > 0) {
      return { turns, selector: site.turnSets[i], skipped, tried: site.turnSets.slice(0, i + 1) };
    }
  }
  return { turns: [], selector: "", skipped, tried: site.turnSets };
}

function inDocumentOrder(nodes) {
  return nodes
    .map((node, index) => ({ node, index }))
    .sort((a, b) => {
      let position = 0;
      try {
        position = a.node.compareDocumentPosition(b.node);
      } catch (_) {
        position = 0;
      }
      /* Shadow roots are separate trees, so compare nothing and keep discovery order. */
      if (position === 0) return a.index - b.index;
      return position & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1;
    })
    .map((entry) => entry.node);
}

function toMessage(turn) {
  const message = { role: turn.role, content: turn.content };
  if (turn.messageId !== undefined) message.messageId = turn.messageId;
  if (turn.at !== undefined) message.at = turn.at;
  return message;
}

function conversationIdOf(site) {
  const path = location.pathname + location.search + location.hash;
  for (let i = 0; i < site.conversation.length; i += 1) {
    const hit = path.match(site.conversation[i]);
    if (hit && hit[1]) {
      try {
        return decodeURIComponent(hit[1]).slice(0, 300);
      } catch (_) {
        return hit[1].slice(0, 300);
      }
    }
  }
  return tabStableId();
}

/* Our own label for a page that publishes no session id. It is not page content, so inventing
 * it is honest; sessionStorage keeps it stable across reloads in the same tab. */
function tabStableId() {
  try {
    const stored = sessionStorage.getItem(FALLBACK_KEY);
    if (stored) return stored;
    const fresh = newFallbackId();
    sessionStorage.setItem(FALLBACK_KEY, fresh);
    return fresh;
  } catch (_) {
    if (!documentFallbackId) documentFallbackId = newFallbackId();
    return documentFallbackId;
  }
}

function newFallbackId() {
  return "codlas-tab-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 10);
}

function selectionRange() {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (range.collapsed) return null;
  return range;
}

function intersects(range, node) {
  try {
    return range.intersectsNode(node);
  } catch (_) {
    return false;
  }
}

function collect(scope) {
  const site = siteFor(location.hostname) || GENERIC_SITE;
  const collected = collectTurns(site);
  const base = {
    ok: true,
    source: site.source,
    conversationId: conversationIdOf(site),
    title: document.title || null
  };

  if (scope === "selection") {
    const selected = String(window.getSelection() || "").trim();
    const range = selectionRange();
    if (!range) return Object.assign(base, { messages: [], note: "没有选中任何文字，所以这一轮什么都没采。" });

    const hits = collected.turns.filter((turn) => intersects(range, turn.node));
    if (hits.length === 0) {
      return Object.assign(base, {
        messages: [],
        selectedChars: selected.length,
        note:
          "选中了 " + selected.length + " 个字符，但它落在已识别的消息结构之外。" +
          "协议里每条消息都要写 role（谁说的），V1 不把选区伪装成一条消息，所以没有发送。" +
          describeTries(collected, site)
      });
    }
    return Object.assign(base, {
      messages: hits.map(toMessage),
      selectedChars: selected.length,
      note:
        "按选区取出 " + hits.length + " 条整轮消息（选中 " + selected.length + " 字）。" +
        describeSkips(collected)
    });
  }

  if (collected.turns.length === 0) {
    return Object.assign(base, {
      messages: [],
      note: "在这个页面里没找到已知的消息结构：" + describeTries(collected, site)
    });
  }

  const taken = collected.turns.slice(0, MAX_TURNS);
  return Object.assign(base, {
    messages: taken.map(toMessage),
    note:
      "按 " + collected.selector + " 采到 " + taken.length + " 轮。" + describeSkips(collected) +
      (collected.turns.length > MAX_TURNS
        ? " 这个会话比 V1 单次上限 " + MAX_TURNS + " 轮更长，剩下的留到下一次点。"
        : "")
  });
}

function describeSkips(collected) {
  if (!collected.skipped) return "";
  return "另外 " + collected.skipped + " 个块读不出角色，按“宁缺毋造”跳过了。";
}

function describeTries(collected, site) {
  const tried = (collected.tried || site.turnSets || []).join(" / ");
  return "试过的选择器：" + tried + "。";
}

/* ---- writing into the composer, never sending ---- */

function isUsable(node) {
  if (!node || node.disabled || node.readOnly) return false;
  if (node.getAttribute("aria-hidden") === "true" || node.hidden) return false;
  const rect = node.getBoundingClientRect();
  if (rect.width < 8 || rect.height < 4) return false;
  const signal = [
    readAttribute(node, ["placeholder", "aria-label", "name", "id", "data-testid", "data-test-id"]),
    String(node.className || "")
  ].join(" ");
  if (NON_COMPOSER.test(signal)) return false;
  if (node.tagName === "INPUT" && /search|email|number|password/i.test(node.getAttribute("type") || "")) return false;
  return true;
}

function composerScore(node, rect) {
  let score = 0;
  const signal = readAttribute(node, ["placeholder", "aria-label", "id", "data-testid", "data-test-id"]);
  if (/ask|message|prompt|输入|说|发|问/i.test(signal)) score += 4;
  if (node.isContentEditable) score += 2;
  if (rect.bottom > window.innerHeight * 0.5) score += 2;
  if (rect.width > 200) score += 1;
  if (rect.top < 160) score -= 3;
  return score;
}

function pickComposer() {
  const ranked = [];
  for (let probe = 0; probe < COMPOSER_PROBES.length; probe += 1) {
    const nodes = queryAll(COMPOSER_PROBES[probe]);
    for (let i = 0; i < nodes.length; i += 1) {
      const node = nodes[i];
      if (!isUsable(node)) continue;
      const rect = node.getBoundingClientRect();
      ranked.push({ node, probe, score: composerScore(node, rect) + (COMPOSER_PROBES.length - probe) });
    }
  }
  ranked.sort((a, b) => b.score - a.score);
  return ranked.length ? ranked[0].node : null;
}

function describeComposer(node) {
  const tag = String(node.tagName || "").toLowerCase();
  const id = node.id ? "#" + node.id : "";
  const testId = readAttribute(node, ["data-testid", "data-test-id"]);
  const hint = node.isContentEditable ? "contenteditable" : tag;
  return hint + id + (testId ? "[" + testId + "]" : "");
}

function fire(node, type, detail) {
  const init = { bubbles: true, cancelable: true };
  let event;
  if (type === "input") {
    event = new InputEvent("input", Object.assign({}, init, { inputType: "insertText", data: detail || null }));
  } else {
    event = new Event(type, init);
  }
  node.dispatchEvent(event);
}

function existingText(node) {
  if (node.isContentEditable) return normalizeText(node.textContent || "");
  return String(node.value || "");
}

function setTextareaValue(node, next) {
  const proto = node.tagName === "TEXTAREA" ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
  const descriptor = Object.getOwnPropertyDescriptor(proto, "value");
  /* Frameworks like React ignore a plain assignment; the prototype setter is what makes
   * their own onChange handler see the new text. */
  if (descriptor && descriptor.set) descriptor.set.call(node, next);
  else node.value = next;
  fire(node, "input", next);
  fire(node, "change");
}

function setContentEditable(node, next) {
  node.focus();
  let replaced = false;
  try {
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(node);
    range.collapse(false);
    selection.removeAllRanges();
    selection.addRange(range);
    replaced = document.execCommand("insertText", false, next);
  } catch (_) {
    replaced = false;
  }
  if (!replaced) {
    /* textContent, never innerHTML: pasted page-authored markup must not become live nodes. */
    node.textContent = next;
  }
  fire(node, "input", next);
  fire(node, "change");
}

function fillComposer(rawText) {
  const text = typeof rawText === "string" ? rawText : "";
  if (text.trim() === "") return { ok: true, filled: false, reason: "要写的文字是空的。", send: false };

  const node = pickComposer();
  if (!node) {
    return {
      ok: true,
      filled: false,
      reason: "页面上找不到可见的输入框（textarea 或 contenteditable），也可能是它在 shadow DOM 里或被站点挡住了。",
      send: false
    };
  }

  const before = existingText(node);
  /* A draft the user was already typing is theirs, not ours: we append instead of overwriting. */
  const merged = before.trim() === "" ? text : before + "\n\n" + text;

  try {
    if (node.isContentEditable) setContentEditable(node, merged);
    else setTextareaValue(node, merged);
  } catch (error) {
    return { ok: true, filled: false, reason: "写的时候出错了：" + String(error && error.message ? error.message : error), send: false };
  }

  const composed = existingText(node);
  return {
    ok: true,
    filled: composed.indexOf(text.slice(-40)) !== -1 || composed.length >= text.length,
    composer: describeComposer(node),
    merged: before.trim() !== "",
    chars: text.length,
    reason: composed === "" ? "写完又读回来是空的，站点可能拦了这次输入。" : "",
    /* §7.8：到这里就停——没有回车，没有 requestSubmit，没有点发送键。 */
    send: false
  };
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (!message || typeof message.type !== "string") return false;
  if (message.type === "ping") {
    respond({ ok: true });
    return false;
  }
  if (message.type === "collect") {
    try {
      respond(collect(message.scope === "selection" ? "selection" : "session"));
    } catch (error) {
      respond({ ok: false, error: "读这个页面时出错了：" + String(error && error.message ? error.message : error) });
    }
    return false;
  }
  if (message.type === "fillComposer") {
    try {
      respond(fillComposer(message.text));
    } catch (error) {
      respond({ ok: false, error: "写输入框时出错了：" + String(error && error.message ? error.message : error) });
    }
    return false;
  }
  return false;
});
