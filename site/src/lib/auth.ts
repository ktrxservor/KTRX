import type { Env } from "../types";

/**
 * Team API keys.
 *   publish  publish tool versions, update their README, emblem and metadata, withdraw a version
 *   admin    everything, including deleting a tool, publishing the engine and managing keys
 * Keys created before 0.6 carry "upload" / "delete", read as publish / admin.
 */
export type Scope = "publish" | "admin";

export interface AuthContext {
  keyId: string;
  label: string;
  scopes: Scope[];
}

export async function sha256Text(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function normalizeScopes(raw: unknown): Scope[] {
  const list = Array.isArray(raw) ? raw.map(String) : [];
  const scopes = new Set<Scope>();
  for (const s of list) {
    if (s === "admin" || s === "delete") scopes.add("admin");
    if (s === "publish" || s === "upload") scopes.add("publish");
  }
  return [...scopes];
}

/** Checks `Authorization: Bearer <key>` against the hashed keys in D1. */
export async function authenticate(request: Request, env: Env): Promise<AuthContext | null> {
  const match = /^Bearer\s+(\S+)$/.exec(request.headers.get("Authorization") ?? "");
  if (!match || match[1].length < 16) return null;
  const row = await env.DB.prepare(
    "SELECT id, label, scopes FROM api_keys WHERE key_hash = ? AND revoked_at IS NULL",
  )
    .bind(await sha256Text(match[1]))
    .first<{ id: string; label: string; scopes: string }>();
  if (!row) return null;
  await env.DB.prepare("UPDATE api_keys SET last_used_at = datetime('now') WHERE id = ?").bind(row.id).run();
  let scopes: Scope[] = [];
  try {
    scopes = normalizeScopes(JSON.parse(row.scopes));
  } catch {
    scopes = [];
  }
  return { keyId: row.id, label: row.label, scopes };
}

export function allows(ctx: AuthContext, scope: Scope): boolean {
  return ctx.scopes.includes("admin") || ctx.scopes.includes(scope);
}

export async function audit(
  env: Env,
  entry: { actor: AuthContext; action: string; toolId?: string | null; version?: string | null; request: Request; detail?: string | null },
): Promise<void> {
  await env.DB.prepare(
    "INSERT INTO audit_log (actor_key_id, action, tool_id, version, ip, detail) VALUES (?, ?, ?, ?, ?, ?)",
  )
    .bind(
      entry.actor.keyId,
      entry.action,
      entry.toolId ?? null,
      entry.version ?? null,
      entry.request.headers.get("cf-connecting-ip"),
      entry.detail ?? null,
    )
    .run();
}
