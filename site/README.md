# Site et registre KTRX

Un Cloudflare Worker : site statique (`public/`) et API (`src/`). Les fichiers sont dans le bucket R2 `ktrx-tools`, le catalogue dans la base D1 `ktrx-catalog` (`migrations/`).

```sh
npm install
npx wrangler login            # le compte Cloudflare qui héberge le registre
npx wrangler deploy
```

Routes, accès équipe, structure des données : voir `DEVELOPING.md` à la racine du dépôt (§15). Les outils et le moteur ne se publient pas depuis ce dossier mais avec `ktrx-dev`.

Mot de passe équipe : secret du Worker `ADMIN_PASSWORD` (`npx wrangler secret put ADMIN_PASSWORD`). Migration : `npx wrangler d1 migrations apply ktrx-catalog --remote`.
