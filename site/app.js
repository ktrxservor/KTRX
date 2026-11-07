const grid = document.querySelector("#tool-grid");
const search = document.querySelector("#search");
const resultCount = document.querySelector("#result-count");
const emptyState = document.querySelector("#empty-state");
const errorState = document.querySelector("#error-state");
const categoryFilters = document.querySelector("#category-filters");

const permissionLabels = {
  network: "Réseau",
  "file-read": "Lecture de fichiers",
};

const categories = {
  investigation: { label: "Investigation", sigil: "⌖" },
  analysis: { label: "Analyse", sigil: "✳" },
  encoding: { label: "Conversion", sigil: "⟡" },
  example: { label: "Démonstration", sigil: "✦" },
};

const toolCategories = {
  ANUBIS: "analysis",
  APOLLO: "example",
  ARGUS: "investigation",
  ASCLEPIUS: "investigation",
  ATHENA: "investigation",
  HADES: "analysis",
  HECATE: "encoding",
  HEPHAESTUS: "encoding",
  HERMES: "investigation",
  HORUS: "investigation",
  IRIS: "encoding",
  JANUS: "analysis",
  METIS: "analysis",
  ODYSSEUS: "investigation",
  POSEIDON: "investigation",
  PROMETHEUS: "investigation",
  SPHINX: "analysis",
  THEMIS: "investigation",
  THOTH: "encoding",
};

let activeCategory = "all";

function createPermission(permission) {
  const badge = document.createElement("span");
  badge.className = "permission";
  badge.textContent = permissionLabels[permission] ?? permission;
  return badge;
}

function createCard(tool) {
  const card = document.createElement("article");
  card.className = "tool-card";
  const mythName = tool.name.replace(/^K~/, "");
  const categoryKey = toolCategories[mythName] ?? "analysis";
  const category = categories[categoryKey];

  card.dataset.category = categoryKey;
  const index = document.createElement("div");
  index.className = "card-index";
  const number = document.createElement("span");
  number.textContent = `ARCHIVE ${String(tool.catalogIndex).padStart(2, "0")}`;
  const sigil = document.createElement("span");
  sigil.className = "index-sigil";
  sigil.setAttribute("aria-hidden", "true");
  sigil.textContent = category.sigil;
  index.append(number, sigil);

  const top = document.createElement("div");
  top.className = "card-title-row";
  const title = document.createElement("h3");
  const prefix = document.createElement("span");
  prefix.textContent = "K~";
  title.append(prefix, document.createTextNode(mythName));
  const version = document.createElement("span");
  version.className = "version";
  version.textContent = `V ${tool.version}`;
  top.append(title, version);

  const categoryLabel = document.createElement("span");
  categoryLabel.className = "category-label";
  categoryLabel.textContent = category.label;

  const description = document.createElement("p");
  description.textContent = tool.description;

  const bottom = document.createElement("div");
  bottom.className = "card-bottom";
  const permissions = document.createElement("div");
  permissions.className = "permissions";
  if (tool.permissions.length === 0) {
    const badge = document.createElement("span");
    badge.className = "permission none";
    badge.textContent = "Aucune permission";
    permissions.append(badge);
  } else {
    tool.permissions.forEach((permission) => permissions.append(createPermission(permission)));
  }

  const actions = document.createElement("div");
  actions.className = "card-actions";
  if (tool.readmeUrl) {
    const details = document.createElement("a");
    details.className = "details";
    details.href = tool.readmeUrl;
    details.textContent = "Détails";
    actions.append(details);
  }
  const download = document.createElement("a");
  download.className = "download";
  download.href = tool.downloadUrl;
  download.setAttribute("aria-label", `Télécharger ${tool.name}, fichier .tool`);
  const downloadIcon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  downloadIcon.setAttribute("aria-hidden", "true");
  downloadIcon.setAttribute("viewBox", "0 0 16 16");
  downloadIcon.setAttribute("fill", "none");
  downloadIcon.innerHTML = '<path d="M8 2.5v7m0 0 2.7-2.7M8 9.5 5.3 6.8M3 10.5v2h10v-2"></path>';
  const downloadLabel = document.createElement("span");
  downloadLabel.textContent = "Télécharger";
  download.append(downloadIcon, downloadLabel);
  actions.append(download);
  bottom.append(permissions, actions);

  card.append(index, top, categoryLabel, description, bottom);
  return card;
}

function render(tools) {
  const query = search.value.trim().toLocaleLowerCase("fr");
  const filtered = tools.filter((tool) =>
    (activeCategory === "all" || toolCategories[tool.name.replace(/^K~/, "")] === activeCategory)
    && `${tool.name} ${tool.description} ${tool.permissions.join(" ")} ${categories[toolCategories[tool.name.replace(/^K~/, "")] ?? "analysis"].label}`
      .toLocaleLowerCase("fr")
      .includes(query),
  );
  grid.replaceChildren(...filtered.map(createCard));
  resultCount.textContent = `${String(filtered.length).padStart(2, "0")} INSTRUMENT${filtered.length === 1 ? "" : "S"} RÉPERTORIÉ${filtered.length === 1 ? "" : "S"}`;
  emptyState.hidden = filtered.length !== 0;
}

async function loadCatalog() {
  try {
    const response = await fetch("catalog.json");
    if (!response.ok) throw new Error(`Catalog request failed: ${response.status}`);
    const catalog = await response.json();
    if (!Array.isArray(catalog.tools)) throw new Error("Catalog has an invalid format");
    const tools = catalog.tools.map((tool, index) => ({ ...tool, catalogIndex: index + 1 }));
    render(tools);
    search.addEventListener("input", () => render(tools));
    categoryFilters.addEventListener("click", (event) => {
      const button = event.target.closest("button[data-category]");
      if (!button) return;
      activeCategory = button.dataset.category;
      categoryFilters.querySelectorAll("button[data-category]").forEach((filter) => {
        const selected = filter === button;
        filter.classList.toggle("is-active", selected);
        filter.setAttribute("aria-pressed", String(selected));
      });
      render(tools);
    });
  } catch (error) {
    console.error("Unable to load the KTRX tools catalog.", error);
    resultCount.textContent = "Catalogue indisponible";
    errorState.hidden = false;
  }
}

document.addEventListener("keydown", (event) => {
  if (event.key === "/" && !["INPUT", "TEXTAREA"].includes(document.activeElement.tagName)) {
    event.preventDefault();
    search.focus();
  }
});

loadCatalog();
