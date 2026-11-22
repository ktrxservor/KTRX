import type { Env } from "../types";

/**
 * Team access: one shared password, kept as the `ADMIN_PASSWORD` Worker secret
 * (`npx wrangler secret put ADMIN_PASSWORD`). Whoever knows it can use the whole
 * admin API: publish and edit tools, delete them, publish the engine.
 */

export async function sha256Text(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Constant-time comparison (both sides hashed first, so lengths do not leak). */
async function samePassword(given: string, expected: string): Promise<boolean> {
  const [a, b] = await Promise.all([sha256Text(given), sha256Text(expected)]);
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/** Checks `Authorization: Bearer <password>` against the `ADMIN_PASSWORD` secret. */
export async function authenticate(request: Request, env: Env): Promise<boolean> {
  if (!env.ADMIN_PASSWORD) return false;
  const match = /^Bearer\s+(.+)$/.exec(request.headers.get("Authorization") ?? "");
  return !!match && (await samePassword(match[1].trim(), env.ADMIN_PASSWORD));
}

export async function audit(
  env: Env,
  entry: { action: string; toolId?: string | null; version?: string | null; request: Request; detail?: string | null },
): Promise<void> {
  await env.DB.prepare("INSERT INTO audit_log (action, tool_id, version, ip, detail) VALUES (?, ?, ?, ?, ?)")
    .bind(
      entry.action,
      entry.toolId ?? null,
      entry.version ?? null,
      entry.request.headers.get("cf-connecting-ip"),
      entry.detail ?? null,
    )
    .run();
}
