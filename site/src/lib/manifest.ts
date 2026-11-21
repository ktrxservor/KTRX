// Parses and validates a .tool archive (ZIP of manifest.toml + module.wasm),
// mirroring the limits enforced by engine/crates/ktrx-core in the Rust engine.
// Keep these limits in sync with ktrx-core and with DEVELOPING.md.
import { unzipSync } from "fflate";
import { parse as parseToml } from "smol-toml";

export const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;
export const MAX_MANIFEST_BYTES = 256 * 1024;
export const MAX_MODULE_BYTES = 48 * 1024 * 1024;

const VALID_PERMISSIONS = new Set([
  "network",
  "file-read",
  "file-write",
  "raw-net",
  "process",
]);

export interface ParsedManifest {
  id: string;
  name: string; // "K~MYTH"
  description: string;
  version: string;
  author: string;
  api_version: string;
  permissions: string[];
  programs: string[];
  min_engine_version?: string;
  examples: string[];
  // Optional registry metadata (ignored by the engine).
  category?: string;
  tags: string[];
  license?: string;
  arguments: ManifestArgument[];
}

export interface ManifestArgument {
  name: string;
  description: string;
  value_type: string;
  required: boolean;
  positional: boolean;
  default?: unknown;
  choices?: string[];
}

export class ManifestError extends Error {}

// Same rule as ktrx-core: 1-80 chars, starts with a letter, ends with a letter or digit.
function isValidId(id: string): boolean {
  return /^[a-z]([a-z0-9-]*[a-z0-9])?$/.test(id) && id.length <= 80;
}

function isValidMythName(name: string): boolean {
  return /^K~[A-Z]+$/.test(name);
}

export function parseToolArchive(bytes: Uint8Array): {
  manifest: ParsedManifest;
  moduleBytes: Uint8Array;
} {
  if (bytes.byteLength === 0) {
    throw new ManifestError("archive vide");
  }
  if (bytes.byteLength > MAX_ARCHIVE_BYTES) {
    throw new ManifestError("archive supérieure à la limite de 64 Mio");
  }

  let entries: Record<string, Uint8Array>;
  try {
    entries = unzipSync(bytes);
  } catch (err) {
    throw new ManifestError(`archive .tool invalide (ZIP illisible): ${String(err)}`);
  }

  const manifestBytes = entries["manifest.toml"];
  const moduleBytes = entries["module.wasm"];
  if (!manifestBytes) throw new ManifestError("manifest.toml manquant dans l'archive");
  if (!moduleBytes) throw new ManifestError("module.wasm manquant dans l'archive");
  if (manifestBytes.byteLength > MAX_MANIFEST_BYTES) {
    throw new ManifestError("manifest.toml dépasse la limite de 256 Kio");
  }
  if (moduleBytes.byteLength > MAX_MODULE_BYTES) {
    throw new ManifestError("module.wasm dépasse la limite de 48 Mio");
  }

  return { manifest: parseManifestBytes(manifestBytes), moduleBytes };
}

function parseManifestBytes(manifestBytes: Uint8Array): ParsedManifest {
  if (manifestBytes.byteLength > MAX_MANIFEST_BYTES) {
    throw new ManifestError("manifest.toml dépasse la limite de 256 Kio");
  }
  const manifestText = new TextDecoder("utf-8").decode(manifestBytes);
  let raw: any;
  try {
    raw = parseToml(manifestText);
  } catch (err) {
    throw new ManifestError(`manifest.toml invalide (TOML illisible): ${String(err)}`);
  }

  for (const field of ["id", "name", "description", "version", "author", "api_version"]) {
    if (typeof raw[field] !== "string" || raw[field].length === 0) {
      throw new ManifestError(`champ manifeste requis manquant ou invalide: ${field}`);
    }
  }
  if (!isValidId(raw.id)) {
    throw new ManifestError("id invalide: lettres minuscules, chiffres et tirets uniquement");
  }
  if (!isValidMythName(raw.name)) {
    throw new ManifestError("name invalide: doit être de la forme K~NOM");
  }
  if (raw.api_version !== "1") {
    throw new ManifestError(`api_version non supportée: ${raw.api_version}`);
  }

  const permissions: string[] = Array.isArray(raw.permissions) ? raw.permissions : [];
  for (const p of permissions) {
    if (!VALID_PERMISSIONS.has(p)) {
      throw new ManifestError(`permission inconnue: ${p}`);
    }
  }

  const programs: string[] = Array.isArray(raw.programs) ? raw.programs : [];
  if (programs.length > 32) {
    throw new ManifestError("trop de programmes listés (32 max)");
  }

  const examples: string[] = Array.isArray(raw.examples) ? raw.examples : [];

  const args: ManifestArgument[] = (Array.isArray(raw.arguments) ? raw.arguments : [])
    .filter((a: any) => a && typeof a.name === "string")
    .map((a: any) => ({
      name: a.name,
      description: typeof a.description === "string" ? a.description : "",
      value_type: typeof a.value_type === "string" ? a.value_type : "string",
      required: a.required === true,
      positional: a.positional === true,
      default: a.default,
      choices: Array.isArray(a.choices) ? a.choices.map(String) : undefined,
    }));

  const manifest: ParsedManifest = {
    id: raw.id,
    name: raw.name,
    description: raw.description,
    version: raw.version,
    author: raw.author,
    api_version: raw.api_version,
    permissions,
    programs,
    min_engine_version: typeof raw.min_engine_version === "string" ? raw.min_engine_version : undefined,
    examples,
    category: typeof raw.category === "string" ? raw.category : undefined,
    tags: Array.isArray(raw.tags) ? raw.tags.map(String) : [],
    license: typeof raw.license === "string" ? raw.license : undefined,
    arguments: args,
  };

  return manifest;
}

/** What the site shows as "usage": everything but identity fields. */
export function usageDoc(m: ParsedManifest) {
  return {
    arguments: m.arguments,
    examples: m.examples,
    programs: m.programs,
    min_engine_version: m.min_engine_version ?? null,
    license: m.license ?? null,
    author: m.author,
  };
}

/** Reads only manifest.toml (does not inflate module.wasm). */
export function parseManifestOnly(bytes: Uint8Array): ParsedManifest {
  const entries = unzipSync(bytes, { filter: (f) => f.name === "manifest.toml" });
  const manifestBytes = entries["manifest.toml"];
  if (!manifestBytes) throw new ManifestError("manifest.toml manquant dans l'archive");
  return parseManifestBytes(manifestBytes);
}

export async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}
