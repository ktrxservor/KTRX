// Tool page: /outils/<name>. Apple-like layout — hero, stat strip, tabbed body
// (overview / usage / versions), sticky sidebar, related tools.
"use strict";

document.addEventListener("DOMContentLoaded", async () => {
  const { el, codeBlock, copyButton, toolCard, catalog, PERMISSIONS, CATEGORIES } = KTRX;
  const root = document.querySelector("#tool");
  const name = decodeURIComponent(location.pathname.split("/").filter(Boolean)[1] ?? "");

  let tool;
  try {
    const response = await fetch(`/api/tools/${encodeURIComponent(name)}`);
    if (!response.ok) throw new Error(String(response.status));
    tool = await response.json();
  } catch {
    root.replaceChildren(
      el("section", { class: "tool-hero" },
        el("a", { class: "back", href: "/outils", text: "‹ Tous les outils" }),
        el("h1", { text: "Outil introuvable" }),
        el("p", { class: "lead", text: "Cet outil n'existe pas ou n'est plus publié." }),
        el("a", { class: "btn btn-secondary", href: "/outils", text: "Voir le catalogue", style: "margin-top:20px" })),
    );
    return;
  }

  // ---- helpers ------------------------------------------------------------
  // README: the site shows the title itself, and the quoted line under it as the lore.
  function splitReadme(markdown) {
    const lines = (markdown ?? "").replace(/\r\n/g, "\n").split("\n");
    const h1 = lines.findIndex((l) => /^#\s/.test(l));
    const rest = h1 >= 0 ? lines.slice(h1 + 1) : lines;
    while (rest.length && !rest[0].trim()) rest.shift();
    const lore = rest.length && rest[0].startsWith(">") ? rest.shift().replace(/^>\s?/, "") : null;
    const body = rest.join("\n").replace(/!\[[^\]]*\]\((?!https?:)[^)]*\)/g, "");
    return { lore, body };
  }

  function markdown(text) {
    const box = el("div", { class: "prose" });
    if (window.marked && window.DOMPurify) {
      box.innerHTML = DOMPurify.sanitize(marked.parse(text, { gfm: true }));
      box.querySelectorAll("a[href^='http']").forEach((a) => {
        a.target = "_blank";
        a.rel = "noopener noreferrer";
      });
      box.querySelectorAll("pre").forEach((pre) => pre.replaceWith(codeBlock(pre.textContent.replace(/\n$/, ""))));
    } else {
      box.textContent = text;
    }
    return box;
  }

  // `ktrx run 'tools/X ( Y )/K~X.tool' …` is shown as `ktrx run x …`.
  const installedForm = (cmd) =>
    cmd.replace(/(['"])[^'"]*?K~([A-Za-z0-9]+)\.tool\1|\S*K~([A-Za-z0-9]+)\.tool/g, (_m, _q, a, b) => (a ?? b).toLowerCase());

  const fmtSize = (bytes) => (bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1)} Mo` : `${Math.max(1, Math.round(bytes / 1024))} Ko`);
  const fmtDate = (s) => (s ? new Date(s.replace(" ", "T").replace(/Z?$/, "Z")).toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" }) : "");

  const { lore, body } = splitReadme(tool.readme);
  const usage = tool.usage ?? {};
  const liveVersions = (tool.versions ?? []).filter((v) => !v.yanked);
  const latest = liveVersions.find((v) => v.version === tool.latest_version) ?? liveVersions[0];
  const permCount = tool.permissions.length;

  // ---- terminal-style command block (install + run) -----------------------
  function commandBlock(lines) {
    const pre = el("pre");
    for (const line of lines) {
      pre.append(el("span", { class: "term-prompt", text: "$ " }), el("span", { class: "term-cmd", text: line }), "\n");
    }
    return el("div", { class: "term-block" },
      el("div", { class: "term-bar" }, el("span"), el("span"), el("span")),
      pre,
      copyButton(() => lines.join("\n")));
  }

  // ---- hero ---------------------------------------------------------------
  const icon = tool.icon_url
    ? el("img", { class: "th-icon", src: tool.icon_url, alt: "" })
    : el("div", { class: "th-icon th-icon-fallback", text: tool.myth_name[2] });

  const metaChips = el("div", { class: "th-chips" },
    el("span", { class: "chip chip-accent", text: CATEGORIES[tool.category] ?? tool.category }),
    el("span", { class: "chip", text: `v${tool.latest_version}` }),
    ...((usage.tags ?? tool.tags ?? []).slice(0, 4).map((t) => el("span", { class: "chip", text: t }))));

  const hero = el("section", { class: "tool-hero reveal is-visible" },
    el("a", { class: "back", href: "/outils", text: "‹ Tous les outils" }),
    el("div", { class: "th-main" }, icon,
      el("div", { class: "th-text" },
        el("h1", { text: tool.myth_name }),
        el("p", { class: "lead", text: tool.description }),
        lore ? el("p", { class: "lore", text: lore }) : null,
        metaChips)));

  // ---- stat strip ---------------------------------------------------------
  const stat = (value, label) => el("div", { class: "stat" }, el("div", { class: "stat-v", text: value }), el("div", { class: "stat-l", text: label }));
  const strip = el("section", { class: "stat-strip" },
    stat(permCount === 0 ? "Isolé" : String(permCount), permCount === 0 ? "aucune permission" : permCount > 1 ? "permissions" : "permission"),
    stat(latest ? fmtSize(latest.size_bytes) : "—", "taille"),
    stat(String(liveVersions.length), liveVersions.length > 1 ? "versions" : "version"),
    stat(usage.min_engine_version ? `${usage.min_engine_version}+` : "0.4+", "moteur requis"));

  // ---- install block ------------------------------------------------------
  const installSection = el("section", { class: "tool-install" },
    el("h2", { class: "block-title", text: "Installation" }),
    el("p", { class: "block-sub", text: "Depuis le moteur KTRX, déjà installé sur votre machine." }),
    commandBlock([`ktrx install ${tool.name}`, `ktrx run ${tool.name} --help`]));

  // ---- tabs ---------------------------------------------------------------
  function overview() {
    return body.trim() ? markdown(body) : el("p", { class: "muted", text: "Pas encore de documentation détaillée pour cet outil." });
  }

  function usagePanel() {
    const box = el("div", { class: "usage" });
    if (usage.examples?.length) {
      box.append(el("h2", { class: "block-title", text: "Exemples" }));
      for (const ex of usage.examples) box.append(commandBlock([installedForm(ex)]));
    }
    box.append(el("h2", { class: "block-title", text: "Arguments" }));
    if (!usage.arguments?.length) {
      box.append(el("p", { class: "muted", text: "Cet outil n'attend aucun argument." }));
      return box;
    }
    const list = el("div", { class: "args" });
    for (const a of usage.arguments) {
      const head = el("div", { class: "arg-head" },
        el("code", { class: "arg-name", text: a.positional ? `<${a.name}>` : `--${a.name}` }),
        el("span", { class: "arg-type", text: a.value_type }),
        a.required ? el("span", { class: "chip chip-req", text: "requis" }) : el("span", { class: "chip", text: "optionnel" }),
        a.default !== undefined && a.default !== null ? el("span", { class: "arg-default", text: `défaut : ${typeof a.default === "string" ? a.default : JSON.stringify(a.default)}` }) : null);
      const desc = el("p", { class: "arg-desc", text: a.description });
      const choices = a.choices?.length
        ? el("div", { class: "arg-choices" }, el("span", { class: "muted", text: "Valeurs : " }), ...a.choices.flatMap((c, i) => [i ? " " : "", el("code", { text: c })]))
        : null;
      list.append(el("div", { class: "arg" }, head, desc, choices));
    }
    box.append(list);
    return box;
  }

  function versionsPanel() {
    const box = el("div", { class: "timeline" });
    const versions = tool.versions ?? [];
    if (!versions.length) return el("p", { class: "muted", text: "Aucune version publiée." });
    for (const v of versions) {
      const meta = el("div", { class: "tl-meta" },
        el("code", { class: "tl-version", text: `v${v.version}` }),
        v.version === tool.latest_version && !v.yanked ? el("span", { class: "chip chip-free", text: "actuelle" }) : null,
        v.yanked ? el("span", { class: "chip chip-warn", text: "retirée" }) : null,
        el("span", { class: "muted", text: `${fmtDate(v.published_at)} · ${fmtSize(v.size_bytes)}` }));
      const body = el("div", { class: "tl-body" }, meta,
        v.changelog ? el("p", { class: "tl-note", text: v.changelog }) : null,
        v.yanked ? null : el("code", { class: "tl-install", text: `ktrx install ${tool.name}@${v.version}` }),
        el("div", { class: "tl-hash", text: `SHA-256 ${v.sha256}` }));
      box.append(el("div", { class: "tl-item" }, el("span", { class: "tl-dot" + (v.yanked ? " tl-dot-off" : "") }), body));
    }
    return box;
  }

  const panels = {
    apercu: ["Aperçu", overview],
    utilisation: ["Utilisation", usagePanel],
    versions: [`Versions`, versionsPanel],
  };
  const tabs = el("div", { class: "tabs", role: "tablist" });
  const panel = el("div", { class: "panel" });
  const cache = {};
  const select = (key) => {
    tabs.querySelectorAll("button").forEach((b) => b.classList.toggle("is-active", b.dataset.key === key));
    panel.classList.remove("panel-in");
    panel.replaceChildren((cache[key] ??= panels[key][1]()));
    requestAnimationFrame(() => panel.classList.add("panel-in"));
    history.replaceState(null, "", key === "apercu" ? location.pathname : `#${key}`);
  };
  for (const [key, [label]] of Object.entries(panels)) {
    const b = el("button", { type: "button", role: "tab", text: label });
    b.dataset.key = key;
    b.addEventListener("click", () => select(key));
    tabs.append(b);
  }

  // ---- sidebar ------------------------------------------------------------
  const download = el("div", { class: "s-card s-card-accent" },
    el("div", { class: "s-dl-head" }, el("span", { class: "muted", text: "Fichier .tool" }), latest ? el("span", { class: "muted", text: fmtSize(latest.size_bytes) }) : null),
    el("a", { class: "btn btn-primary", href: tool.download_url, text: "Télécharger l'archive" }),
    el("p", { class: "s-dl-note", text: "Vérifié par SHA-256 et validé par le moteur avant toute exécution." }),
    latest ? el("code", { class: "hash", text: latest.sha256 }) : null);

  const perms = el("div", { class: "s-card" }, el("h3", { text: permCount ? "Permissions demandées" : "Permissions" }));
  if (!permCount) {
    perms.append(el("div", { class: "perm perm-none" },
      el("div", { class: "perm-ico", html: "<svg viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M12 3 5 6v5c0 4 3 7.5 7 9 4-1.5 7-5 7-9V6z'/><path d='m9 12 2 2 4-4.5'/></svg>" }),
      el("div", {}, el("strong", { text: "Aucune" }), el("span", { text: "Entièrement isolé : ni réseau, ni fichiers, ni programmes." }))));
  } else {
    for (const p of tool.permissions) {
      const [label, help] = PERMISSIONS[p] ?? [p, ""];
      perms.append(el("div", { class: "perm" }, el("strong", { text: label }), el("span", { text: help })));
    }
    if (usage.programs?.length) perms.append(el("div", { class: "perm" }, el("strong", { text: "Programmes lancés" }), el("span", {}, ...usage.programs.flatMap((p, i) => [i ? ", " : "", el("code", { text: p })]))));
    perms.append(el("p", { class: "perm-foot", text: "Demandées à chaque exécution. Rien n'est accordé sans votre accord." }));
  }

  const facts = el("dl", { class: "facts" });
  const fact = (k, v) => v && facts.append(el("dt", { text: k }), el("dd", { text: v }));
  fact("Dernière version", tool.latest_version);
  fact("Catégorie", CATEGORIES[tool.category] ?? tool.category);
  fact("Auteur", tool.author);
  fact("Licence", usage.license);
  fact("Moteur requis", usage.min_engine_version ? `${usage.min_engine_version} ou plus` : "toute version");
  fact("Mise à jour", fmtDate(tool.updated_at));
  fact("Identifiant", tool.id);
  const info = el("div", { class: "s-card" }, el("h3", { text: "Détails" }), facts);

  const aside = el("aside", { class: "aside" }, download, perms, info);

  const layout = el("div", { class: "tool-layout" }, el("div", { class: "tool-body" }, tabs, panel), aside);

  // ---- related tools ------------------------------------------------------
  const related = el("section", { class: "related" });
  root.replaceChildren(hero, strip, installSection, layout, related);
  const initial = location.hash.slice(1);
  select(panels[initial] ? initial : "apercu");

  catalog()
    .then(({ tools }) => {
      const same = tools.filter((t) => t.id !== tool.id && t.category === tool.category);
      const picks = (same.length ? same : tools.filter((t) => t.id !== tool.id)).sort(() => Math.random() - 0.5).slice(0, 4);
      if (!picks.length) return;
      related.append(
        el("h2", { class: "block-title", text: same.length ? "Dans la même catégorie" : "Autres outils" }),
        el("div", { class: "tool-grid", style: "margin-top:20px" }, ...picks.map(toolCard)),
      );
    })
    .catch(() => {});
});
