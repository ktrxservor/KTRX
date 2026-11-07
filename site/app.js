const grid = document.querySelector("#tool-grid");
const search = document.querySelector("#search");
const resultCount = document.querySelector("#result-count");
const emptyState = document.querySelector("#empty-state");
const errorState = document.querySelector("#error-state");

const permissionLabels = {
  network: "Réseau",
  "file-read": "Lecture de fichiers",
};

function createPermission(permission) {
  const badge = document.createElement("span");
  badge.className = "permission";
  badge.textContent = permissionLabels[permission] ?? permission;
  return badge;
}

function createCard(tool) {
  const card = document.createElement("article");
  card.className = "tool-card";

  const top = document.createElement("div");
  top.className = "card-top";
  const title = document.createElement("h2");
  title.textContent = tool.name;
  const version = document.createElement("span");
  version.className = "version";
  version.textContent = `v${tool.version}`;
  top.append(title, version);

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
  download.setAttribute("download", "");
  download.setAttribute("aria-label", `Télécharger ${tool.name}, fichier .tool`);
  download.innerHTML = '<svg aria-hidden="true" viewBox="0 0 16 16" fill="none"><path d="M8 2.5v7m0 0 2.7-2.7M8 9.5 5.3 6.8M3 10.5v2h10v-2"></path></svg><span>Télécharger</span>';
  actions.append(download);
  bottom.append(permissions, actions);

  card.append(top, description, bottom);
  return card;
}

function render(tools) {
  const query = search.value.trim().toLocaleLowerCase("fr");
  const filtered = tools.filter((tool) =>
    `${tool.name} ${tool.description} ${tool.permissions.join(" ")}`.toLocaleLowerCase("fr").includes(query),
  );
  grid.replaceChildren(...filtered.map(createCard));
  resultCount.textContent = `${filtered.length} outil${filtered.length === 1 ? "" : "s"}`;
  emptyState.hidden = filtered.length !== 0;
}

async function loadCatalog() {
  try {
    const response = await fetch("catalog.json");
    if (!response.ok) throw new Error(`Catalog request failed: ${response.status}`);
    const catalog = await response.json();
    if (!Array.isArray(catalog.tools)) throw new Error("Catalog has an invalid format");
    render(catalog.tools);
    search.addEventListener("input", () => render(catalog.tools));
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
