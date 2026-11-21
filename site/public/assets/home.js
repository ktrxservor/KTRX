// Home page: terminal demo, catalogue rail, hero parallax.
"use strict";

document.addEventListener("DOMContentLoaded", () => {
  const { el, toolCard, catalog, INSTALL, reducedMotion } = KTRX;

  // ---- terminal demo (real engine output, shortened)
  const term = document.querySelector("#terminal");
  const script = [
    ["cmd", "ktrx install thoth"],
    ["out", "Downloading K~THOTH 1.0.0…"],
    ["out", "Installed K~THOTH 1.0.0 → ~/.ktrx/tools/K~THOTH.tool"],
    ["ok", "sha256 88b4cabd…5f2e0ba (verified)"],
    ["cmd", 'ktrx run thoth --encode "Bonjour"'],
    ["out", '{\n  "encoded": "Qm9uam91cg=="\n}'],
    ["cmd", "ktrx run hermes https://example.com"],
    ["ask", "K~HERMES requests network access for this run. Allow? [y/N] y"],
    ["out", '{\n  "alpn": "h2",\n  "content_encoding": "gzip",\n  "elapsed_ms": 254,\n  …'],
  ];
  const cls = { out: "t-dim", ok: "t-ok", ask: "t-ask" };
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  async function play() {
    term.textContent = "";
    const caret = el("span", { class: "caret" });
    for (const [kind, text] of script) {
      if (kind === "cmd") {
        const line = el("span", { class: "t-cmd" });
        term.append(el("span", { class: "t-dim", text: "$ " }), line, caret);
        for (const ch of text) {
          line.textContent += ch;
          await sleep(reducedMotion ? 0 : 28 + Math.random() * 40);
        }
        await sleep(reducedMotion ? 0 : 350);
        caret.remove();
        term.append("\n");
      } else {
        term.append(el("span", { class: cls[kind], text }), "\n");
        await sleep(reducedMotion ? 0 : 260);
      }
    }
    term.append(el("span", { class: "t-dim", text: "$ " }), caret);
  }

  if (term) {
    if (reducedMotion || !("IntersectionObserver" in window)) play();
    else {
      const io = new IntersectionObserver((entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          setTimeout(play, 500);
        }
      }, { threshold: 0.3 });
      io.observe(term);
    }
  }

  // ---- hero parallax: the terminal settles as the page scrolls
  const visual = document.querySelector("#hero-visual");
  if (visual && !reducedMotion) {
    let ticking = false;
    const update = () => {
      const rect = visual.getBoundingClientRect();
      const progress = Math.min(Math.max(1 - rect.top / innerHeight, 0), 1);
      if (visual.classList.contains("is-visible")) {
        visual.style.transition = "opacity 1.1s";
        visual.style.transform = `perspective(1200px) rotateX(${(1 - progress) * 10}deg) scale(${0.94 + progress * 0.06})`;
      }
      ticking = false;
    };
    addEventListener("scroll", () => {
      if (!ticking) {
        ticking = true;
        requestAnimationFrame(update);
      }
    }, { passive: true });
    setTimeout(update, 1200);
  }

  // ---- install command in the steps
  const step = document.querySelector("#step-install");
  if (step) step.textContent = INSTALL[/windows/i.test(navigator.userAgent) ? "windows" : "unix"].command;

  // ---- catalogue rail and tool count
  catalog()
    .then(({ tools }) => {
      document.querySelector("#stat-tools").textContent = String(tools.length);
      const picks = [...tools].sort(() => Math.random() - 0.5).slice(0, 8);
      document.querySelector("#rail").replaceChildren(...picks.map(toolCard));
    })
    .catch(() => document.querySelector("#rail").replaceChildren());
});
