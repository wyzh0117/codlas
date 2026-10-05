/* Codlas workbench — V1-E shell front end.
 *
 * Renders what `apps/desktop/src/views.rs` compiled over the existing ports. Nothing here folds
 * events, judges state or decides whether a fact happened: the only write the shell can issue is
 * `apply_pending`, which is §7.4's own confirm path, on the entries the user ticked and with one
 * of the verbs the record behind that entry actually offers.
 */

const { invoke } = window.__TAURI__.core;

const work = document.getElementById("work");
const ctxLine = document.getElementById("ctx");
const screens = {};
let current = "diff";
/* §14 item 7: a click on one record is the request. The screens that can be clicked set this,
 * the context screen consumes it, and nothing else in the shell reads it. */
let pendingFocus = "";

function focusButton(ref) {
  return `<button class="act tiny" data-focus="${esc(ref)}">带它去上下文 →</button>`;
}

function onRefClick(node) {
  node.addEventListener("click", (event) => {
    const btn = event.target.closest("button[data-focus]");
    if (!btn) return;
    pendingFocus = btn.dataset.focus;
    go("context");
  });
}

const esc = (value) =>
  String(value ?? "").replace(/[&<>"']/g, (ch) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[ch]
  );

const clock = (secs) =>
  secs ? new Date(secs * 1000).toLocaleString("zh-CN", { hour12: false }) : "—";

function el(tag, cls, html) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (html !== undefined) node.innerHTML = html;
  return node;
}

function screen(title, sub, actions) {
  const head = el("header", "screen", `<h1>${esc(title)}</h1><span class="sub">${esc(sub || "")}</span>`);
  if (actions) {
    const box = el("div", "actions");
    actions.forEach(([label, cls, fn]) => {
      const btn = el("button", `act ${cls || ""}`, esc(label));
      btn.addEventListener("click", fn);
      box.appendChild(btn);
    });
    head.appendChild(box);
  }
  work.replaceChildren(head);
  return work;
}

async function guard(fn) {
  work.replaceChildren(el("p", "empty-screen", "读取中…"));
  try {
    await fn();
  } catch (error) {
    screen("读不出来", "工作台没有替你猜");
    work.appendChild(el("div", "card", `<p class="error">${esc(error)}</p>`));
  }
}

/* ── ① 变动：第一屏 ─────────────────────────────────────────────────────── */
screens.diff = () =>
  guard(async () => {
    const view = await invoke("diff_view");
    const head = screen(
      "这一轮发生了什么变动",
      `${view.fromSnapshot} → ${view.toSnapshot} · ${clock(view.capturedAt)}`,
      [["刷新", "", () => go("diff")], ["进入项目地图 →", "primary", () => go("map")]]
    );

    const card = el(
      "div",
      "card",
      `<h2>模块变动</h2><p class="note">下面这些是<em>已经发生</em>的变动——由已接受的事件折叠出来，只读。要改它们，得回到事件本身。</p>`
    );
    if (view.note) card.appendChild(el("p", "note", esc(view.note)));
    if (!view.sections.length) {
      card.appendChild(el("p", "empty-screen", "与上一份快照相比没有任何模块发生变化。"));
    }
    view.sections.forEach((section) => {
      const block = el(
        "div",
        `mod ${esc(section.change)}`,
        `<h3>${esc(section.name)} <span class="pill">${esc(section.change)}</span></h3>`
      );
      const list = el("div", "items");
      section.entries.forEach((entry) => {
        const row = el("div");
        row.appendChild(
          el(
            "span",
            "",
            section.change === "removed"
              ? `<del>− ${esc(entry.text)}</del>`
              : section.change === "added"
                ? `<ins>+ ${esc(entry.text)}</ins>`
                : esc(entry.text)
          )
        );
        list.appendChild(row);
      });
      block.appendChild(list);
      card.appendChild(block);
    });
    head.appendChild(card);

    const pending = el(
      "div",
      "card",
      `<h2>等你决定的条目 <span class="pill">${view.pending.length}</span></h2>` +
        `<p class="note">这些不是「变动是否发生」，而是 §7.4 说的「由用户确认才生效」的条目：变更提案与 Inbox 里待判的记录。` +
        `勾上你同意的，并为每一条选一个它自己提供的动作，再应用。确认之后提案携带的基线变化才会生效。</p>`
    );
    if (!view.pending.length) {
      pending.appendChild(el("p", "empty-screen", "没有待确认的条目。"));
      head.appendChild(pending);
      return;
    }

    const rows = new Map();
    const form = el("ul", "plain");
    view.pending.forEach((item) => {
      const li = el("li", "");
      const label = el("label", "item");
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = true;
      box.dataset.id = item.id;
      label.appendChild(box);
      label.appendChild(
        el(
          "span",
          "",
          `<span class="kind">${esc(item.source)} · ${esc(item.kind)}</span><br>${esc(item.summary)}` +
            (item.detail ? `<br><span class="kind">${esc(item.detail)}</span>` : "") +
            `<br><span class="kind">${clock(item.at)}</span>`
        )
      );
      // The verbs come from the record itself; the shell never offers a button it cannot honour.
      const pick = el("select", "act");
      (item.actions || []).forEach((action) => {
        const option = document.createElement("option");
        option.value = action;
        option.textContent = action;
        pick.appendChild(option);
      });
      label.appendChild(pick);
      li.appendChild(label);
      form.appendChild(li);
      rows.set(item.id, { box, pick, item, li });
    });
    pending.appendChild(form);

    const bar = el("div", "actions");
    const all = el("button", "act", "全选 / 全不选");
    all.addEventListener("click", () => {
      const checked = [...pending.querySelectorAll("input:checked")];
      const state = checked.length !== pending.querySelectorAll("input").length;
      pending.querySelectorAll("input").forEach((input) => (input.checked = state));
    });
    const apply = el("button", "act primary", "应用所选");
    apply.addEventListener("click", async () => {
      const chosen = [...rows.values()].filter((row) => row.box.checked);
      if (!chosen.length) return;
      const choices = chosen.map((row) => ({
        id: row.item.id,
        kind: row.item.kind,
        action: row.pick.value,
      }));
      apply.disabled = true;
      apply.textContent = "应用中…";
      try {
        const result = await invoke("apply_pending", { choices });
        result.applied.forEach((id) => rows.get(id)?.li.remove());
        pending.appendChild(
          el(
            "p",
            "note",
            `已确认 ${result.applied.length} 条` +
              (result.refused.length
                ? `；被拒绝 ${result.refused.length} 条：` +
                  result.refused
                    .map((refusal) => `${esc(refusal.id)} — ${esc(refusal.reason)}`)
                    .join("；")
                : "")
          )
        );
      } catch (error) {
        pending.appendChild(el("p", "error", esc(error)));
      }
      apply.disabled = false;
      apply.textContent = "应用所选";
    });
    bar.appendChild(all);
    bar.appendChild(apply);
    pending.appendChild(bar);
    head.appendChild(pending);
  });

/* ── ② 项目地图 ───────────────────────────────────────────────────────── */
screens.map = () =>
  guard(async () => {
    const map = await invoke("map_view");
    const head = screen("PROJECT_MAP", `编译于 ${clock(map.compiledAt)} · §31 的 13 个 section`, [
      ["← 回到变动", "", () => go("diff")],
    ]);
    const card = el("div", "card map");
    map.sections.forEach((section) => {
      const block = el("div", "", `<h3>${esc(section.name)}</h3>`);
      if (section.empty) {
        block.appendChild(el("p", "empty", esc(section.lines[0] || "_none recorded_")));
      } else {
        block.appendChild(el("ul", "", section.lines.map((line) => `<li>${esc(line)}</li>`).join("")));
      }
      card.appendChild(block);
    });
    head.appendChild(card);
    head.appendChild(
      el(
        "div",
        "card",
        `<p class="note">这张图是编译出来的视图，不是第二套事实（禁止 4）。状态哈希：<code>${esc(map.stateHash)}</code></p>`
      )
    );
  });

/* ── 总览 ─────────────────────────────────────────────────────────────── */
screens.overview = () =>
  guard(async () => {
    const view = await invoke("overview_view");
    const head = screen("总览", `seq ${view.lastSeq} · ${view.appliedEvents} 条已接受事件折叠 · ${clock(view.builtAt)}`);

    const baseline = el("div", "card", `<h2>基线</h2>`);
    baseline.appendChild(
      el(
        "table",
        "grid",
        `<tbody><tr><th>Charter</th><td>${view.charterVersion === undefined ? "—" : `v${esc(view.charterVersion)}`}</td></tr>` +
          `<tr><th>Scope</th><td>${view.scopeVersion === undefined ? "—" : `v${esc(view.scopeVersion)}`}</td></tr>` +
          `<tr><th>Architecture Baseline</th><td>${view.baselineVersion === undefined ? "—" : `v${esc(view.baselineVersion)}`}</td></tr>` +
          `<tr><th>目录</th><td>${view.rootPath ? `<code>${esc(view.rootPath)}</code>` : "没有绑定目录"}</td></tr>` +
          `<tr><th>最近快照</th><td>${view.latestSnapshot ? esc(view.latestSnapshot) : "还没有快照：在终端跑 wp snapshot take"}</td></tr>` +
          `<tr><th>状态哈希</th><td><code>${esc(view.stateHash)}</code></td></tr></tbody>`
      )
    );
    head.appendChild(baseline);

    const counts = el("div", "card", `<h2>折叠出来的记录</h2><p class="note">${esc(view.note)}</p>`);
    counts.appendChild(
      el(
        "table",
        "grid",
        `<thead><tr><th>类型</th><th>条数</th><th>分布</th></tr></thead><tbody>` +
          view.kinds
            .map(
              (row) =>
                `<tr><td>${esc(row.name)}</td><td class="num">${esc(row.total)}</td><td>` +
                (row.tally.length
                  ? row.tally.map((t) => `<span class="pill">${esc(t.status)} ${esc(t.count)}</span>`).join(" ")
                  : "—") +
                `</td></tr>`
            )
            .join("") +
          `</tbody>`
      )
    );
    const pointers = el("div", "actions");
    if (view.openProposals) {
      const toDiff = el("button", "act primary", `${esc(view.openProposals)} 条变更提案等你决定 →`);
      toDiff.addEventListener("click", () => go("diff"));
      pointers.appendChild(toDiff);
    }
    if (view.driftFindings) {
      const toDrift = el("button", "act", `${esc(view.driftFindings)} 条偏航提示 →`);
      toDrift.addEventListener("click", () => go("drift"));
      pointers.appendChild(toDrift);
    }
    if (pointers.children.length) counts.appendChild(pointers);
    head.appendChild(counts);
  });

/* ── 决策 ─────────────────────────────────────────────────────────────── */
screens.decisions = () =>
  guard(async () => {
    const view = await invoke("decisions_view");
    const head = screen("决策", `${view.decisions.length} 条 · ${esc(view.note)}`);
    if (!view.decisions.length) {
      head.appendChild(el("div", "card", `<p class="empty-screen">还没有记录任何决策。决策由事件折出来，不在这里补写。</p>`));
      return;
    }
    view.decisions.forEach((item) => {
      const card = el(
        "div",
        "card",
        `<h2>${esc(item.title)} <span class="pill${item.locked ? " ok" : ""}">${esc(item.status)}</span></h2>` +
          `<p>${esc(item.decision)}</p>` +
          (item.rationale ? `<p class="note">为什么：${esc(item.rationale)}</p>` : "") +
          (item.alternatives.length ? `<p class="note">被放弃的选项：${item.alternatives.map(esc).join("；")}</p>` : "") +
          `<p class="kind"><code>${esc(item.id)}</code> · ${clock(item.at)}` +
          (item.supersedes ? ` · 取代 <code>${esc(item.supersedes)}</code>` : "") +
          ` · 证据 ${esc(item.evidence)} 条${item.locked ? " · 已由用户锁定（§7.4）" : ""}</p>`
      );
      head.appendChild(card);
    });
  });

/* ── 演进 / AEP ───────────────────────────────────────────────────────── */
screens.evolution = () =>
  guard(async () => {
    const view = await invoke("evolution_view");
    const head = screen("演进 / AEP", `${view.aeps.length} 份 · ${esc(view.note)}`);
    if (!view.aeps.length) {
      head.appendChild(el("div", "card", `<p class="empty-screen">还没有演进提案。AEP 住在 <code>docs/evolution/</code>，由采集折进时间线之后才出现在这里。</p>`));
      return;
    }
    view.aeps.forEach((item) => {
      const card = el(
        "div",
        "card",
        `<h2>${esc(item.code)} ${esc(item.title)} <span class="pill">${esc(item.status)}</span></h2>` +
          `<p class="note">现在：${esc(item.current)}</p>` +
          `<p class="note">限制：${esc(item.limitation)}</p>` +
          `<p class="note">为什么现在不做：${esc(item.whyNotNow)}</p>` +
          `<p class="note">触发条件：${item.trigger.length ? item.trigger.map(esc).join("；") : "未写明"}</p>` +
          `<p>${esc(item.target)}</p>` +
          (item.changeSurface.length ? `<p class="kind">会动到：${item.changeSurface.map(esc).join("、")}</p>` : "") +
          (item.preservedCore.length ? `<p class="kind">保持不变：${item.preservedCore.map(esc).join("、")}</p>` : "") +
          `<p class="kind"><code>${esc(item.id)}</code>${item.benefit ? ` · 收益 ${esc(item.benefit)}` : ""}${item.cost ? ` · 代价 ${esc(item.cost)}` : ""}</p>`
      );
      head.appendChild(card);
    });
  });

/* ── 时间线 ───────────────────────────────────────────────────────────── */
screens.timeline = () =>
  guard(async () => {
    const log = await invoke("timeline_view", { limit: 60 });
    const head = screen("时间线", "已接受的事件，按 seq 倒序 · 待判的记录不在这张表里");
    const card = el("div", "card");
    if (!log.events.length) card.appendChild(el("p", "empty-screen", "还没有已接受的事件。"));
    else {
      const table = el(
        "table",
        "grid",
        `<thead><tr><th>seq</th><th>时间</th><th>类型</th><th>行为者</th><th>判定</th><th>摘要</th></tr></thead>`
      );
      const body = el("tbody");
      log.events.forEach((event) => {
        body.appendChild(
          el(
            "tr",
            "",
            `<td class="num">${esc(event.seq)}</td><td>${clock(event.at)}</td><td><code>${esc(event.kind)}</code></td>` +
              `<td>${esc(event.actor)}</td><td><span class="pill ${event.status === "accepted" ? "ok" : ""}">${esc(event.status)}</span></td>` +
              `<td>${esc(event.summary)}</td>`
          )
        );
      });
      table.appendChild(body);
      card.appendChild(table);
    }
    head.appendChild(card);
  });

/* ── 偏航 / Inbox ─────────────────────────────────────────────────────── */
screens.drift = () =>
  guard(async () => {
    const drift = await invoke("drift_view");
    const head = screen("偏航与待判", "§26 的词法规则只会提议，不会替你决定", [
      ["去变动屏确认 →", "primary", () => go("diff")],
    ]);
    const findings = el("div", "card", `<h2>偏航提示 <span class="pill">${drift.findings.length}</span></h2>`);
    if (!drift.findings.length) findings.appendChild(el("p", "empty-screen", "没有命中任何一条规则。"));
    else
      findings.appendChild(
        el(
          "ul",
          "plain",
          drift.findings
            .map(
              (item) =>
                `<li><code>${esc(item.id)}</code><br><span class="kind">${esc(item.rule)} · ${esc(item.severity)} · ${esc(item.status)} · ${clock(item.at)}</span><br>${esc(item.message)}` +
                (item.subjects.length ? `<br><span class="kind">对象：${item.subjects.map(esc).join("、")}</span>` : "") +
                (item.subjects.some((ref) => ref.includes(":"))
                  ? `<br>${focusButton(item.subjects.find((ref) => ref.includes(":")))}`
                  : "") +
                `</li>`
            )
            .join("")
        )
      );
    onRefClick(findings);
    head.appendChild(findings);

    const inbox = el(
      "div",
      "card",
      `<h2>Inbox <span class="pill">${drift.inbox.length}</span></h2><p class="note">这里每一条都能回到 ① 变动 那一屏去确认。</p>`
    );
    if (!drift.inbox.length) inbox.appendChild(el("p", "empty-screen", "Inbox 是空的。"));
    else
      inbox.appendChild(
        el(
          "ul",
          "plain",
          drift.inbox
            .map(
              (item) =>
                `<li><code>${esc(item.id)}</code> · ${esc(item.kind)} — ${esc(item.summary)} <span class="kind">${clock(item.at)}</span></li>`
            )
            .join("")
        )
      );
    head.appendChild(inbox);
  });

/* ── 检索 ─────────────────────────────────────────────────────────────── */
screens.search = () =>
  guard(async () => {
    const head = screen("检索", "L1 结构化 · L2 FTS5 · L3 关联（同一个管线）");
    const bar = el("div", "actions");
    const box = document.createElement("input");
    box.className = "act";
    box.style.flex = "1";
    box.placeholder = "输入要查的东西，回车";
    const out = el("div", "card");
    out.appendChild(el("p", "empty-screen", "还没有查询。"));
    const run = async () => {
      const query = box.value.trim();
      if (!query) return;
      out.replaceChildren(el("p", "empty-screen", "检索中…"));
      try {
        const hits = await invoke("search_view", { query, limit: 25 });
        if (!hits.length) {
          out.replaceChildren(
            el(
              "p",
              "empty-screen",
              "没有命中。中文整段是一个 token，片段查不到是记录在案的限制（见 ADR-018 / §40）。"
            )
          );
          return;
        }
        const table = el(
          "table",
          "grid",
          `<thead><tr><th>类型</th><th>标题</th><th>片段</th><th>层</th><th>分</th><th>对象</th></tr></thead><tbody>` +
            hits
              .map(
                (hit) =>
                  `<tr><td>${esc(hit.kind)}</td><td>${esc(hit.title)}</td><td>${esc(hit.snippet)}</td>` +
                  `<td><code>${esc(hit.layer)}</code></td><td class="num">${hit.score.toFixed(2)}</td>` +
                  `<td><code>${esc(hit.id)}</code> ${focusButton(`${hit.kind}:${hit.id}`)}</td></tr>`
              )
              .join("") +
            `</tbody>`
        );
        onRefClick(table);
        out.replaceChildren(table);
      } catch (error) {
        out.replaceChildren(el("p", "error", esc(error)));
      }
    };
    box.addEventListener("keydown", (event) => event.key === "Enter" && run());
    const go2 = el("button", "act primary", "检索");
    go2.addEventListener("click", run);
    bar.appendChild(box);
    bar.appendChild(go2);
    head.appendChild(bar);
    head.appendChild(out);
  });

/* ── 上下文预览 ───────────────────────────────────────────────────────── */
screens.context = () =>
  guard(async () => {
    const head = screen(
      "上下文预览",
      "§42 阶梯 · §43 预算 · §53 Policy Filter · §7.8 发送永远是人的动作"
    );
    const bar = el("div", "actions");
    const ask = document.createElement("input");
    ask.className = "act";
    ask.style.flex = "1";
    ask.placeholder = "要问 agent 什么？（留空就把下面聚焦的那一条当作意图）";
    const pick = document.createElement("input");
    pick.className = "act";
    pick.style.width = "13rem";
    pick.placeholder = "聚焦 kind:id";
    pick.title = "在 偏航 / 检索 屏上点「带它去上下文」会自动填这里，也可以自己写 kind:id——§14 第 7 项要的就是这一击。";
    pick.value = pendingFocus;
    const to = document.createElement("input");
    to.className = "act";
    to.value = "generic-cli";
    to.title = "目标 adapter id。V1 里没有任何 adapter 提供 inject，所以这一步永远停在复制兜底。";
    const out = el("div");
    const compile = async () => {
      const question = ask.value.trim();
      const focus = pick.value.trim();
      if (!question && !focus) return;
      out.replaceChildren(el("p", "empty-screen", "编译中…"));
      try {
        const pack = await invoke("context_view", {
          question,
          maxTokens: 1200,
          to: to.value.trim() || "generic-cli",
          select: focus || null,
        });
        out.replaceChildren();
        const plan = el(
          "div",
          "card",
          `<h2>装进去的 / 被留下的</h2><p class="note">${pack.keptCount} / ${pack.totalCount} 条通过预算 · 估计 ${pack.tokenEstimate} token · Policy Filter 遮掉 ${pack.redactedCount} 处` +
            (pack.focus ? ` · 这一包聚焦 <code>${esc(pack.focus)}</code>` : "") +
            `</p>`
        );
        plan.appendChild(
          el(
            "table",
            "grid",
            `<thead><tr><th>优先级</th><th>section</th><th>来源</th><th>token</th><th>结论</th></tr></thead><tbody>` +
              pack.rows
                .map(
                  (row) =>
                    `<tr><td class="num">${esc(row.priority)}</td><td>${esc(row.section)}</td><td><code>${esc(row.sourceRef)}</code></td>` +
                    `<td class="num">${esc(row.tokens)}</td><td>${row.kept ? '<span class="pill ok">带上</span>' : `<span class="pill degraded">${esc(row.reason || "留下")}</span>`}</td></tr>`
                )
                .join("") +
              `</tbody>`
          )
        );
        out.appendChild(plan);
        out.appendChild(
          el(
            "div",
            "card",
            `<h2>准备给 ${esc(pack.prepared.adapterId)} 的正文</h2>` +
              (pack.prepared.copyFallback
                ? `<p class="note">这个 adapter 不能注入，所以正文只能由你复制过去——工作台自己从不发送（§7.8）。</p>`
                : "") +
              `<pre class="pack">${esc(pack.prepared.text)}</pre>`
          )
        );
        out.appendChild(
          el(
            "div",
            "card",
            `<h2>兜底</h2><p>${esc(pack.fallback.instruction)}</p>` +
              `<p class="note">payloadPath：${pack.fallback.payloadPath ? `<code>${esc(pack.fallback.payloadPath)}</code>` : "没有落盘文件"}</p>` +
              (pack.fallback.deepLink ? `<p class="note">deep link：<code>${esc(pack.fallback.deepLink)}</code></p>` : "")
          )
        );
      } catch (error) {
        out.replaceChildren(el("div", "card", `<p class="error">${esc(error)}</p>`));
      }
    };
    const run = el("button", "act primary", "编译 Context Pack");
    run.addEventListener("click", compile);
    bar.appendChild(ask);
    bar.appendChild(pick);
    bar.appendChild(to);
    bar.appendChild(run);
    head.appendChild(bar);
    head.appendChild(out);
    if (pendingFocus) {
      pendingFocus = "";
      compile();
    }
  });

/* ── 健康与备份 ───────────────────────────────────────────────────────── */
screens.health = () =>
  guard(async () => {
    const view = await invoke("health_view");
    const head = screen(
      "健康与备份",
      "`wp adapters` 的判词 · `wp duplicates` 的提示 · `wp backup status` 的结论"
    );
    const adapters = el(
      "div",
      "card",
      `<h2>Adapter 健康</h2><p class="note">unavailable ≠ failed：源不在这台机器上，不是它坏了（ADR-021）。</p>`
    );
    if (!view.adapters.length) adapters.appendChild(el("p", "empty-screen", "还没有采集过任何源。"));
    else
      adapters.appendChild(
        el(
          "table",
          "grid",
          `<thead><tr><th>id</th><th>结局</th><th>上次</th><th>细节</th></tr></thead><tbody>` +
            view.adapters
              .map(
                (row) =>
                  `<tr><td><code>${esc(row.id)}</code></td><td><span class="pill ${esc(row.outcome)}">${esc(row.outcome)}</span></td>` +
                  `<td>${clock(row.lastRun)}</td><td>${esc(row.detail || "")}</td></tr>`
              )
              .join("") +
            `</tbody>`
        )
      );
    head.appendChild(adapters);

    const dups = el(
      "div",
      "card",
      `<h2>近似重复 <span class="pill">${view.duplicates.length}</span></h2><p class="note">只是提示，不合并、不删除（ADR-024）。</p>`
    );
    if (!view.duplicates.length) dups.appendChild(el("p", "empty-screen", "没有达到阈值的近似重复。"));
    else
      dups.appendChild(
        el(
          "ul",
          "plain",
          view.duplicates
            .map(
              (pair) =>
                `<li>${esc(pair.left)} ↔ ${esc(pair.right)} <span class="kind">相似度 ${pair.score.toFixed(3)}</span><br>${esc(pair.preview)}</li>`
            )
            .join("")
        )
      );
    head.appendChild(dups);

    const backup = el("div", "card", `<h2>备份</h2>`);
    backup.appendChild(
      el(
        "table",
        "grid",
        `<tbody><tr><th>路径</th><td><code>${esc(view.backup.path || "—")}</code></td></tr>` +
          `<tr><th>大小</th><td>${esc(view.backup.sizeBytes ? `${view.backup.sizeBytes} bytes` : "—")}</td></tr>` +
          `<tr><th>schema</th><td>v${esc(view.backup.version)}${view.backup.restorable ? ' <span class="pill ok">本机可恢复</span>' : ' <span class="pill failed">比本 build 新，恢复会被挡下</span>'}</td></tr>` +
          `<tr><th>integrity_check</th><td>${esc(view.backup.integrity)}</td></tr>` +
          `<tr><th>事件行</th><td>${esc(view.backup.rowsInStore)}</td></tr>` +
          `<tr><th>项目行</th><td>${esc(view.backup.projectsInStore)}</td></tr></tbody>`
      )
    );
    backup.appendChild(el("p", "note", esc(view.backup.note)));
    head.appendChild(backup);
  });

/* ── 导航 ─────────────────────────────────────────────────────────────── */
function go(name) {
  current = name;
  document.querySelectorAll("nav.side button").forEach((btn) =>
    btn.setAttribute("aria-current", String(btn.dataset.screen === name))
  );
  screens[name]();
}

document.getElementById("nav").addEventListener("click", (event) => {
  const btn = event.target.closest("button[data-screen]");
  if (btn) go(btn.dataset.screen);
});

(async function boot() {
  let info;
  try {
    info = await invoke("project_info");
  } catch (error) {
    ctxLine.textContent = "没有可用项目";
    work.replaceChildren(
      el(
        "div",
        "card",
        `<h2>还没有绑定项目</h2><p class="note">桌面壳不替你创建项目，也不替你猜要显示哪一个。先在终端里：</p><pre class="pack">wp project create "我的工作项目"\nwp project use &lt;id&gt;</pre><p class="error">${esc(error)}</p>`
      )
    );
    return;
  }
  ctxLine.textContent =
    `${info.projectName}${info.rootPath ? ` · ${info.rootPath}` : ""}` +
    ` · seq ${info.lastSeq} / ${info.appliedEvents} 条已折叠`;
  go("diff");
})();
