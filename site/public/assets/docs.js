// Docs page: copy buttons on code blocks, active section in the side nav.
"use strict";
document.addEventListener("DOMContentLoaded", () => {
  document.querySelectorAll(".prose .code pre").forEach((pre) => {
    const text = pre.textContent.replace(/\n$/, "");
    const button = KTRX.el("button", { class: "copy", type: "button", text: "Copier" });
    button.addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(text); button.textContent = "Copié"; button.classList.add("is-done"); }
      catch { button.textContent = "Échec"; }
      setTimeout(() => { button.textContent = "Copier"; button.classList.remove("is-done"); }, 1600);
    });
    pre.parentElement.append(button);
  });
  const links = [...document.querySelectorAll(".docs-nav a")];
  const sections = links.map((a) => document.querySelector(a.getAttribute("href"))).filter(Boolean);
  if (!("IntersectionObserver" in window)) return;
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        const id = e.target.id;
        links.forEach((a) => a.classList.toggle("is-active", a.getAttribute("href") === `#${id}`));
      }
    }
  }, { rootMargin: "-20% 0px -70% 0px" });
  sections.forEach((s) => io.observe(s));
});
