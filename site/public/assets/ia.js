// "Pour les IA" page: the registration command for each assistant.
"use strict";
document.addEventListener("DOMContentLoaded", () => {
  const COMMANDS = {
    claude: { cmd: "claude mcp add --scope user ktrx -- ktrx mcp", prompt: "$" },
    codex: { cmd: "codex mcp add ktrx -- ktrx mcp", prompt: "$" },
    gemini: { cmd: "gemini mcp add ktrx ktrx mcp", prompt: "$" },
    json: {
      cmd: '{\n  "mcpServers": {\n    "ktrx": { "command": "ktrx", "args": ["mcp"] }\n  }\n}',
      prompt: "",
      note: "À ajouter dans le fichier de configuration MCP de l'assistant (Cursor : <code>~/.cursor/mcp.json</code>, Claude Desktop, Antigravity, Windsurf, VS Code…). Le moteur KTRX doit être installé (<a href=\"/telecharger\">une ligne</a>), puis redémarrez l'assistant.",
    },
  };
  const code = document.getElementById("cmd");
  const copy = document.getElementById("cmd-copy");
  const prompt = document.getElementById("cmd-prompt");
  const note = document.getElementById("cmd-note");
  const defaultNote = note.innerHTML;
  document.querySelectorAll("#clients button").forEach((button) => {
    button.addEventListener("click", () => {
      const entry = COMMANDS[button.dataset.client];
      document.querySelectorAll("#clients button").forEach((b) => b.classList.toggle("is-active", b === button));
      code.textContent = entry.cmd;
      copy.dataset.copy = entry.cmd;
      prompt.textContent = entry.prompt;
      note.innerHTML = entry.note || defaultNote;
    });
  });
});
