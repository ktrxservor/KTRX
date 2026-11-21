# Registre KTRX

Le registre héberge le catalogue des outils `.tool` et les binaires du moteur `ktrx`. Il alimente le site, la commande `ktrx install` et les téléchargements du moteur.

- Site : <https://ktrx-site.tiagoantunes2009.workers.dev>
- Hébergement : un Worker Cloudflare (API et site), les fichiers dans un bucket R2 (`ktrx-tools`), le catalogue dans une base D1 (`ktrx-catalog`). Le code du Worker est maintenu à part de ce dépôt.
- L’adresse est celle que le moteur utilise par défaut ; `KTRX_REGISTRY` ou `--registry URL` la remplacent.

## Côté utilisateur

```sh
ktrx search hash              # catalogue (sans argument : tout)
ktrx install anubis           # accepte anubis, K~ANUBIS ou l’id hash-audit
ktrx install anubis@1.1.0     # version précise
ktrx list                     # outils installés
ktrx update                   # tous ; ou ktrx update anubis
ktrx uninstall anubis
ktrx run anubis --help        # un outil installé se lance par son nom
```

Dossier d’installation : `--dir`, sinon `$KTRX_HOME/tools`, sinon `~/.ktrx/tools`.

Chaque téléchargement est vérifié avant d’être écrit : le SHA-256 doit être celui publié par le registre, l’archive doit s’ouvrir comme un `.tool` valide (manifeste, `api_version`, `min_engine_version`) et son `id` doit correspondre à l’outil demandé. En cas d’échec rien n’est installé. Le registre doit être en HTTPS.

Le moteur s’installe en une ligne, sans droits administrateur :

```sh
curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install.sh | sh        # macOS, Linux
```

```powershell
irm https://ktrx-site.tiagoantunes2009.workers.dev/install.ps1 | iex             # Windows (PowerShell 5.1 ou 7)
```

Les installateurs (`/install.sh`, `/install.ps1`, servis en texte UTF-8) choisissent l’archive de la machine (`linux`, `linux-arm64`, `macos` universel, `windows`), vérifient son SHA-256 (en-tête `x-ktrx-sha256`), installent `ktrx` dans `~/.ktrx/bin` et l’ajoutent au PATH. Les relancer met à jour. Options : `KTRX_VERSION`, `KTRX_INSTALL`, `KTRX_NO_MODIFY_PATH=1`. Ils évitent aussi la mise en quarantaine qu’applique un navigateur (Gatekeeper sur macOS, marque « téléchargé depuis Internet » sur Windows) à un binaire non signé.

Archives pour une installation manuelle : `/dl/engine/<plateforme>[?version=X]`, avec leur SHA-256 sur le site et dans l’en-tête `x-ktrx-sha256`. Les versions 0.5.0 et 0.5.1 du moteur, qui dépendaient d’un OpenSSL installé, ne sont plus distribuées.

## API publique (lecture)

| Requête | Réponse |
|---|---|
| `GET /api/tools` | `{ tools, categories }` : outils non dépréciés (id, nom, description, catégorie, permissions, dernière version, `download_url`, `icon_url`). |
| `GET /api/tools/<id>` | La fiche, le `readme`, la liste des `versions` (version, `sha256`, taille, date, `yanked`) et `usage` : arguments, exemples, programmes, moteur minimal, licence, tirés du manifeste. |
| `GET /dl/<id>[?version=X]` | L’archive `.tool` (dernière version par défaut). `404` outil ou version inconnus, `410` version retirée. |
| `GET /icons/<id>.png` | L’emblème, réduit pour le web. |
| `GET /api/engine` | Dernière version du moteur et ses archives par plateforme (`filename`, `sha256`, `size_bytes`, `url`), plus la liste des versions. |
| `GET /dl/engine/<plateforme>[?version=X]` | L’archive du moteur. |

Les téléchargements sont comptés (outil, version, pays, user-agent), sans adresse IP.

## API d’administration (écriture)

Toutes ces requêtes exigent `Authorization: Bearer <clé>`. Sans clé valide, ou sans le scope requis, la réponse est `401`.

| Requête | Scope | Effet |
|---|---|---|
| `POST /api/tools[?category=…&changelog=…]` | `upload` | Corps : le fichier `.tool` brut. Le serveur le valide (ZIP, `manifest.toml`, `module.wasm`, limites 64 Mio / 256 Kio / 48 Mio, champs requis, permissions connues), lit lui-même les métadonnées dans le manifeste et enregistre la version. `201` nouvel outil, `200` nouvelle version, `409` version déjà publiée, `400` archive invalide. |
| `POST /api/tools/<id>/icon` | `upload` | Corps : PNG (≤ 10 Mio). Remplace l’emblème. |
| `PATCH /api/tools/<id>` | `upload` | JSON : `description`, `category`, `tags`, `readme`, `deprecated`. |
| `DELETE /api/tools/<id>/versions/<version>` | `delete` | Retire la version (elle répond alors `410`). Avec `?hard=1`, supprime aussi le fichier. |
| `DELETE /api/tools/<id>` | `delete` | Supprime l’outil, ses versions et ses fichiers. |
| `POST /api/engine?version=X.Y.Z&platform=…&filename=…` | `upload` | Corps : l’archive du moteur (≤ 90 Mio). `platform` : `windows`, `linux`, `linux-arm64`, `macos` (universel depuis 0.5.3), `macos-intel` (versions antérieures). `409` si cette plateforme existe déjà pour cette version. |

Le scope `admin` autorise tout. Une version publiée ne change jamais : pour corriger un outil, publier une nouvelle version.

```sh
# Publier ou mettre à jour un outil à la main
curl -X POST "$REGISTRY/api/tools" -H "Authorization: Bearer $KTRX_REGISTRY_KEY" \
  --data-binary @'tools/THOTH ( Base64 )/K~THOTH.tool'

# Retirer une version défectueuse
curl -X DELETE "$REGISTRY/api/tools/base64/versions/1.0.0" -H "Authorization: Bearer $KTRX_REGISTRY_KEY"
```

## Publier depuis ce dépôt

`scripts/publish-tools.sh` publie tous les `tools/*/K~*.tool` : nouvelle version si elle n’existe pas encore, puis emblème et README (qui remplacent les précédents). Le workflow `.github/workflows/registry-publish.yml` l’exécute à chaque push sur `main` qui touche un `.tool`, un `README.md` ou un `icon.png` de `tools/`. Il lit la clé dans le secret GitHub `KTRX_REGISTRY_KEY` ; sans ce secret, il ne publie rien. Utiliser de préférence une clé limitée au scope `upload`.

```sh
KTRX_REGISTRY_KEY=… scripts/publish-tools.sh            # manuellement
```

Le workflow de release du moteur (`engine-release.yml`) envoie de la même façon les archives du moteur au registre à chaque tag `v*`.

## Clés d’API

Une clé a un identifiant, un libellé et des scopes (`upload`, `delete`, `admin`). Le registre ne conserve que son empreinte SHA-256 : une clé perdue ne se récupère pas, on en émet une autre et on révoque l’ancienne. Chaque écriture (création, version, suppression, emblème, moteur) est inscrite dans un journal d’audit avec la clé utilisée et l’adresse IP.

## Données

- R2 : `tools/<id>/<version>/K~NOM.tool`, `icons/<id>.png`, `engine/<version>/<fichier>`.
- D1 : `tools`, `tool_versions` (SHA-256, taille, manifeste), `engine_releases`, `categories`, `download_events`, `api_keys`, `audit_log`.
- Catégories : `investigation` (Réseau), `analysis` (Analyse), `encoding` (Utilitaires), `generation` (Génération), lues dans le champ `category` du manifeste.

## Limites connues

- Les archives ne sont pas signées : l’intégrité repose sur le SHA-256 publié par le registre lui-même et sur HTTPS. Une signature (clé détenue par le mainteneur, vérifiée par `ktrx install`) est prévue mais n’existe pas encore.
- Pas de limitation de débit configurée sur les routes d’écriture ni sur les téléchargements.
- Les binaires du moteur ne sont pas signés (Apple Developer ID, Authenticode). Les installateurs en une ligne évitent les blocages, mais un binaire téléchargé avec un navigateur peut être bloqué par Gatekeeper (macOS) ou signalé par SmartScreen (Windows).
