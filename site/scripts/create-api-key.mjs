// Generates a new API key, prints the raw secret ONCE, and prints the SQL
// to insert its hash into D1 (run it yourself with `wrangler d1 execute`,
// since this script does not touch the network).
//
// Usage: node scripts/create-api-key.mjs <label> <scope1,scope2,...>
// Example: node scripts/create-api-key.mjs "Alice" publish
// Example: node scripts/create-api-key.mjs "admin" admin
import { randomBytes, createHash } from "node:crypto";

const label = process.argv[2];
const scopesArg = process.argv[3];
if (!label || !scopesArg) {
  console.error("Usage: node scripts/create-api-key.mjs <label> <scope1,scope2,...>");
  console.error("Scopes: publish, admin");
  process.exit(1);
}
const scopes = scopesArg.split(",").map((s) => s.trim());

const id = randomBytes(8).toString("hex");
const secret = "ktrx_" + randomBytes(32).toString("base64url");
const hash = createHash("sha256").update(secret).digest("hex");

console.log("\n=== Clé API générée (copie-la maintenant, elle ne sera plus affichée) ===");
console.log(secret);
console.log("\n=== SQL à exécuter (wrangler d1 execute ktrx-catalog --remote --command \"...\") ===");
console.log(
  `INSERT INTO api_keys (id, key_hash, label, scopes) VALUES ('${id}', '${hash}', '${label.replace(/'/g, "''")}', '${JSON.stringify(scopes)}');`,
);
console.log("\nUtilisation: Authorization: Bearer " + secret + "\n");
