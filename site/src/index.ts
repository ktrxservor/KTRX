// KTRX registry: public site and catalogue (read-only), plus the team API under /api/admin/.
import type { Env } from "./types";
import { error } from "./lib/http";
import { handleAdmin } from "./admin";
import { llmsTxt } from "./llms";
import { listTools, getTool, downloadTool, getIcon, engineInfo, downloadEngine, downloadDev, findTool, toolToJson } from "./public";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;
    try {
      if (path.startsWith("/api/admin/")) return await handleAdmin(env, request, path);

      if (request.method !== "GET" && request.method !== "HEAD") {
        return error("méthode non autorisée (publication : voir ktrx-dev)", 405);
      }

      if (path === "/api/tools") return await listTools(env);
      let m = /^\/api\/tools\/([A-Za-z0-9~-]+)$/.exec(path);
      if (m) return await getTool(env, m[1]);
      if (path === "/api/engine") return await engineInfo(env);
      m = /^\/dl\/dev\/([a-z0-9-]+)$/.exec(path);
      if (m) return await downloadDev(env, request, m[1]);
      m = /^\/dl\/engine\/([a-z0-9-]+)$/.exec(path);
      if (m) return await downloadEngine(env, request, m[1]);
      m = /^\/dl\/([A-Za-z0-9~-]+)$/.exec(path);
      if (m) return await downloadTool(env, request, m[1]);
      m = /^\/icons\/([a-z0-9-]+)\.png$/.exec(path);
      if (m) return await getIcon(env, m[1]);

      // Installers: UTF-8 text, so `curl … | sh` and `irm … | iex` (PowerShell 5.1) read them right.
      if (path === "/install.sh" || path === "/install.ps1" || path === "/install-dev.sh" || path === "/install-dev.ps1") {
        const asset = await env.ASSETS.fetch(request);
        if (!asset.ok) return asset;
        return new Response(asset.body, {
          headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "public, max-age=300", "x-content-type-options": "nosniff" },
        });
      }

      // Description of the site for language models (llmstxt.org), built from the live catalogue.
      if (path === "/llms.txt") return await llmsTxt(env, url.origin);

      // Developer guides (public, no secret inside): UTF-8 so they read right in a browser and for an AI fetching them.
      if (path === "/dev/guide-ia.md") {
        const asset = await env.ASSETS.fetch(request);
        if (!asset.ok) return asset;
        return new Response(asset.body, {
          headers: { "content-type": "text/markdown; charset=utf-8", "cache-control": "public, max-age=300", "x-content-type-options": "nosniff" },
        });
      }

      // Tool pages: /outils/<name>, with the tool's title and description in the HTML head.
      m = /^\/outils\/([A-Za-z0-9~-]+)\/?$/.exec(path);
      if (m) return await toolPage(env, request, m[1]);
      // Old links.
      if (path === "/tool.html" || path === "/tool") {
        const tool = await findTool(env, url.searchParams.get("id") ?? "");
        return Response.redirect(new URL(tool ? toolToJson(tool).page_url : "/outils", url).toString(), 301);
      }

      return await env.ASSETS.fetch(request);
    } catch (err) {
      console.error(err);
      return error("erreur interne", 500);
    }
  },
};

async function toolPage(env: Env, request: Request, key: string): Promise<Response> {
  const tool = await findTool(env, key);
  const page = await env.ASSETS.fetch(new Request(new URL("/outil.html", request.url)));
  if (!tool) return new Response(page.body, { status: 404, headers: page.headers });
  const info = toolToJson(tool);
  if (key !== info.name) return Response.redirect(new URL(info.page_url, request.url).toString(), 301);
  const title = `${tool.myth_name} — KTRX`;
  return new HTMLRewriter()
    .on("title", { element: (e) => void e.setInnerContent(title) })
    .on('meta[name="description"]', { element: (e) => void e.setAttribute("content", tool.description) })
    .on('meta[property="og:title"]', { element: (e) => void e.setAttribute("content", title) })
    .on('meta[property="og:description"]', { element: (e) => void e.setAttribute("content", tool.description) })
    .transform(page);
}
