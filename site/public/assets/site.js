// Shared by every page: header, scroll animations, install command, copy buttons, tool cards.
"use strict";

const KTRX = (() => {
  const SITE = location.origin;
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;

  const INSTALL = {
    unix: { label: "macOS et Linux", command: `curl -fsSL ${SITE}/install.sh | sh`, note: "Dans le Terminal. Aucun droit administrateur, rien d'autre à installer." },
    windows: { label: "Windows", command: `irm ${SITE}/install.ps1 | iex`, note: "Dans PowerShell. Aucun droit administrateur, rien d'autre à installer." },
  };

  const PERMISSIONS = {
    network: ["Réseau", "Connexions DNS, TCP, TLS et UDP vers la cible que vous indiquez."],
    "file-read": ["Lecture de fichiers", "Lit les fichiers que vous lui désignez."],
    "file-write": ["Écriture de fichiers", "Écrit uniquement dans le dossier choisi pour l'exécution."],
    "raw-net": ["Réseau bas niveau", "Ping ICMP, ARP, sondes TCP. Peut demander des droits administrateur."],
    process: ["Programmes", "Lance des programmes de votre machine, avec vos droits."],
  };

  const CATEGORIES = { investigation: "Réseau", analysis: "Analyse", encoding: "Utilitaires", generation: "Génération" };

  function el(tag, props = {}, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (value == null || value === false) continue;
      if (key === "class") node.className = value;
      else if (key === "text") node.textContent = value;
      else if (key === "html") node.innerHTML = value;
      else node.setAttribute(key, value);
    }
    node.append(...children.flat().filter((c) => c != null && c !== false));
    return node;
  }

  async function copy(text, button) {
    try {
      await navigator.clipboard.writeText(text);
      button.textContent = "Copié";
      button.classList.add("is-done");
    } catch {
      button.textContent = "Échec";
    }
    setTimeout(() => {
      button.textContent = "Copier";
      button.classList.remove("is-done");
    }, 1600);
  }

  function copyButton(getText) {
    const button = el("button", { class: "copy", type: "button", text: "Copier" });
    button.addEventListener("click", () => copy(getText(), button));
    return button;
  }

  function codeBlock(text) {
    const pre = el("pre", {}, el("code", { text }));
    return el("div", { class: "code" }, pre, copyButton(() => text));
  }

  function detectOs() {
    return /windows/i.test(navigator.userAgent) ? "windows" : "unix";
  }

  /** <div data-install> … </div> becomes the install command with its OS switch. */
  function mountInstall(root) {
    const command = (os) => (root.dataset.command ? root.dataset.command : INSTALL[os].command);
    const code = el("code");
    const note = el("p", { class: "install-note" });
    let current = detectOs();
    const switcher = el("div", { class: "os-switch", role: "tablist" });
    const render = () => {
      code.textContent = command(current);
      note.textContent = INSTALL[current].note;
      switcher.querySelectorAll("button").forEach((b) => b.classList.toggle("is-active", b.dataset.os === current));
    };
    if (!root.dataset.command) {
      for (const [os, info] of Object.entries(INSTALL)) {
        const b = el("button", { type: "button", text: info.label });
        b.dataset.os = os;
        b.addEventListener("click", () => {
          current = os;
          render();
        });
        switcher.append(b);
      }
      root.append(switcher);
    }
    const line = el("div", { class: "install-line" }, el("span", { class: "prompt", text: current === "windows" ? ">" : "$" }), code, copyButton(() => code.textContent));
    root.append(line);
    if (!root.dataset.command && !root.hasAttribute("data-no-note")) root.append(note);
    render();
  }

  function toolCard(tool) {
    const name = tool.myth_name.replace(/^K~/, "");
    const icon = tool.icon_url
      ? el("img", { class: "tool-icon", src: tool.icon_url, alt: "", loading: "lazy", decoding: "async" })
      : el("div", { class: "tool-icon-fallback", text: name[0] });
    const permissions = tool.permissions.length
      ? tool.permissions.map((p) => el("span", { class: "chip", text: (PERMISSIONS[p] ?? [p])[0] }))
      : [el("span", { class: "chip chip-free", text: "Aucune permission" })];
    return el(
      "a",
      { class: "tool-card", href: tool.page_url },
      el("div", { class: "tool-card-head" }, icon, el("div", {}, el("h3", { text: tool.myth_name }), el("div", { class: "meta", text: `${CATEGORIES[tool.category] ?? tool.category} · v${tool.latest_version}` }))),
      el("p", { text: tool.description }),
      el("div", { class: "chips" }, permissions),
    );
  }

  let catalogPromise;
  function catalog() {
    catalogPromise ??= fetch("/api/tools").then((r) => {
      if (!r.ok) throw new Error(String(r.status));
      return r.json();
    });
    return catalogPromise;
  }

  function reveal(root = document) {
    const items = root.querySelectorAll(".reveal, .reveal-scale");
    if (reducedMotion || !("IntersectionObserver" in window)) {
      items.forEach((i) => i.classList.add("is-visible"));
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            observer.unobserve(entry.target);
          }
        }
      },
      { rootMargin: "0px 0px -8% 0px", threshold: 0.12 },
    );
    items.forEach((i) => observer.observe(i));
  }

  function header() {
    const bar = document.querySelector(".site-header");
    const onScroll = () => bar?.classList.toggle("is-scrolled", scrollY > 4);
    addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    const toggle = document.querySelector(".menu-toggle");
    const nav = document.querySelector(".nav");
    toggle?.addEventListener("click", () => {
      const open = nav.classList.toggle("is-open");
      toggle.setAttribute("aria-expanded", String(open));
    });
    const here = location.pathname.replace(/\/$/, "") || "/";
    document.querySelectorAll(".nav a").forEach((a) => {
      const target = new URL(a.href).pathname.replace(/\/$/, "") || "/";
      if (target !== "/" && (here === target || here.startsWith(target + "/"))) a.setAttribute("aria-current", "page");
    });
  }

  document.addEventListener("DOMContentLoaded", () => {
    header();
    document.querySelectorAll("[data-install]").forEach(mountInstall);
    document.querySelectorAll("[data-copy]").forEach((b) => b.addEventListener("click", () => copy(b.dataset.copy, b)));
    reveal();
    const version = document.querySelectorAll("[data-engine-version]");
    if (version.length) {
      fetch("/api/engine")
        .then((r) => r.json())
        .then((e) => e.version && version.forEach((v) => (v.textContent = `Moteur ${e.version}`)))
        .catch(() => {});
    }
  });

  return { el, copyButton, codeBlock, toolCard, catalog, reveal, PERMISSIONS, CATEGORIES, INSTALL, reducedMotion, SITE };
})();
