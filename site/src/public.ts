// Public, read-only part of the registry: catalogue, downloads, engine releases.
import type { Env } from "./types";
import { json, error, compareVersions, safeJsonArray, PLATFORMS } from "./lib/http";
import { parseManifestOnly, usageDoc } from "./lib/manifest";

export interface ToolRow {
  id: string;
  myth_name: string;
  display_name: string;
  description: string;
  category_id: string;
  author: string;
  tags: string;
  permissions: string;
  latest_version: string;
  icon_key: string | null;
  readme_md: string | null;
  deprecated: number;
  created_at: string;
  updated_at: string;
}

export function toolToJson(row: ToolRow) {
  const name = row.myth_name.replace(/^K~/, "").toLowerCase();
  return {
    id: row.id,
    name,
    myth_name: row.myth_name,
    description: row.description,
    category: row.category_id,
    author: row.author,
    tags: safeJsonArray(row.tags),
    permissions: safeJsonArray(row.permissions),
    latest_version: row.latest_version,
    icon_url: row.icon_key ? `/icons/${row.id}.png?v=${encodeURIComponent(row.updated_at)}` : null,
    page_url: `/outils/${name}`,
    download_url: `/dl/${row.id}`,
    install: `ktrx install ${name}`,
    deprecated: !!row.deprecated,
    created_at: row.created_at,
    updated_at: row.updated_at,
  };
}

/** Finds a tool by id (`hash-audit`), short name (`anubis`) or myth name (`K~ANUBIS`). */
export async function findTool(env: Env, key: string): Promise<ToolRow | null> {
  const myth = `K~${key.replace(/^k~/i, "").toUpperCase()}`;
  return env.DB.prepare("SELECT * FROM tools WHERE id = ? OR myth_name = ?").bind(key.toLowerCase(), myth).first<ToolRow>();
}

export async function listTools(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare("SELECT * FROM tools WHERE deprecated = 0 ORDER BY myth_name ASC").all<ToolRow>();
  const { results: categories } = await env.DB.prepare(
    "SELECT id, label FROM categories ORDER BY sort_order ASC",
  ).all();
  return json({ tools: results.map(toolToJson), categories }, 200, { "cache-control": "public, max-age=60" });
}

export async function getTool(env: Env, key: string): Promise<Response> {
  const tool = await findTool(env, key);
  if (!tool) return error("outil introuvable", 404);
  const { results: versions } = await env.DB.prepare(
    "SELECT version, sha256, size_bytes, changelog, min_engine_version, yanked, published_at FROM tool_versions WHERE tool_id = ? ORDER BY published_at DESC",
  )
    .bind(tool.id)
    .all();
  const usage = await loadUsage(env, tool.id, tool.latest_version);
  return json({ ...toolToJson(tool), readme: tool.readme_md, versions, usage }, 200, { "cache-control": "public, max-age=60" });
}

/** Arguments, examples… of a version, read from its archive the first time and kept in D1. */
async function loadUsage(env: Env, id: string, version: string): Promise<unknown> {
  const row = await env.DB.prepare("SELECT r2_key, manifest_json FROM tool_versions WHERE tool_id = ? AND version = ?")
    .bind(id, version)
    .first<{ r2_key: string; manifest_json: string | null }>();
  if (!row) return null;
  if (row.manifest_json) {
    try {
      return JSON.parse(row.manifest_json);
    } catch {
      /* rebuilt below */
    }
  }
  const object = await env.TOOLS_BUCKET.get(row.r2_key);
  if (!object) return null;
  try {
    const usage = usageDoc(parseManifestOnly(new Uint8Array(await object.arrayBuffer())));
    await env.DB.prepare("UPDATE tool_versions SET manifest_json = ? WHERE tool_id = ? AND version = ?")
      .bind(JSON.stringify(usage), id, version)
      .run();
    return usage;
  } catch {
    return null;
  }
}

export async function downloadTool(env: Env, request: Request, key: string): Promise<Response> {
  const tool = await findTool(env, key);
  if (!tool) return error("outil introuvable", 404);
  const version = new URL(request.url).searchParams.get("version") ?? tool.latest_version;
  const row = await env.DB.prepare("SELECT r2_key, sha256, yanked FROM tool_versions WHERE tool_id = ? AND version = ?")
    .bind(tool.id, version)
    .first<{ r2_key: string; sha256: string; yanked: number }>();
  if (!row) return error("version introuvable", 404);
  if (row.yanked) return error("cette version a été retirée", 410);
  const object = await env.TOOLS_BUCKET.get(row.r2_key);
  if (!object) return error("fichier manquant", 500);
  await logDownload(env, request, tool.id, version);
  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("content-disposition", `attachment; filename="${tool.myth_name}.tool"`);
  headers.set("x-ktrx-sha256", row.sha256);
  headers.set("cache-control", "public, max-age=31536000, immutable");
  return new Response(object.body, { headers });
}

export async function getIcon(env: Env, id: string): Promise<Response> {
  const tool = await env.DB.prepare("SELECT icon_key FROM tools WHERE id = ?").bind(id).first<{ icon_key: string | null }>();
  const object = tool?.icon_key ? await env.TOOLS_BUCKET.get(tool.icon_key) : null;
  if (!object) return new Response("not found", { status: 404 });
  return new Response(object.body, {
    headers: { "content-type": "image/png", "cache-control": "public, max-age=604800" },
  });
}

interface ReleaseRow {
  version: string;
  platform: string;
  filename: string;
  r2_key: string;
  sha256: string;
  size_bytes: number;
  published_at: string;
}

async function engineVersions(env: Env): Promise<string[]> {
  const { results } = await env.DB.prepare("SELECT DISTINCT version FROM engine_releases").all<{ version: string }>();
  return results.map((r) => r.version).sort((a, b) => compareVersions(b, a));
}

export async function engineInfo(env: Env): Promise<Response> {
  const versions = await engineVersions(env);
  if (versions.length === 0) return json({ version: null, platforms: {}, versions: [] });
  const { results } = await env.DB.prepare("SELECT * FROM engine_releases WHERE version = ?").bind(versions[0]).all<ReleaseRow>();
  const platforms: Record<string, unknown> = {};
  for (const r of results) {
    platforms[r.platform] = { filename: r.filename, sha256: r.sha256, size_bytes: r.size_bytes, url: `/dl/engine/${r.platform}` };
  }
  return json(
    { version: versions[0], published_at: results[0]?.published_at ?? null, platforms, versions },
    200,
    { "cache-control": "public, max-age=60" },
  );
}

export async function downloadEngine(env: Env, request: Request, platform: string): Promise<Response> {
  if (!PLATFORMS.has(platform)) return error("plateforme inconnue (windows, linux, linux-arm64, macos)", 404);
  // Without ?version only the latest release counts: a platform it no longer ships must not
  // silently fall back to an older release.
  const version = new URL(request.url).searchParams.get("version") ?? (await engineVersions(env))[0];
  const row = version
    ? await env.DB.prepare("SELECT * FROM engine_releases WHERE platform = ? AND version = ?").bind(platform, version).first<ReleaseRow>()
    : null;
  if (!row) return error("aucune version disponible pour cette plateforme", 404);
  const object = await env.TOOLS_BUCKET.get(row.r2_key);
  if (!object) return error("fichier manquant", 500);
  await logDownload(env, request, `engine-${platform}`, row.version);
  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("content-disposition", `attachment; filename="${row.filename}"`);
  headers.set("x-ktrx-sha256", row.sha256);
  headers.set("cache-control", "public, max-age=3600");
  return new Response(object.body, { headers });
}

async function logDownload(env: Env, request: Request, id: string, version: string): Promise<void> {
  await env.DB.prepare("INSERT INTO download_events (tool_id, version, country, user_agent) VALUES (?, ?, ?, ?)")
    .bind(id, version, request.headers.get("cf-ipcountry"), request.headers.get("user-agent"))
    .run();
}
