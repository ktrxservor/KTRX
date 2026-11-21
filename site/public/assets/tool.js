// Tool page: /outils/<name>. README (Aperçu), usage reference from the manifest, versions.
"use strict";

document.addEventListener("DOMContentLoaded", async () => {
  const { el, codeBlock, copyButton, PERMISSIONS, CATEGORIES } = KTRX;
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
        el("p", { class: "desc", text: "Cet outil n'existe pas ou n'est plus publié." })),
    );
    return;
  }

  // README: the site shows the title itself, and the quoted line under it as the tool's lore.
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

  const { lore, body } = splitReadme(tool.readme);
  const usage = tool.usage ?? {};
  const latest = (tool.versions ?? []).find((v) => v.version === tool.latest_version);

  // ---- hero
  const icon = tool.icon_url
    ? el("img", { class: "tool-hero-icon", src: tool.icon_url, alt: "" })
    : el("div", { class: "tool-hero-icon tool-icon-fallback", text: tool.myth_name[2] });
  const hero = el("section", { class: "tool-hero" },
    el("a", { class: "back", href: "/outils", text: "‹ Tous les outils" }),
    el("div", { class: "tool-hero-main reveal is-visible" }, icon,
      el("div", {},
        el("h1", { text: tool.myth_name }),
        el("p", { class: "desc", text: tool.description }),
        lore ? el("p", { class: "lore", text: lore }) : null,
        el("div", { class: "chips" },
          el("span", { class: "chip chip-accent", text: CATEGORIES[tool.category] ?? tool.category }),
          el("span", { class: "chip", text: `Version ${tool.latest_version}` }),
          tool.permissions.length ? null : el("span", { class: "chip chip-free", text: "Aucune permission" })))),
    el("div", { class: "install", "data-install": "", "data-command": tool.install }));

  // ---- tabs
  const panels = {
    apercu: ["Aperçu", () => (body.trim() ? markdown(body) : el("p", { class: "muted", text: "Pas encore de documentation détaillée." }))],
    utilisation: ["Utilisation", usagePanel],
    versions: [`Versions`, versionsPanel],
  };
  const tabs = el("div", { class: "tabs", role: "tablist" });
  const panel = el("div", { class: "panel" });
  const cache = {};
  const select = (key) => {
    tabs.querySelectorAll("button").forEach((b) => b.classList.toggle("is-active", b.dataset.key === key));
    panel.replaceChildren((cache[key] ??= panels[key][1]()));
    history.replaceState(null, "", key === "apercu" ? location.pathname : `#${key}`);
  };
  for (const [key, [label]] of Object.entries(panels)) {
    const b = el("button", { type: "button", role: "tab", text: label });
    b.dataset.key = key;
    b.addEventListener("click", () => select(key));
    tabs.append(b);
  }

  function usagePanel() {
    const box = el("div");
    if (usage.examples?.length) {
      box.append(el("h2", { text: "Exemples" }), ...usage.examples.map((e) => codeBlock(installedForm(e))));
    }
    box.append(el("h2", { text: "Arguments" }));
    if (!usage.arguments?.length) {
      box.append(el("p", { class: "muted", text: "Cet outil n'attend aucun argument." }));
    } else {
      const rows = usage.arguments.map((a) => {
        const flags = el("div", { class: "flags" },
          a.required ? el("span", { class: "chip chip-accent", text: "requis" }) : null,
          a.default !== undefined && a.default !== null ? el("span", { class: "chip", text: `défaut : ${typeof a.default === "string" ? a.default : JSON.stringify(a.default)}` }) : null);
        return el("tr", {},
          el("td", {}, el("code", { text: a.positional ? `<${a.name}>` : `--${a.name}` }), flags),
          el("td", { class: "muted", text: a.value_type }),
          el("td", {}, a.description, a.choices?.length ? el("div", { class: "choices" }, "Valeurs : ", ...a.choices.flatMap((c, i) => [i ? ", " : "", el("code", { text: c })])) : null));
      });
      box.append(el("div", { class: "table-scroll" }, el("table", { class: "ref" },
        el("thead", {}, el("tr", {}, el("th", { text: "Argument" }), el("th", { text: "Type" }), el("th", { text: "Description" }))),
        el("tbody", {}, rows))));
    }
    box.append(el("h2", { text: "Aide dans le terminal" }), codeBlock(`ktrx run ${tool.name} --help`));
    return box;
  }

  function versionsPanel() {
    const rows = (tool.versions ?? []).map((v) => {
      const date = v.published_at ? new Date(v.published_at.replace(" ", "T").replace(/Z?$/, "Z")).toLocaleDateString("fr-FR") : "";
      return el("tr", {},
        el("td", {}, el("code", { text: v.version }), v.changelog ? el("div", { class: "choices", text: v.changelog }) : null),
        el("td", { class: "muted", text: date }),
        el("td", { class: "muted", text: `${Math.max(1, Math.round(v.size_bytes / 1024))} Ko` }),
        el("td", {}, v.yanked
          ? el("span", { class: "chip", text: "retirée" })
          : el("code", { text: `ktrx install ${tool.name}@${v.version}`, style: "font-size:12px" })));
    });
    return el("div", { class: "table-scroll" }, el("table", { class: "ref" },
      el("thead", {}, el("tr", {}, el("th", { text: "Version" }), el("th", { text: "Date" }), el("th", { text: "Taille" }), el("th", { text: "Installer" }))),
      el("tbody", {}, rows)));
  }

  // ---- aside
  const perms = el("div", { class: "card" }, el("h3", { text: "Permissions" }));
  if (!tool.permissions.length) {
    perms.append(el("p", { class: "perm" }, el("strong", { text: "Aucune" }), el("span", { text: "L'outil s'exécute entièrement isolé : ni réseau, ni fichiers, ni programmes." })));
  } else {
    for (const p of tool.permissions) {
      const [label, help] = PERMISSIONS[p] ?? [p, ""];
      perms.append(el("p", { class: "perm" }, el("strong", { text: label }), el("span", { text: help })));
    }
    if (usage.programs?.length) perms.append(el("p", { class: "perm" }, el("strong", { text: "Programmes lancés" }), el("span", { text: usage.programs.join(", ") })));
    perms.append(el("p", { class: "perm" }, el("span", { text: "Demandées à chaque exécution : rien n'est accordé sans votre accord." })));
  }

  const facts = el("dl", { class: "facts" });
  const addFact = (k, v) => v && facts.append(el("dt", { text: k }), el("dd", { text: v }));
  addFact("Version", tool.latest_version);
  addFact("Catégorie", CATEGORIES[tool.category] ?? tool.category);
  addFact("Auteur", tool.author);
  addFact("Licence", usage.license);
  addFact("Moteur requis", usage.min_engine_version ? `${usage.min_engine_version} ou plus` : "toutes versions");
  addFact("Taille", latest ? `${Math.max(1, Math.round(latest.size_bytes / 1024))} Ko` : null);

  const download = el("div", { class: "card" },
    el("h3", { text: "Fichier .tool" }),
    el("a", { class: "btn btn-secondary btn-small", href: tool.download_url, text: "Télécharger l'archive" }),
    latest ? el("div", { class: "hash", text: `SHA-256 ${latest.sha256}` }) : null);

  const aside = el("aside", { class: "aside" }, perms, el("div", { class: "card" }, el("h3", { text: "Informations" }), facts), download);
  root.replaceChildren(hero, el("div", { class: "tool-layout" }, el("div", {}, tabs, panel), aside));
  root.querySelectorAll("[data-install]").forEach((node) => {
    // site.js mounted install boxes at load; this one was added later.
    const line = el("div", { class: "install-line" }, el("span", { class: "prompt", text: "$" }), el("code", { text: tool.install }), copyButton(() => tool.install));
    node.append(line);
  });
  const initial = location.hash.slice(1);
  select(panels[initial] ? initial : "apercu");
});
