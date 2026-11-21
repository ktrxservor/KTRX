// Catalogue page: search and category filter.
"use strict";

document.addEventListener("DOMContentLoaded", async () => {
  const { el, toolCard, catalog, CATEGORIES } = KTRX;
  const grid = document.querySelector("#grid");
  const search = document.querySelector("#search");
  const filters = document.querySelector("#filters");
  const count = document.querySelector("#count");
  const empty = document.querySelector("#empty");
  const params = new URLSearchParams(location.search);
  let category = params.get("categorie") ?? "all";
  search.value = params.get("q") ?? "";

  let tools = [];
  try {
    tools = (await catalog()).tools;
  } catch {
    grid.replaceChildren();
    empty.hidden = false;
    empty.textContent = "Le catalogue est momentanément indisponible.";
    return;
  }

  const present = new Set(tools.map((t) => t.category));
  const choices = [["all", "Tous"], ...Object.entries(CATEGORIES).filter(([k]) => present.has(k))];
  for (const [key, label] of choices) {
    const b = el("button", { type: "button", text: label });
    b.dataset.key = key;
    b.addEventListener("click", () => {
      category = key;
      render();
    });
    filters.append(b);
  }

  function render() {
    const q = search.value.trim().toLocaleLowerCase("fr");
    const shown = tools.filter(
      (t) =>
        (category === "all" || t.category === category) &&
        `${t.myth_name} ${t.name} ${t.description} ${t.tags.join(" ")} ${CATEGORIES[t.category] ?? ""}`.toLocaleLowerCase("fr").includes(q),
    );
    filters.querySelectorAll("button").forEach((b) => b.classList.toggle("is-active", b.dataset.key === category));
    grid.replaceChildren(...shown.map(toolCard));
    count.textContent = `${shown.length} outil${shown.length > 1 ? "s" : ""}`;
    empty.hidden = shown.length > 0;
    const url = new URL(location.href);
    url.searchParams.delete("q");
    url.searchParams.delete("categorie");
    if (q) url.searchParams.set("q", search.value.trim());
    if (category !== "all") url.searchParams.set("categorie", category);
    history.replaceState(null, "", url);
  }

  search.addEventListener("input", render);
  addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== search) {
      e.preventDefault();
      search.focus();
    }
  });
  render();
});
