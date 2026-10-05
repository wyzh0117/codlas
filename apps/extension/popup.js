/* Codlas 采集 — popup. Every string here is painted with textContent: the host's own sentence
 * and whatever a page said reach the screen as text, never as markup. */

const status = document.getElementById("status");
const placeholder = document.getElementById("placeholder");
const where = document.getElementById("where");
const projectLine = document.getElementById("project");
const urlInput = document.getElementById("url");
const packInput = document.getElementById("pack");
const packInfo = document.getElementById("packInfo");
const buttons = {
  hello: document.getElementById("hello"),
  captureSelection: document.getElementById("captureSelection"),
  captureSession: document.getElementById("captureSession"),
  open: document.getElementById("open"),
  inject: document.getElementById("inject")
};

const DEFAULT_URL = "https://chatgpt.com/";
let packText = "";

function say(text, cls) {
  if (placeholder && placeholder.parentNode) placeholder.parentNode.removeChild(placeholder);
  const line = document.createElement("p");
  line.className = cls || "";
  line.textContent = text;
  status.appendChild(line);
  status.scrollTop = status.scrollHeight;
  return line;
}

function ask(message) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(message, (reply) => {
      const lastError = chrome.runtime.lastError;
      if (lastError) {
        resolve({ ok: false, local: true, error: "后台没有回应：" + lastError.message });
        return;
      }
      resolve(reply || { ok: false, local: true, error: "后台没有给出结果。" });
    });
  });
}

function report(label, reply) {
  if (!reply || typeof reply !== "object") {
    say(label + "：没有结果。", "err");
    return;
  }
  if (reply.ok === false) {
    /* The error sentence is shown exactly as it came back — local refusal or host's own words. */
    say((reply.local ? "本机校验：" : "宿主说：") + String(reply.error), "err");
  } else {
    say(label + "：通了。", "");
  }

  if (typeof reply.turns === "number") {
    say("送出去 " + reply.turns + " 轮，" + reply.bytes + " 字节，用了 " + reply.elapsedMs + " 毫秒。", "meta");
  } else if (typeof reply.bytes === "number") {
    say(reply.bytes + " 字节，用了 " + reply.elapsedMs + " 毫秒。", "meta");
  }
  if (reply.opened) say(reply.opened, "");
  if (reply.filled === true) {
    say("已写进 " + reply.composer + (reply.merged ? "（原来有草稿，接在后面）" : "") + "。发送我没做。", "");
  } else if (reply.filled === false) {
    say("没能写进输入框：" + (reply.reason || "原因不明。"), "err");
  }
  if (reply.note) say(reply.note, "meta");
  if (reply.project) showProject(reply.project);
  if (reply.data && reply.data.action) say("宿主的动作：" + reply.data.action, "meta");
  if (reply.origin) say("来源：" + reply.origin, "meta");
}

function showProject(project) {
  projectLine.hidden = false;
  projectLine.textContent = "";
  const strong = document.createElement("b");
  strong.textContent = project;
  projectLine.appendChild(document.createTextNode("项目："));
  projectLine.appendChild(strong);
}

async function busy(run) {
  Object.keys(buttons).forEach((key) => {
    buttons[key].disabled = true;
  });
  try {
    await run();
  } catch (error) {
    say("扩展自己出错了：" + String(error && error.message ? error.message : error), "err");
  }
  Object.keys(buttons).forEach((key) => {
    if (key !== "inject" || packText !== "") buttons[key].disabled = false;
  });
}

async function readContext() {
  const stored = await chrome.storage.local.get(["codlasOpenUrl", "codlasProject"]);
  if (typeof stored.codlasOpenUrl === "string" && stored.codlasOpenUrl) urlInput.value = stored.codlasOpenUrl;
  if (typeof stored.codlasProject === "string" && stored.codlasProject) showProject(stored.codlasProject);
}

async function describeTab() {
  const tabs = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  const tab = tabs && tabs[0];
  if (!tab || !tab.url) {
    where.textContent = "当前标签页读不到地址，扩展在它身上没有权限。";
    return;
  }
  let origin = tab.url;
  try {
    origin = new URL(tab.url).origin;
  } catch (_) {
    /* keep the raw string when the URL is something the page cannot be */
  }
  where.textContent = "当前页面：" + origin + (tab.title ? "（" + tab.title + "）" : "");
}

buttons.hello.addEventListener("click", () => {
  busy(async () => {
    say("连接自检：正在敲宿主…", "meta");
    report("自检", await ask({ action: "hello" }));
  });
});

buttons.captureSelection.addEventListener("click", () => {
  busy(async () => {
    say("采集选中：正在读页面上的选区…", "meta");
    report("采集选中", await ask({ action: "capture", scope: "selection" }));
  });
});

buttons.captureSession.addEventListener("click", () => {
  busy(async () => {
    say("导入本会话：正在数消息轮次…", "meta");
    report("导入本会话", await ask({ action: "capture", scope: "session" }));
  });
});

buttons.open.addEventListener("click", () => {
  busy(async () => {
    const url = urlInput.value.trim();
    if (!url) {
      say("地址是空的。", "err");
      return;
    }
    chrome.storage.local.set({ codlasOpenUrl: url });
    report("打开目标网页", await ask({ action: "open", url }));
  });
});

packInput.addEventListener("change", () => {
  const file = packInput.files && packInput.files[0];
  if (!file) {
    packText = "";
    buttons.inject.disabled = true;
    packInfo.textContent = "还没选文件。";
    return;
  }
  const reader = new FileReader();
  reader.onload = () => {
    packText = typeof reader.result === "string" ? reader.result : "";
    /* Counted here so an over-cap pack is explained before the worker rejects it. */
    const bytes = new TextEncoder().encode(packText).length;
    buttons.inject.disabled = packText.trim() === "";
    packInfo.textContent =
      file.name + "：" + packText.trim().length + " 个字符，" + bytes + " 字节。" +
      (bytes > 245760 ? " 已经超 240 KiB，会被本机挡下。" : "");
  };
  reader.onerror = () => {
    packText = "";
    buttons.inject.disabled = true;
    packInfo.textContent = "这个文件读不出来。";
  };
  reader.readAsText(file, "utf-8");
});

buttons.inject.addEventListener("click", () => {
  busy(async () => {
    if (packText.trim() === "") {
      say("还没有选要写进去的包文件。", "err");
      return;
    }
    say("把包写进输入框：先问宿主，再写当前页面…", "meta");
    report("把包写进输入框", await ask({ action: "inject", text: packText }));
  });
});

urlInput.value = DEFAULT_URL;
readContext();
describeTab();
