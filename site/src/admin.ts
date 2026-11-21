// Team API, under /api/admin/. Every route needs `Authorization: Bearer <key>`.
// Used by ktrx-dev; never linked from the public site.
import type { Env } from "./types";
import { authenticate, allows, audit, sha256Text, type AuthContext, type Scope } from "./lib/auth";
import { json, error, compareVersions, CATEGORIES, PLATFORMS } from "./lib/http";
import { parseToolArchive, usageDoc, sha256Hex, ManifestError } from "./lib/manifest";
import { toolToJson, type ToolRow } from "./public";

const MAX_ICON = 10 * 1024 * 1024;
const MAX_README = 512 * 1024;
const MAX_ENGINE = 90 * 1024 * 1024;

type Handler = (env: Env, request: Request, auth: AuthContext, params: string[]) => Promise<Response>;
interface Route {
  method: string;
  pattern: RegExp;
  scope: Scope;
  handler: Handler;
}

const routes: Route[] = [
  { method: "GET", pattern: /^\/api\/admin\/whoami$/, scope: "publish", handler: whoami },
  { method: "POST", pattern: /^\/api\/admin\/tools$/, scope: "publish", handler: publishVersion },
  { method: "PUT", pattern: /^\/api\/admin\/tools\/([a-z0-9-]+)\/icon$/, scope: "publish", handler: putIcon },
  { method: "PUT", pattern: /^\/api\/admin\/tools\/([a-z0-9-]+)\/readme$/, scope: "publish", handler: putReadme },
  { method: "PATCH", pattern: /^\/api\/admin\/tools\/([a-z0-9-]+)$/, scope: "publish", handler: patchTool },
  { method: "POST", pattern: /^\/api\/admin\/tools\/([a-z0-9-]+)\/versions\/([0-9A-Za-z.+-]+)\/yank$/, scope: "publish", handler: yankVersion },
  { method: "DELETE", pattern: /^\/api\/admin\/tools\/([a-z0-9-]+)$/, scope: "admin", handler: deleteTool },
  { method: "POST", pattern: /^\/api\/admin\/engine$/, scope: "admin", handler: publishEngine },
  { method: "GET", pattern: /^\/api\/admin\/keys$/, scope: "admin", handler: listKeys },
  { method: "POST", pattern: /^\/api\/admin\/keys$/, scope: "admin", handler: createKey },
  { method: "DELETE", pattern: /^\/api\/admin\/keys\/([a-f0-9]+)$/, scope: "admin", handler: revokeKey },
  { method: "GET", pattern: /^\/api\/admin\/audit$/, scope: "admin", handler: listAudit },
];

export async function handleAdmin(env: Env, request: Request, path: string): Promise<Response> {
  const auth = await authenticate(request, env);
  if (!auth) return error("clé API manquante ou invalide", 401);
  for (const route of routes) {
    const match = route.pattern.exec(path);
    if (!match || route.method !== request.method) continue;
    if (!allows(auth, route.scope)) return error(`cette action demande une clé « ${route.scope} »`, 403);
    return route.handler(env, request, auth, match.slice(1));
  }
  return error("route inconnue", 404);
}

async function whoami(_env: Env, _request: Request, auth: AuthContext): Promise<Response> {
  return json({ id: auth.keyId, label: auth.label, scopes: auth.scopes });
}

async function toolExists(env: Env, id: string): Promise<ToolRow | null> {
  return env.DB.prepare("SELECT * FROM tools WHERE id = ?").bind(id).first<ToolRow>();
}

async function publishVersion(env: Env, request: Request, auth: AuthContext): Promise<Response> {
  const changelog = new URL(request.url).searchParams.get("changelog");
  const archive = new Uint8Array(await request.arrayBuffer());
  let manifest;
  try {
    manifest = parseToolArchive(archive).manifest;
  } catch (err) {
    if (err instanceof ManifestError) return error(err.message, 400);
    throw err;
  }
  const category = manifest.category ?? "analysis";
  if (!CATEGORIES.has(category)) return error(`catégorie inconnue : ${category}`, 400);

  const existing = await toolExists(env, manifest.id);
  if (existing && existing.myth_name !== manifest.name) {
    return error(`l'id ${manifest.id} appartient déjà à ${existing.myth_name}`, 409);
  }
  const sameName = await env.DB.prepare("SELECT id FROM tools WHERE myth_name = ? AND id != ?")
    .bind(manifest.name, manifest.id)
    .first();
  if (sameName) return error(`le nom ${manifest.name} est déjà utilisé par un autre outil`, 409);
  const duplicate = await env.DB.prepare("SELECT 1 FROM tool_versions WHERE tool_id = ? AND version = ?")
    .bind(manifest.id, manifest.version)
    .first();
  if (duplicate) return error(`la version ${manifest.version} de ${manifest.name} est déjà publiée : incrémentez « version »`, 409);

  const sha256 = await sha256Hex(archive);
  const key = `tools/${manifest.id}/${manifest.version}/${manifest.name}.tool`;
  await env.TOOLS_BUCKET.put(key, archive, { httpMetadata: { contentType: "application/zip" } });
  const now = new Date().toISOString();
  if (existing) {
    await env.DB.prepare(
      "UPDATE tools SET description = ?, category_id = ?, author = ?, tags = ?, permissions = ?, latest_version = ?, deprecated = 0, updated_at = ? WHERE id = ?",
    )
      .bind(manifest.description, category, manifest.author, JSON.stringify(manifest.tags), JSON.stringify(manifest.permissions), manifest.version, now, manifest.id)
      .run();
  } else {
    await env.DB.prepare(
      `INSERT INTO tools (id, myth_name, display_name, description, category_id, author, tags, permissions, latest_version, icon_key, readme_md, deprecated, created_at, updated_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, 0, ?, ?)`,
    )
      .bind(manifest.id, manifest.name, manifest.name.replace(/^K~/, ""), manifest.description, category, manifest.author,
        JSON.stringify(manifest.tags), JSON.stringify(manifest.permissions), manifest.version, now, now)
      .run();
  }
  await env.DB.prepare(
    `INSERT INTO tool_versions (tool_id, version, r2_key, sha256, size_bytes, permissions, changelog, min_engine_version, yanked, published_at, manifest_json)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)`,
  )
    .bind(manifest.id, manifest.version, key, sha256, archive.byteLength, JSON.stringify(manifest.permissions), changelog,
      manifest.min_engine_version ?? null, now, JSON.stringify(usageDoc(manifest)))
    .run();
  await audit(env, { actor: auth, action: existing ? "publish_version" : "create_tool", toolId: manifest.id, version: manifest.version, request });
  const name = manifest.name.replace(/^K~/, "").toLowerCase();
  return json({ ok: true, id: manifest.id, name, version: manifest.version, sha256, page_url: `/outils/${name}`, created: !existing }, existing ? 200 : 201);
}

function isPng(bytes: Uint8Array): boolean {
  const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  return bytes.length > 24 && sig.every((b, i) => bytes[i] === b);
}

async function putIcon(env: Env, request: Request, auth: AuthContext, [id]: string[]): Promise<Response> {
  if (!(await toolExists(env, id))) return error("outil introuvable", 404);
  const bytes = new Uint8Array(await request.arrayBuffer());
  if (bytes.byteLength > MAX_ICON) return error("emblème supérieur à 10 Mio", 413);
  if (!isPng(bytes)) return error("l'emblème doit être un PNG", 400);
  const key = `icons/${id}.png`;
  await env.TOOLS_BUCKET.put(key, bytes, { httpMetadata: { contentType: "image/png" } });
  await env.DB.prepare("UPDATE tools SET icon_key = ?, updated_at = ? WHERE id = ?").bind(key, new Date().toISOString(), id).run();
  await audit(env, { actor: auth, action: "update_icon", toolId: id, request });
  return json({ ok: true });
}

async function putReadme(env: Env, request: Request, auth: AuthContext, [id]: string[]): Promise<Response> {
  if (!(await toolExists(env, id))) return error("outil introuvable", 404);
  const text = await request.text();
  if (text.length > MAX_README) return error("README supérieur à 512 Kio", 413);
  await env.DB.prepare("UPDATE tools SET readme_md = ?, updated_at = ? WHERE id = ?").bind(text, new Date().toISOString(), id).run();
  await audit(env, { actor: auth, action: "update_readme", toolId: id, request });
  return json({ ok: true });
}

async function patchTool(env: Env, request: Request, auth: AuthContext, [id]: string[]): Promise<Response> {
  if (!(await toolExists(env, id))) return error("outil introuvable", 404);
  const body = await request.json<Record<string, unknown>>().catch(() => null);
  if (!body) return error("JSON invalide", 400);
  const sets: string[] = [];
  const values: unknown[] = [];
  if (typeof body.category === "string") {
    if (!CATEGORIES.has(body.category)) return error(`catégorie inconnue : ${body.category}`, 400);
    sets.push("category_id = ?");
    values.push(body.category);
  }
  if (typeof body.deprecated === "boolean") {
    sets.push("deprecated = ?");
    values.push(body.deprecated ? 1 : 0);
  }
  if (sets.length === 0) return error("champs modifiables : category, deprecated", 400);
  await env.DB.prepare(`UPDATE tools SET ${sets.join(", ")}, updated_at = ? WHERE id = ?`).bind(...values, new Date().toISOString(), id).run();
  await audit(env, { actor: auth, action: "patch_tool", toolId: id, request, detail: JSON.stringify(body) });
  return json({ ok: true });
}

async function yankVersion(env: Env, request: Request, auth: AuthContext, [id, version]: string[]): Promise<Response> {
  const tool = await toolExists(env, id);
  if (!tool) return error("outil introuvable", 404);
  const row = await env.DB.prepare("SELECT yanked FROM tool_versions WHERE tool_id = ? AND version = ?").bind(id, version).first();
  if (!row) return error("version introuvable", 404);
  await env.DB.prepare("UPDATE tool_versions SET yanked = 1 WHERE tool_id = ? AND version = ?").bind(id, version).run();
  // The latest installable version moves back to the newest one still available.
  const { results } = await env.DB.prepare("SELECT version FROM tool_versions WHERE tool_id = ? AND yanked = 0").bind(id).all<{ version: string }>();
  const latest = results.map((r) => r.version).sort((a, b) => compareVersions(b, a))[0];
  if (latest) {
    await env.DB.prepare("UPDATE tools SET latest_version = ?, updated_at = ? WHERE id = ?").bind(latest, new Date().toISOString(), id).run();
  } else {
    await env.DB.prepare("UPDATE tools SET deprecated = 1, updated_at = ? WHERE id = ?").bind(new Date().toISOString(), id).run();
  }
  await audit(env, { actor: auth, action: "yank_version", toolId: id, version, request });
  return json({ ok: true, latest_version: latest ?? null, hidden: !latest });
}

async function deleteTool(env: Env, request: Request, auth: AuthContext, [id]: string[]): Promise<Response> {
  const tool = await toolExists(env, id);
  if (!tool) return error("outil introuvable", 404);
  const { results } = await env.DB.prepare("SELECT r2_key FROM tool_versions WHERE tool_id = ?").bind(id).all<{ r2_key: string }>();
  for (const r of results) await env.TOOLS_BUCKET.delete(r.r2_key);
  if (tool.icon_key) await env.TOOLS_BUCKET.delete(tool.icon_key);
  await env.DB.prepare("DELETE FROM tool_versions WHERE tool_id = ?").bind(id).run();
  await env.DB.prepare("DELETE FROM tools WHERE id = ?").bind(id).run();
  await audit(env, { actor: auth, action: "delete_tool", toolId: id, request, detail: tool.myth_name });
  return json({ ok: true, deleted: toolToJson(tool).name });
}

async function publishEngine(env: Env, request: Request, auth: AuthContext): Promise<Response> {
  const url = new URL(request.url);
  const version = url.searchParams.get("version") ?? "";
  const platform = url.searchParams.get("platform") ?? "";
  const filename = url.searchParams.get("filename") ?? "";
  if (!/^\d+\.\d+\.\d+$/.test(version)) return error("version invalide (MAJOR.MINOR.PATCH)", 400);
  if (!PLATFORMS.has(platform)) return error("plateforme inconnue (windows, linux, linux-arm64, macos)", 400);
  if (!/^[A-Za-z0-9._-]{1,80}$/.test(filename)) return error("nom de fichier invalide", 400);
  const bytes = new Uint8Array(await request.arrayBuffer());
  if (bytes.byteLength === 0 || bytes.byteLength > MAX_ENGINE) return error("archive vide ou supérieure à 90 Mio", 400);
  if (await env.DB.prepare("SELECT 1 FROM engine_releases WHERE version = ? AND platform = ?").bind(version, platform).first()) {
    return error(`le moteur ${version} pour ${platform} est déjà publié`, 409);
  }
  const sha256 = await sha256Hex(bytes);
  const key = `engine/${version}/${filename}`;
  await env.TOOLS_BUCKET.put(key, bytes, { httpMetadata: { contentType: "application/octet-stream" } });
  await env.DB.prepare("INSERT INTO engine_releases (version, platform, filename, r2_key, sha256, size_bytes) VALUES (?, ?, ?, ?, ?, ?)")
    .bind(version, platform, filename, key, sha256, bytes.byteLength)
    .run();
  await audit(env, { actor: auth, action: "publish_engine", toolId: `engine-${platform}`, version, request });
  return json({ ok: true, version, platform, sha256 }, 201);
}

async function listKeys(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare(
    "SELECT id, label, scopes, created_at, last_used_at, revoked_at FROM api_keys ORDER BY created_at",
  ).all<Record<string, string>>();
  return json({ keys: results.map((k) => ({ ...k, scopes: JSON.parse(k.scopes || "[]") })) });
}

function randomHex(bytes: number): string {
  return [...crypto.getRandomValues(new Uint8Array(bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function randomSecret(): string {
  const raw = crypto.getRandomValues(new Uint8Array(32));
  return "ktrx_" + btoa(String.fromCharCode(...raw)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function createKey(env: Env, request: Request, auth: AuthContext): Promise<Response> {
  const body = await request.json<{ label?: unknown; scope?: unknown }>().catch(() => null);
  const label = typeof body?.label === "string" ? body.label.trim() : "";
  const scope = body?.scope === "admin" ? "admin" : "publish";
  if (!label || label.length > 60) return error("label requis (60 caractères max)", 400);
  const id = randomHex(8);
  const secret = randomSecret();
  await env.DB.prepare("INSERT INTO api_keys (id, key_hash, label, scopes) VALUES (?, ?, ?, ?)")
    .bind(id, await sha256Text(secret), label, JSON.stringify([scope]))
    .run();
  await audit(env, { actor: auth, action: "create_key", request, detail: `${id} ${label} ${scope}` });
  return json({ id, label, scope, key: secret }, 201);
}

async function revokeKey(env: Env, request: Request, auth: AuthContext, [id]: string[]): Promise<Response> {
  if (id === auth.keyId) return error("impossible de révoquer la clé utilisée pour cette requête", 400);
  const result = await env.DB.prepare("UPDATE api_keys SET revoked_at = datetime('now') WHERE id = ? AND revoked_at IS NULL").bind(id).run();
  if (!result.meta.changes) return error("clé introuvable ou déjà révoquée", 404);
  await audit(env, { actor: auth, action: "revoke_key", request, detail: id });
  return json({ ok: true });
}

async function listAudit(env: Env, request: Request): Promise<Response> {
  const limit = Math.min(Math.max(parseInt(new URL(request.url).searchParams.get("limit") ?? "50", 10) || 50, 1), 500);
  const { results } = await env.DB.prepare(
    `SELECT a.at, a.action, a.tool_id, a.version, a.detail, k.label AS actor
     FROM audit_log a LEFT JOIN api_keys k ON k.id = a.actor_key_id ORDER BY a.id DESC LIMIT ?`,
  )
    .bind(limit)
    .all();
  return json({ entries: results });
}
