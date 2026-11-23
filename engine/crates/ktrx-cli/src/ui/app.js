// KTRX interface. Talks to the local `ktrx ui` server only; no external script, no inline style.
"use strict";

(() => {
  // ------------------------------------------------------------------ constants

  const PERMISSIONS = {
    network: ["Réseau", "Connexions DNS, TCP, TLS et UDP vers la cible que vous indiquez.", "globe", ""],
    "file-read": ["Lecture de fichiers", "Lit les fichiers que vous lui désignez.", "file", ""],
    "file-write": ["Écriture de fichiers", "Écrit uniquement dans le dossier choisi pour l'exécution.", "save", ""],
    "raw-net": ["Réseau bas niveau", "Ping ICMP, ARP, sondes TCP. Peut demander des droits administrateur.", "radar", "is-warn"],
    process: ["Programmes", "Lance des programmes de votre machine, avec vos droits, en dehors du bac à sable.", "terminal", "is-warn"],
  };
  const CATEGORIES = { investigation: "Réseau", analysis: "Analyse", encoding: "Utilitaires", generation: "Génération" };
  const TEXT_AREAS = /^(text|input|data|json|content|body|payload|message|xml|csv|html|markdown|code|query|list|lines|document|yaml|template)$/i;
  const ICONS = {
    globe: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/></svg>',
    file: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>',
    save: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12m0 0-4-4m4 4 4-4"/><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/></svg>',
    radar: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="4"/><path d="M12 12l6-6"/></svg>',
    terminal: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="m7 10 3 2-3 2M13 15h4"/></svg>',
    shield: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6z"/><path d="m9 12 2 2 4-4"/></svg>',
    folder: '<svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
    doc: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>',
    search: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>',
  };

  // ------------------------------------------------------------------ helpers

  function el(tag, props = {}, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (value == null || value === false) continue;
      if (key === "class") node.className = value;
      else if (key === "text") node.textContent = value;
      else if (key === "html") node.innerHTML = value; // only for the static icons above
      else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
      else node.setAttribute(key, value === true ? "" : value);
    }
    node.append(...children.flat().filter((c) => c != null && c !== false));
    return node;
  }
  const icon = (name) => el("span", { class: "svg-slot", html: ICONS[name] });

  function formatBytes(n) {
    if (n < 1024) return `${n} o`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} Ko`;
    return `${(n / 1024 / 1024).toFixed(1)} Mo`;
  }
  function formatTime(ms) {
    return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(ms < 10000 ? 1 : 0)} s`;
  }
  function parseVersion(v) {
    const m = /^(\d+)\.(\d+)\.(\d+)/.exec(String(v));
    return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
  }
  function isNewer(latest, current) {
    const a = parseVersion(latest);
    const b = parseVersion(current);
    if (!a || !b) return false;
    for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
    return false;
  }
  const shortName = (name) => String(name).replace(/^K~/, "");
  const plural = (n, word) => `${n} ${word}${n > 1 ? "s" : ""}`;
  const store = {
    get(key) {
      try {
        return localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    set(key, value) {
      try {
        localStorage.setItem(key, value);
      } catch {
        /* storage unavailable: the choice just is not remembered */
      }
    },
  };

  // ------------------------------------------------------------------ state and API

  const app = document.getElementById("app");
  const modalRoot = document.getElementById("modal-root");
  const header = document.getElementById("header");
  const S = {
    token: null,
    info: null,
    installed: [],
    catalog: null,
    catalogError: null,
    busy: new Set(),
    jobs: {},
    filter: { q: "", category: "all" },
    poll: null,
  };

  class ApiError extends Error {}

  async function api(path, body) {
    let response;
    try {
      response = await fetch(path, {
        method: body === undefined ? "GET" : "POST",
        headers: { "X-KTRX-Token": S.token, "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      showFatal("Connexion perdue", "Le serveur local ne répond plus. Relancez la commande ci-dessous pour rouvrir l'interface.");
      throw new ApiError("Connexion perdue");
    }
    let data = {};
    try {
      data = await response.json();
    } catch {
      /* an empty or non-JSON answer: reported below */
    }
    if (response.status === 401) {
      showFatal("Session expirée", "Ce lien n'est plus valable. Relancez la commande ci-dessous pour rouvrir l'interface.");
      throw new ApiError("Session expirée");
    }
    if (!response.ok) throw new ApiError(data.error || `Erreur ${response.status}`);
    return data;
  }

  function showFatal(title, text) {
    stopPolling();
    header.hidden = true;
    modalRoot.replaceChildren();
    app.replaceChildren(
      el("div", { class: "boot" }, el("div", {}, el("div", { class: "brand-mark", text: "K" }), el("h1", { text: title }), el("p", { text }), el("div", { class: "term", text: "$ ktrx ui" }))),
    );
  }

  // ------------------------------------------------------------------ small UI pieces

  function toast(message, bad = false, ms = 3600) {
    const root = document.getElementById("toasts");
    const node = el("div", { class: `toast${bad ? " is-bad" : ""}`, text: message });
    root.append(node);
    const remove = () => {
      node.classList.add("is-out");
      setTimeout(() => node.remove(), 320);
    };
    setTimeout(remove, bad ? Math.max(ms, 6000) : ms);
    node.addEventListener("click", remove);
  }

  async function copyText(text, button) {
    try {
      await navigator.clipboard.writeText(text);
      const old = button.textContent;
      button.textContent = "Copié";
      setTimeout(() => (button.textContent = old), 1500);
    } catch {
      toast("Copie impossible", true);
    }
  }

  function toolIcon(id, name, large = false) {
    const slot = el("span", { class: "icon-slot", "data-icon-id": id, "data-name": shortName(name), "data-large": large ? "1" : "" });
    fillIcon(slot);
    return slot;
  }
  function fillIcon(slot) {
    const id = slot.dataset.iconId;
    const large = slot.dataset.large === "1";
    const entry = S.catalog && S.catalog.find((t) => t.id === id);
    const fallback = () => el("div", { class: `tool-icon-fallback${large ? " is-large" : ""}`, text: (slot.dataset.name || "?")[0].toUpperCase() });
    if (entry && entry.icon_url) {
      const img = el("img", { class: `tool-icon${large ? " is-large" : ""}`, src: S.info.registry + entry.icon_url, alt: "", loading: "lazy", decoding: "async" });
      img.addEventListener("error", () => slot.replaceChildren(fallback()));
      slot.replaceChildren(img);
    } else {
      slot.replaceChildren(fallback());
    }
  }
  function refreshIcons() {
    document.querySelectorAll(".icon-slot").forEach(fillIcon);
  }

  function permChips(permissions) {
    if (!permissions.length) return [el("span", { class: "chip chip-good", text: "Aucune permission" })];
    return permissions.map((p) => el("span", { class: "chip", text: (PERMISSIONS[p] || [p])[0] }));
  }

  function permBlock(p) {
    const [label, text, ico, tone] = PERMISSIONS[p] || [p, "", "shield", ""];
    return el("div", { class: "perm" }, el("div", { class: `perm-ico ${tone}`, html: ICONS[ico] }), el("div", {}, el("strong", { text: label }), el("span", { text })));
  }
  function noPermBlock() {
    return el("div", { class: "perm" }, el("div", { class: "perm-ico is-good", html: ICONS.shield }), el("div", {}, el("strong", { text: "Aucune permission" }), el("span", { text: "Entièrement isolé : il ne touche ni au réseau ni à vos fichiers." })));
  }

  // ------------------------------------------------------------------ modals

  let modalStack = [];
  function openModal({ title, text, body, foot, wide = false, dismissable = true }) {
    const previous = document.activeElement;
    const overlay = el("div", { class: "overlay", role: "dialog", "aria-modal": "true" });
    const modal = el("div", { class: `modal${wide ? " is-wide" : ""}` });
    modal.append(el("div", { class: "modal-head" }, el("h2", { text: title }), text ? el("p", { text }) : null));
    if (body) modal.append(el("div", { class: "modal-body" }, body));
    if (foot) modal.append(el("div", { class: "modal-foot" }, foot));
    overlay.append(modal);
    const handle = {
      overlay,
      modal,
      close() {
        overlay.remove();
        modalStack = modalStack.filter((m) => m !== handle);
        if (previous && previous.focus) previous.focus();
      },
      dismissable,
    };
    if (dismissable) overlay.addEventListener("mousedown", (e) => e.target === overlay && handle.close());
    modalRoot.append(overlay);
    modalStack.push(handle);
    const first = modal.querySelector("input:not([type=checkbox]), textarea, .btn-primary");
    (first || modal).focus?.();
    return handle;
  }
  addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modalStack.length) {
      const top = modalStack[modalStack.length - 1];
      if (top.dismissable) top.close();
    }
  });

  function confirmModal({ title, text, confirm, danger = false }) {
    return new Promise((resolve) => {
      let done = false;
      const finish = (value) => {
        if (done) return;
        done = true;
        handle.close();
        resolve(value);
      };
      const handle = openModal({
        title,
        text,
        foot: [el("button", { class: "btn btn-ghost", type: "button", text: "Annuler", onclick: () => finish(false) }), el("button", { class: `btn ${danger ? "btn-danger" : "btn-primary"}`, type: "button", text: confirm, onclick: () => finish(true) })],
      });
      handle.overlay.addEventListener("mousedown", (e) => e.target === handle.overlay && finish(false));
    });
  }

  // ------------------------------------------------------------------ file browser

  function pickPath({ mode, start, title }) {
    return new Promise((resolve) => {
      let current = null;
      let selected = null;
      let hidden = false;
      let done = false;
      const pathInput = el("input", { class: "input mono", type: "text", spellcheck: "false", "aria-label": "Chemin" });
      const list = el("div", { class: "fb-list", role: "listbox" });
      const note = el("div", { class: "fb-note" });
      const chooseBtn = el("button", { class: "btn btn-primary", type: "button", text: "Choisir", onclick: () => finish(selected || current) });
      const dirBtn = el("button", { class: "btn btn-ghost", type: "button", text: "Choisir ce dossier", onclick: () => finish(current) });
      const finish = (value) => {
        if (done) return;
        done = true;
        handle.close();
        resolve(value || null);
      };
      const sep = () => S.info.separator;
      const join = (dir, name) => (dir.endsWith(sep()) ? dir + name : dir + sep() + name);

      async function go(path) {
        try {
          const data = await api(`/api/fs?path=${encodeURIComponent(path || "")}${hidden ? "&hidden=1" : ""}`);
          current = data.path;
          selected = null;
          pathInput.value = data.path;
          const rows = [];
          if (data.parent) rows.push(row("..", true, 0, () => go(data.parent)));
          for (const e of data.entries) {
            rows.push(
              row(e.name, e.dir, e.size, () => {
                if (e.dir) go(join(data.path, e.name));
                else if (mode === "file") {
                  selected = join(data.path, e.name);
                  list.querySelectorAll(".fb-item").forEach((n) => n.classList.remove("is-selected"));
                  rows.find((r) => r.dataset.name === e.name)?.classList.add("is-selected");
                  update();
                }
              }, () => e.dir || mode !== "file" || finish(join(data.path, e.name))),
            );
          }
          list.replaceChildren(...(rows.length ? rows : [el("div", { class: "empty", text: "Dossier vide" })]));
          list.scrollTop = 0;
          note.textContent = data.truncated ? "Liste tronquée : saisissez le chemin exact." : "";
          update();
        } catch (error) {
          if (!(error instanceof ApiError) || error.message === "Connexion perdue") return;
          toast(error.message, true);
        }
      }
      function row(name, dir, size, onClick, onDouble) {
        const node = el("button", { class: `fb-item${dir ? " is-dir" : ""}`, type: "button", "data-name": name, role: "option" }, el("span", { html: dir ? ICONS.folder : ICONS.doc }), el("span", { class: "name", text: name }), !dir ? el("span", { class: "size", text: formatBytes(size) }) : null);
        node.addEventListener("click", onClick);
        if (onDouble) node.addEventListener("dblclick", onDouble);
        return node;
      }
      function update() {
        if (mode === "dir") {
          chooseBtn.textContent = "Choisir ce dossier";
          dirBtn.hidden = true;
        } else {
          chooseBtn.textContent = selected ? "Choisir ce fichier" : "Choisir";
          chooseBtn.disabled = !selected;
          dirBtn.hidden = false;
        }
      }
      pathInput.addEventListener("keydown", (e) => e.key === "Enter" && go(pathInput.value.trim()));
      const hiddenBox = el("input", { type: "checkbox", onchange: (e) => ((hidden = e.target.checked), go(current)) });
      const handle = openModal({
        title: title || (mode === "dir" ? "Choisir un dossier" : "Choisir un fichier"),
        wide: true,
        body: [
          el("div", { class: "fb-path" }, pathInput, el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Aller", onclick: () => go(pathInput.value.trim()) }), el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Dossier personnel", onclick: () => go(S.info.home) })),
          list,
          el("label", { class: "check" }, hiddenBox, el("span", { text: "Afficher les fichiers cachés" })),
          note,
        ],
        foot: [el("button", { class: "btn btn-ghost", type: "button", text: "Annuler", onclick: () => finish(null) }), dirBtn, chooseBtn],
      });
      handle.overlay.addEventListener("mousedown", (e) => e.target === handle.overlay && finish(null));
      go(start || "");
    });
  }

  // ------------------------------------------------------------------ data loading

  async function loadInstalled() {
    S.installed = (await api("/api/installed")).tools;
  }
  async function loadCatalog(refresh = false) {
    try {
      S.catalog = (await api(`/api/catalog${refresh ? "?refresh=1" : ""}`)).tools.filter((t) => !t.deprecated);
      S.catalogError = null;
    } catch (error) {
      if (error.message === "Connexion perdue" || error.message === "Session expirée") throw error;
      S.catalogError = error.message;
    }
    refreshIcons();
  }
  function updatesAvailable() {
    if (!S.catalog) return [];
    return S.installed.filter((t) => {
      const remote = S.catalog.find((c) => c.id === t.id);
      return remote && isNewer(remote.latest_version, t.version);
    });
  }
  const remoteOf = (id) => (S.catalog || []).find((c) => c.id === id);

  // ------------------------------------------------------------------ actions

  async function installTool(id, label) {
    if (S.busy.has(id)) return;
    S.busy.add(id);
    rerender();
    try {
      await api("/api/install", { spec: id });
      await loadInstalled();
      toast(`${label} installé`);
    } catch (error) {
      toast(`Installation impossible : ${error.message}`, true);
    } finally {
      S.busy.delete(id);
      rerender();
    }
  }
  async function updateTool(id, label) {
    if (S.busy.has(id)) return;
    S.busy.add(id);
    rerender();
    try {
      await api("/api/update", { name: id });
      await loadInstalled();
      toast(`${label} mis à jour`);
    } catch (error) {
      toast(`Mise à jour impossible : ${error.message}`, true);
    } finally {
      S.busy.delete(id);
      rerender();
    }
  }
  async function updateAll() {
    if (S.busy.has("*")) return;
    S.busy.add("*");
    rerender();
    try {
      await api("/api/update", {});
      await loadInstalled();
      toast("Tous les outils sont à jour");
    } catch (error) {
      toast(`Mise à jour impossible : ${error.message}`, true);
    } finally {
      S.busy.delete("*");
      rerender();
    }
  }
  async function uninstallTool(tool) {
    const ok = await confirmModal({
      title: `Supprimer ${tool.name} ?`,
      text: "L'outil est retiré de cet ordinateur. Vous pourrez le réinstaller depuis le catalogue.",
      confirm: "Supprimer",
      danger: true,
    });
    if (!ok) return;
    try {
      await api("/api/uninstall", { name: tool.id });
      await loadInstalled();
      toast(`${tool.name} supprimé`);
      location.hash = "#/";
    } catch (error) {
      toast(`Suppression impossible : ${error.message}`, true);
    }
  }

  // ------------------------------------------------------------------ views

  function spinnerBtn(label, busyLabel, busy, props) {
    return el("button", { ...props, disabled: busy }, busy ? el("span", { class: "spinner" }) : null, busy ? busyLabel : label);
  }

  function searchBox() {
    const input = el("input", { type: "search", placeholder: "Rechercher un outil", value: S.filter.q, autocomplete: "off", spellcheck: "false", "aria-label": "Rechercher" });
    return { input, node: el("label", { class: "search" }, el("span", { html: ICONS.search }), input, el("kbd", { text: "/" })) };
  }

  function viewInstalled() {
    const updates = updatesAvailable();
    const { input, node } = searchBox();
    const grid = el("div", { class: "grid" });
    const count = el("span", { class: "count" });
    function draw() {
      const q = S.filter.q.trim().toLocaleLowerCase("fr");
      const shown = S.installed.filter((t) => `${t.name} ${t.id} ${t.description}`.toLocaleLowerCase("fr").includes(q));
      count.textContent = plural(shown.length, "outil");
      if (!S.installed.length) {
        grid.replaceChildren();
        return;
      }
      grid.replaceChildren(
        ...shown.map((t) => {
          const remote = remoteOf(t.id);
          const update = remote && isNewer(remote.latest_version, t.version);
          return el(
            "a",
            { class: "card", href: `#/tool/${encodeURIComponent(t.id)}` },
            update ? el("span", { class: "chip chip-accent badge-dot", text: `v${remote.latest_version} disponible` }) : null,
            el("div", { class: "card-head" }, toolIcon(t.id, t.name), el("div", {}, el("h3", { text: t.name }), el("div", { class: "meta", text: `v${t.version}${remote ? ` · ${CATEGORIES[remote.category] || remote.category}` : ""}` }))),
            el("p", { text: t.description }),
            el("div", { class: "card-foot" }, el("div", { class: "chips" }, permChips(t.permissions))),
          );
        }),
      );
      if (!shown.length) grid.replaceChildren(el("div", { class: "empty" }, el("h2", { text: "Aucun résultat" }), el("p", { text: "Aucun outil installé ne correspond à cette recherche." })));
    }
    input.addEventListener("input", () => {
      S.filter.q = input.value;
      draw();
    });
    const page = el(
      "section",
      { class: "page" },
      el(
        "div",
        { class: "page-head" },
        el("div", {}, el("h1", { class: "title", text: "Mes outils" }), el("p", { class: "subtitle", text: S.installed.length ? `${plural(S.installed.length, "outil")} installé${S.installed.length > 1 ? "s" : ""} sur cet ordinateur.` : "Rien d'installé pour l'instant." })),
        el("div", { class: "head-actions" }, updates.length ? spinnerBtn(`Tout mettre à jour (${updates.length})`, "Mise à jour…", S.busy.has("*"), { class: "btn btn-primary", type: "button", onclick: updateAll }) : null, el("a", { class: "btn btn-secondary", href: "#/catalog", text: "Parcourir le catalogue" })),
      ),
    );
    if (!S.installed.length) {
      page.append(el("div", { class: "empty" }, el("h2", { text: "Commencez par installer un outil" }), el("p", { text: "Le catalogue réunit des outils d'analyse, de réseau, de conversion et de génération, tous isolés dans un bac à sable." }), el("a", { class: "btn btn-primary", href: "#/catalog", text: "Ouvrir le catalogue" })));
    } else {
      page.append(el("div", { class: "toolbar" }, node, count), grid);
    }
    draw();
    return page;
  }

  function viewCatalog() {
    const { input, node } = searchBox();
    const grid = el("div", { class: "grid" });
    const count = el("span", { class: "count" });
    const filters = el("div", { class: "segmented", role: "tablist" });
    const page = el("section", { class: "page" });
    page.append(
      el(
        "div",
        { class: "page-head" },
        el("div", {}, el("h1", { class: "title", text: "Catalogue" }), el("p", { class: "subtitle", text: "Installez un outil en un clic. Chaque outil annonce ses permissions avant toute exécution." })),
        el("div", { class: "head-actions" }, el("button", { class: "btn btn-ghost", type: "button", text: "Actualiser", onclick: async () => (await loadCatalog(true), rerender()) })),
      ),
    );
    if (!S.catalog) {
      if (S.catalogError) {
        page.append(el("div", { class: "empty" }, el("h2", { text: "Catalogue indisponible" }), el("p", { text: `${S.catalogError}. Vérifiez votre connexion, puis réessayez.` }), el("button", { class: "btn btn-primary", type: "button", text: "Réessayer", onclick: async () => (await loadCatalog(true), rerender()) })));
      } else {
        page.append(el("div", { class: "grid" }, ...Array.from({ length: 6 }, () => el("div", { class: "skeleton" }))));
      }
      return page;
    }
    const present = new Set(S.catalog.map((t) => t.category));
    const choices = [["all", "Tous"], ...Object.entries(CATEGORIES).filter(([k]) => present.has(k))];
    for (const [key, label] of choices) {
      filters.append(
        el("button", {
          type: "button",
          "data-key": key,
          text: label,
          onclick: () => {
            S.filter.category = key;
            draw();
          },
        }),
      );
    }
    function draw() {
      const q = S.filter.q.trim().toLocaleLowerCase("fr");
      const shown = S.catalog.filter((t) => (S.filter.category === "all" || t.category === S.filter.category) && `${t.myth_name} ${t.name} ${t.id} ${t.description} ${(t.tags || []).join(" ")}`.toLocaleLowerCase("fr").includes(q));
      filters.querySelectorAll("button").forEach((b) => b.classList.toggle("is-active", b.dataset.key === S.filter.category));
      count.textContent = plural(shown.length, "outil");
      grid.replaceChildren(...shown.map(catalogCard));
      if (!shown.length) grid.replaceChildren(el("div", { class: "empty" }, el("h2", { text: "Aucun résultat" }), el("p", { text: "Essayez un autre mot-clé ou une autre catégorie." })));
    }
    input.addEventListener("input", () => {
      S.filter.q = input.value;
      draw();
    });
    page.append(el("div", { class: "toolbar" }, node, filters, count), grid);
    draw();
    return page;
  }

  function catalogCard(t) {
    const local = S.installed.find((i) => i.id === t.id);
    const update = local && isNewer(t.latest_version, local.version);
    const busy = S.busy.has(t.id);
    let action;
    if (!local) action = spinnerBtn("Installer", "Installation…", busy, { class: "btn btn-primary btn-small", type: "button", onclick: () => installTool(t.id, t.myth_name) });
    else if (update) action = spinnerBtn(`Mettre à jour`, "Mise à jour…", busy, { class: "btn btn-primary btn-small", type: "button", onclick: () => updateTool(t.id, t.myth_name) });
    else action = el("a", { class: "btn btn-secondary btn-small", href: `#/tool/${encodeURIComponent(t.id)}`, text: "Ouvrir" });
    return el(
      "div",
      { class: "card" },
      el("div", { class: "card-head" }, toolIcon(t.id, t.myth_name), el("div", {}, el("h3", { text: t.myth_name }), el("div", { class: "meta", text: `${CATEGORIES[t.category] || t.category} · v${t.latest_version}${local ? ` · installé v${local.version}` : ""}` }))),
      el("p", { text: t.description }),
      el("div", { class: "chips" }, permChips(t.permissions || [])),
      el("div", { class: "card-foot" }, el("a", { class: "meta", href: S.info.registry + t.page_url, target: "_blank", rel: "noopener noreferrer", text: "Détails sur le site ›" }), action),
    );
  }

  // ---- a tool: form, run, result

  function viewTool(id) {
    const tool = S.installed.find((t) => t.id === id || shortName(t.name).toLowerCase() === id.toLowerCase());
    if (!tool) {
      return el("section", { class: "page" }, el("div", { class: "empty" }, el("h2", { text: "Outil introuvable" }), el("p", { text: "Cet outil n'est pas installé sur cet ordinateur." }), el("a", { class: "btn btn-primary", href: "#/catalog", text: "Ouvrir le catalogue" })));
    }
    const remote = remoteOf(tool.id);
    const update = remote && isNewer(remote.latest_version, tool.version);
    const fields = new Map();
    const form = el("form", { class: "form", novalidate: "novalidate" });
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      submit();
    });

    for (const a of tool.arguments) {
      const field = buildField(a);
      fields.set(a.name, field);
      form.append(field.node);
    }
    const runBtn = el("button", { class: "btn btn-primary", type: "submit" }, "Exécuter");
    form.append(el("div", { class: "form-foot" }, runBtn, el("span", { class: "faint", text: navigator.platform.includes("Mac") ? "⌘ ↵" : "Ctrl ↵" })));
    form.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        submit();
      }
    });
    if (!tool.arguments.length) form.prepend(el("p", { class: "muted", text: "Cet outil n'a aucun paramètre : lancez-le directement." }));

    const resultHost = el("div", {});
    const formPanel = el("div", { class: "panel" }, el("h2", { text: "Exécuter" }), form);
    const examples = tool.examples.length
      ? el("div", { class: "panel" }, el("h2", { text: "Exemples en ligne de commande" }), el("div", { class: "examples" }, tool.examples.map((x) => el("div", { class: "example" }, el("code", { text: x }), el("button", { class: "copy", type: "button", text: "Copier", onclick: (e) => copyText(x, e.target) })))))
      : null;

    const aside = el(
      "aside",
      { class: "aside" },
      el("div", { class: "s-card" }, el("h3", { text: "Permissions" }), ...(tool.permissions.length ? tool.permissions.map(permBlock) : [noPermBlock()])),
      el("div", { class: "s-card" }, el("h3", { text: "Informations" }), kv("Version", `v${tool.version}`), kv("Auteur", tool.author), kv("Taille", formatBytes(tool.bytes)), tool.timeout_seconds ? kv("Délai maximal", `${tool.timeout_seconds} s`) : null, tool.min_engine_version ? kv("Moteur requis", `≥ ${tool.min_engine_version}`) : null, remote ? el("a", { class: "meta", href: S.info.registry + remote.page_url, target: "_blank", rel: "noopener noreferrer", text: "Page sur le site ›" }) : null),
      el("div", { class: "actions" }, update ? spinnerBtn(`Mettre à jour vers v${remote.latest_version}`, "Mise à jour…", S.busy.has(tool.id), { class: "btn btn-primary btn-block", type: "button", onclick: () => updateTool(tool.id, tool.name) }) : null, el("button", { class: "btn btn-danger btn-block", type: "button", text: "Supprimer l'outil", onclick: () => uninstallTool(tool) })),
    );

    function collect() {
      const values = {};
      let firstBad = null;
      let message = null;
      for (const [name, f] of fields) {
        const v = f.read();
        const arg = f.arg;
        f.setInvalid(false);
        const empty = v === "" || v === undefined;
        if (arg.required && arg.type !== "boolean" && empty && arg.default == null) {
          f.setInvalid(true, "Ce champ est requis.");
          firstBad ||= f;
        } else if (arg.type === "integer" && !empty && !/^-?\d+$/.test(String(v).trim())) {
          f.setInvalid(true, "Entrez un nombre entier.");
          firstBad ||= f;
        }
        if (!empty) values[name] = typeof v === "string" ? v : v;
      }
      if (firstBad) {
        firstBad.focus();
        message = "Complétez les champs signalés.";
      }
      return { values, message };
    }

    async function submit() {
      if (runBtn.disabled) return;
      const { values, message } = collect();
      if (message) return toast(message, true);
      let approval = {};
      if (tool.permissions.length) {
        approval = await askApproval(tool);
        if (!approval) return;
      }
      runBtn.disabled = true;
      try {
        const { job } = await api("/api/run", { tool: tool.id, values, approved: tool.permissions, write_dir: approval.write_dir, programs: approval.programs, any_program: approval.any_program });
        S.jobs[tool.id] = { id: job, write_dir: approval.write_dir || null };
        trackJob(tool, resultHost);
        resultHost.scrollIntoView({ behavior: "smooth", block: "nearest" });
      } catch (error) {
        toast(error.message, true);
      } finally {
        runBtn.disabled = false;
      }
    }

    const page = el(
      "section",
      { class: "page" },
      el("a", { class: "crumb", href: "#/", text: "Mes outils" }),
      el("div", { class: "tool-head" }, toolIcon(tool.id, tool.name, true), el("div", {}, el("h1", { class: "title", text: tool.name }), el("p", { class: "subtitle", text: tool.description }))),
      el("div", { class: "layout" }, el("div", {}, formPanel, resultHost, examples), aside),
    );
    if (S.jobs[tool.id]) trackJob(tool, resultHost);
    queueMicrotask(() => [...fields.values()][0]?.focus());
    return page;
  }

  const kv = (k, v) => el("div", { class: "kv" }, el("span", { text: k }), el("span", { text: v }));

  function buildField(arg) {
    const hint = arg.description ? el("p", { class: "hint", text: arg.description }) : null;
    const err = el("p", { class: "error-text", hidden: true });
    const defaultText = arg.default == null ? "" : String(arg.default);
    if (arg.type === "boolean") {
      const box = el("input", { type: "checkbox", id: `f-${arg.name}` });
      box.checked = arg.default === true;
      return {
        arg,
        node: el("div", { class: "field" }, el("div", { class: "switch" }, el("div", {}, el("div", { class: "label", text: arg.name }), hint), el("span", { class: "toggle" }, box, el("i")))),
        read: () => box.checked,
        focus: () => box.focus(),
        setInvalid: () => {},
      };
    }
    let input;
    if (TEXT_AREAS.test(arg.name) && arg.type === "string") {
      input = el("textarea", { class: "textarea mono", id: `f-${arg.name}`, rows: "5", spellcheck: "false", placeholder: defaultText });
    } else {
      input = el("input", { class: `input${arg.type === "path" ? " mono" : ""}`, id: `f-${arg.name}`, type: "text", autocomplete: "off", autocapitalize: "off", spellcheck: "false", inputmode: arg.type === "integer" ? "numeric" : null, placeholder: defaultText });
    }
    if (defaultText && arg.type !== "path") input.value = "";
    let control = input;
    if (arg.type === "path") {
      const browse = el("button", {
        class: "btn btn-ghost",
        type: "button",
        text: "Parcourir",
        onclick: async () => {
          const picked = await pickPath({ mode: "file", start: dirOf(input.value) });
          if (picked) input.value = picked;
          input.focus();
        },
      });
      control = el("div", { class: "input-row" }, input, browse);
    }
    const label = el("label", { for: `f-${arg.name}` }, arg.name, arg.required && arg.default == null ? el("span", { class: "req", text: "requis" }) : el("span", { class: "opt", text: arg.default != null ? `par défaut : ${defaultText}` : "facultatif" }));
    return {
      arg,
      node: el("div", { class: "field" }, label, control, hint, err),
      read: () => input.value.trim() === "" ? "" : (arg.type === "string" && input.tagName === "TEXTAREA" ? input.value : input.value.trim()),
      focus: () => input.focus(),
      setInvalid(bad, message) {
        input.classList.toggle("is-invalid", bad);
        err.hidden = !bad;
        err.textContent = message || "";
      },
    };
  }

  function dirOf(path) {
    const p = String(path || "").trim();
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i > 0 ? p.slice(0, i) : "";
  }

  // ---- approval sheet: the permissions of this run, shown before anything starts

  function askApproval(tool) {
    return new Promise((resolve) => {
      let done = false;
      const finish = (value) => {
        if (done) return;
        done = true;
        handle.close();
        resolve(value);
      };
      const wants = (p) => tool.permissions.includes(p);
      const body = [el("div", { class: "perm-list" }, tool.permissions.map(permBlock))];
      let writeInput;
      let anyBox;
      const programBoxes = [];
      if (wants("file-write")) {
        writeInput = el("input", { class: "input mono", type: "text", spellcheck: "false", value: store.get("ktrx-write-dir") || S.info.write_dir });
        body.push(
          el(
            "div",
            { class: "field" },
            el("label", { for: "w-dir", text: "Dossier d'écriture" }),
            el("div", { class: "input-row" }, writeInput, el("button", { class: "btn btn-ghost", type: "button", text: "Parcourir", onclick: async () => {
              const dir = await pickPath({ mode: "dir", start: writeInput.value, title: "Dossier d'écriture" });
              if (dir) writeInput.value = dir;
            } })),
            el("p", { class: "hint", text: "L'outil ne peut créer des fichiers que dans ce dossier. Il est créé s'il n'existe pas." }),
          ),
        );
      }
      if (wants("process")) {
        if (tool.programs.length) {
          body.push(
            el("div", { class: "field" }, el("div", { class: "label", text: "Programmes autorisés" }), ...tool.programs.map((p) => {
              const box = el("input", { type: "checkbox", checked: "checked", "data-program": p });
              programBoxes.push(box);
              return el("label", { class: "check" }, box, el("span", {}, el("code", { text: p })));
            })),
          );
        } else {
          anyBox = el("input", { type: "checkbox" });
          body.push(
            el("div", { class: "banner banner-warn", text: "Ce outil ne limite pas les programmes qu'il peut lancer : il pourrait démarrer n'importe lequel, avec vos droits." }),
            el("label", { class: "check" }, anyBox, el("span", { text: "J'autorise cet outil à lancer n'importe quel programme pour cette exécution." })),
          );
        }
      }
      const okBtn = el("button", {
        class: "btn btn-primary",
        type: "button",
        text: "Autoriser et exécuter",
        onclick: () => {
          const result = {};
          if (writeInput) {
            const dir = writeInput.value.trim();
            if (!dir) return toast("Choisissez un dossier d'écriture.", true);
            result.write_dir = dir;
            store.set("ktrx-write-dir", dir);
          }
          if (wants("process")) {
            if (anyBox) {
              if (!anyBox.checked) return toast("Cochez la case pour autoriser l'exécution de programmes.", true);
              result.any_program = true;
            } else {
              result.programs = programBoxes.filter((b) => b.checked).map((b) => b.dataset.program);
              if (!result.programs.length) return toast("Choisissez au moins un programme.", true);
            }
          }
          finish(result);
        },
      });
      const handle = openModal({
        title: `Autoriser ${tool.name} ?`,
        text: "Ces permissions ne valent que pour cette exécution.",
        body,
        foot: [el("button", { class: "btn btn-ghost", type: "button", text: "Annuler", onclick: () => finish(null) }), okBtn],
      });
      handle.overlay.addEventListener("mousedown", (e) => e.target === handle.overlay && finish(null));
    });
  }

  // ---- following a run

  function stopPolling() {
    if (S.poll) clearTimeout(S.poll);
    S.poll = null;
  }

  function trackJob(tool, host) {
    stopPolling();
    const ref = S.jobs[tool.id];
    let tab = "pretty";
    let last = null;
    const draw = () => host.replaceChildren(renderRun(tool, last, ref, tab, (t) => ((tab = t), draw())));
    async function tick() {
      let job;
      try {
        job = await api(`/api/jobs/${ref.id}`);
      } catch (error) {
        if (error.message === "exécution introuvable") {
          delete S.jobs[tool.id];
          host.replaceChildren();
        }
        return;
      }
      if (S.jobs[tool.id] !== ref || !host.isConnected) return;
      last = job;
      draw();
      if (job.status === "running") S.poll = setTimeout(tick, 280);
    }
    tick();
  }

  function renderRun(tool, job, ref, tab, setTab) {
    const running = job.status === "running";
    const parsedOut = job.output != null ? tryParse(job.output) : { ok: false };
    // A tool reports its own failure as {"error": "..."}: show it as a failure, not a success.
    const toolError = parsedOut.ok && parsedOut.value && typeof parsedOut.value === "object" && !Array.isArray(parsedOut.value) && Object.keys(parsedOut.value).length === 1 && typeof parsedOut.value.error === "string" ? parsedOut.value.error : null;
    const status = job.status === "done" && toolError ? "failed" : job.status;
    const label = { running: "En cours", done: "Terminé", failed: "Échec", cancelled: "Annulé" }[status] || status;
    const panel = el("div", { class: "panel" });
    panel.append(
      el(
        "div",
        { class: "run-status" },
        el("span", { class: `dot is-${status}` }),
        el("strong", { text: label }),
        el("span", { class: "muted", text: formatTime(job.elapsed_ms) }),
        running
          ? el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Annuler", onclick: async () => {
              try {
                await api(`/api/jobs/${job.id}/cancel`, {});
              } catch (error) {
                toast(error.message, true);
              }
            } })
          : null,
      ),
    );
    const last = job.progress[job.progress.length - 1];
    if (running && last !== undefined) {
      const pct = progressPercent(last);
      if (pct != null) {
        const bar = el("i");
        bar.style.width = `${pct}%`;
        panel.append(el("div", { class: "progress-bar" }, bar));
      }
    }
    if (job.progress.length || job.processes.length) {
      const lines = [...job.processes.map((p) => `▶ ${p.program} ${(p.args || []).join(" ")}`), ...job.progress.slice(-30).map((p) => (typeof p === "string" ? p : JSON.stringify(p)))];
      const log = el("div", { class: "log", text: lines.join("\n") });
      panel.append(log);
      queueMicrotask(() => (log.scrollTop = log.scrollHeight));
    }
    if (job.status === "failed" || job.status === "cancelled") {
      panel.append(el("div", { class: `banner ${job.status === "failed" ? "banner-bad" : "banner-warn"}`, text: job.status === "cancelled" ? "L'exécution a été annulée." : job.error || "L'outil a échoué." }));
    }
    if (job.output != null) {
      const parsed = parsedOut;
      if (toolError) {
        panel.append(el("div", { class: "banner banner-bad", text: toolError }));
      } else {
        const pretty = parsed.ok ? JSON.stringify(parsed.value, null, 2) : job.output;
        const shown = tab === "raw" ? job.output : pretty;
        const tabs = el("div", { class: "tabs" }, ...[["pretty", "Résultat"], ["raw", "Brut"]].map(([k, l]) => el("button", { type: "button", class: tab === k ? "is-active" : "", text: l, onclick: () => setTab(k) })));
        const actions = el(
          "div",
          { class: "result-actions" },
          ref.write_dir && tool.permissions.includes("file-write")
            ? el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Ouvrir le dossier", onclick: async () => {
                try {
                  await api("/api/open", { path: ref.write_dir });
                } catch (error) {
                  toast(error.message, true);
                }
              } })
            : null,
          el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Copier", onclick: (e) => copyText(shown, e.target) }),
          el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Télécharger", onclick: () => download(`${tool.id}.json`, pretty) }),
        );
        panel.append(el("div", { class: "result-bar" }, tabs, actions), el("pre", { class: "code" }, parsed.ok && tab === "pretty" && shown.length < 300000 ? highlightJson(shown) : shown));
      }
    }
    return panel;
  }

  function progressPercent(p) {
    if (p && typeof p === "object") {
      for (const key of ["percent", "percentage", "progress", "pct"]) {
        const v = p[key];
        if (typeof v === "number" && isFinite(v)) return Math.max(0, Math.min(100, v <= 1 && key === "progress" ? v * 100 : v));
      }
    }
    return null;
  }

  function tryParse(text) {
    try {
      return { ok: true, value: JSON.parse(text) };
    } catch {
      return { ok: false };
    }
  }

  function download(name, text) {
    const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
    const a = el("a", { href: url, download: name });
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function highlightJson(text) {
    const frag = document.createDocumentFragment();
    const re = /("(?:\\.|[^"\\])*")(\s*:)?|\b(true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|[{}[\],:]/g;
    let last = 0;
    for (const m of text.matchAll(re)) {
      if (m.index > last) frag.append(text.slice(last, m.index));
      let cls = "j-punct";
      if (m[1] !== undefined) {
        frag.append(el("span", { class: m[2] ? "j-key" : "j-str", text: m[1] }));
        if (m[2]) frag.append(el("span", { class: "j-punct", text: m[2] }));
        last = m.index + m[0].length;
        continue;
      }
      if (m[3] !== undefined) cls = "j-lit";
      else if (/^-?\d/.test(m[0])) cls = "j-num";
      frag.append(el("span", { class: cls, text: m[0] }));
      last = m.index + m[0].length;
    }
    if (last < text.length) frag.append(text.slice(last));
    return frag;
  }

  // ------------------------------------------------------------------ settings

  function openSettings() {
    const writeInput = el("input", { class: "input mono", type: "text", spellcheck: "false", value: store.get("ktrx-write-dir") || S.info.write_dir });
    const updates = updatesAvailable();
    const handle = openModal({
      title: "Réglages",
      body: [
        el("div", { class: "field" }, el("label", { for: "x", text: "Dossier d'écriture par défaut" }), el("div", { class: "input-row" }, writeInput, el("button", { class: "btn btn-ghost", type: "button", text: "Parcourir", onclick: async () => {
          const dir = await pickPath({ mode: "dir", start: writeInput.value, title: "Dossier d'écriture" });
          if (dir) writeInput.value = dir;
        } })), el("p", { class: "hint", text: "Proposé aux outils qui écrivent des fichiers ; vous le confirmez à chaque exécution." })),
        el("div", { class: "kvlist" }, ...[["Version du moteur", `KTRX ${S.info.version}`], ["Système", `${S.info.os} · ${S.info.arch}`], ["Outils installés dans", S.info.install_dir], ["Registre", S.info.registry]].map(([k, v]) => el("div", {}, el("dt", { text: k }), el("dd", { text: v })))),
        el("div", { class: "actions" }, spinnerBtn(updates.length ? `Tout mettre à jour (${updates.length})` : "Tout est à jour", "Mise à jour…", S.busy.has("*"), { class: "btn btn-ghost", type: "button", disabled: !updates.length, onclick: async () => (handle.close(), updateAll()) })),
      ],
      foot: [el("button", { class: "btn btn-ghost", type: "button", text: "Fermer", onclick: () => handle.close() }), el("button", { class: "btn btn-primary", type: "button", text: "Enregistrer", onclick: () => {
        const dir = writeInput.value.trim();
        if (dir) store.set("ktrx-write-dir", dir);
        handle.close();
        toast("Réglages enregistrés");
      } })],
    });
  }

  // ------------------------------------------------------------------ router

  function route() {
    const hash = location.hash.replace(/^#/, "") || "/";
    const parts = hash.split("/").filter(Boolean);
    return { name: parts[0] || "installed", arg: parts[1] ? decodeURIComponent(parts[1]) : null };
  }

  function rerender() {
    if (!S.info) return;
    stopPolling();
    const r = route();
    document.querySelectorAll(".nav a").forEach((a) => {
      const active = (a.dataset.route === "installed" && (r.name === "installed" || r.name === "tool")) || a.dataset.route === r.name;
      if (active) a.setAttribute("aria-current", "page");
      else a.removeAttribute("aria-current");
    });
    let view;
    if (r.name === "catalog") view = viewCatalog();
    else if (r.name === "tool" && r.arg) view = viewTool(r.arg);
    else view = viewInstalled();
    const scroll = window.scrollY;
    // The entrance animation plays when the page changes, not when data refreshes it.
    if (!S.fresh) view.classList.add("no-anim");
    S.fresh = false;
    app.replaceChildren(view);
    if (!view.classList.contains("no-anim")) window.scrollTo(0, 0);
    else window.scrollTo(0, scroll);
  }

  addEventListener("hashchange", () => {
    modalStack.forEach((m) => m.close());
    S.fresh = true;
    rerender();
  });
  addEventListener("scroll", () => header.classList.toggle("is-scrolled", window.scrollY > 4), { passive: true });
  addEventListener("keydown", (e) => {
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "");
    if (e.key === "/" && !typing && !modalStack.length) {
      const search = document.querySelector(".search input");
      if (search) {
        e.preventDefault();
        search.focus();
      }
    }
  });
  document.getElementById("settings-btn").addEventListener("click", () => S.info && openSettings());

  // ------------------------------------------------------------------ start

  async function start() {
    const fromUrl = /[#&]t=([0-9a-f]{32,})/.exec(location.hash);
    if (fromUrl) {
      try {
        sessionStorage.setItem("ktrx-token", fromUrl[1]);
      } catch {
        /* kept in memory only */
      }
      S.token = fromUrl[1];
      history.replaceState(null, "", location.pathname + "#/");
    } else {
      try {
        S.token = sessionStorage.getItem("ktrx-token");
      } catch {
        S.token = null;
      }
    }
    if (!S.token) {
      showFatal("Ouvrez l'interface avec KTRX", "Cette page se lance depuis un terminal : la commande ouvre le navigateur avec le bon lien.");
      return;
    }
    app.replaceChildren(el("div", { class: "boot" }, el("div", {}, el("span", { class: "spinner" }))));
    try {
      [S.info] = await Promise.all([api("/api/state"), loadInstalled()]);
    } catch {
      return;
    }
    header.hidden = false;
    S.fresh = true;
    rerender();
    loadCatalog().then(() => {
      // Update badges and the catalogue need the registry; refresh without losing a form being filled in.
      const r = route();
      if (r.name === "catalog" || r.name === "installed" || !r.name) rerender();
      else refreshIcons();
    }).catch(() => {});
  }

  start();
})();
