# Site et registre KTRX

Un Cloudflare Worker : site statique (`public/`) et API (`src/`). Les fichiers sont dans le bucket R2 `ktrx-tools`, le catalogue dans la base D1 `ktrx-catalog` (`migrations/`).

```sh
npm install
npx wrangler login            # le compte Cloudflare qui héberge le registre
npx wrangler deploy
```

Routes, droits des clés, structure des données : voir `DEVELOPING.md` à la racine du dépôt (§15). Les outils et le moteur ne se publient pas depuis ce dossier mais avec `ktrx-dev`.

Première clé administrateur : `node scripts/create-api-key.mjs "Nom" admin`, puis exécuter le SQL affiché avec `npx wrangler d1 execute ktrx-catalog --remote --command "…"`.
