export function json(data: unknown, status = 200, extra: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...extra },
  });
}

export function error(message: string, status: number): Response {
  return json({ error: message }, status);
}

export function semverParts(v: string): number[] {
  return v.split(".").map((n) => parseInt(n, 10) || 0);
}

export function compareVersions(a: string, b: string): number {
  const x = semverParts(a);
  const y = semverParts(b);
  for (let i = 0; i < 3; i++) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  }
  return 0;
}

export function safeJsonArray(text: string | null): string[] {
  try {
    const value = JSON.parse(text ?? "[]");
    return Array.isArray(value) ? value.map(String) : [];
  } catch {
    return [];
  }
}

export const CATEGORIES = new Set(["investigation", "analysis", "encoding", "generation"]);

/** Engine platforms. macos is universal since 0.5.3; macos-intel only exists for 0.5.2. */
export const PLATFORMS = new Set(["windows", "linux", "linux-arm64", "macos", "macos-intel"]);
