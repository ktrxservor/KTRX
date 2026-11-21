// Download page: manual archives with their checksums.
"use strict";

document.addEventListener("DOMContentLoaded", async () => {
  const { el, codeBlock } = KTRX;
  const LABELS = {
    windows: ["Windows", "x64 · .zip"],
    macos: ["macOS", "Apple Silicon et Intel · .tar.gz"],
    linux: ["Linux", "x86_64 · statique · .tar.gz"],
    "linux-arm64": ["Linux ARM64", "statique · .tar.gz"],
    "macos-intel": ["macOS Intel", ".tar.gz"],
  };
  const box = document.querySelector("#downloads");
  const meta = document.querySelector("#engine-meta");
  try {
    const engine = await (await fetch("/api/engine")).json();
    if (!engine.version) throw new Error("no release");
    const date = engine.published_at ? new Date(engine.published_at.replace(" ", "T") + "Z").toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" }) : "";
    meta.textContent = `Version ${engine.version}${date ? ` · publiée le ${date}` : ""}`;
    const cards = Object.entries(LABELS)
      .filter(([p]) => engine.platforms[p])
      .map(([p, [name, detail]]) => {
        const info = engine.platforms[p];
        return el(
          "a",
          { class: "download", href: info.url },
          el("strong", { text: name }),
          el("span", { text: `${detail} · ${(info.size_bytes / 1048576).toFixed(1)} Mo` }),
          el("span", { class: "hash", text: info.sha256 }),
          el("span", { class: "get", text: "Télécharger" }),
        );
      });
    box.replaceChildren(...cards);
  } catch {
    meta.textContent = "Informations de version momentanément indisponibles.";
  }
  document.querySelector("#verify").append(
    codeBlock("shasum -a 256 ktrx-macos.tar.gz        # macOS\nsha256sum ktrx-linux.tar.gz           # Linux"),
    codeBlock("Get-FileHash ktrx-windows.zip          # Windows (PowerShell)"),
  );
});
