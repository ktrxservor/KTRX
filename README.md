# KTRX

KTRX exécute des outils de sécurité, d'analyse et de conversion dans un bac à sable WebAssembly. Le moteur `ktrx` est un seul binaire, sans dépendance ; chaque outil est une archive `.tool` isolée qui déclare ce dont elle a besoin (réseau, fichiers, programmes), et dont chaque accès est demandé à l'utilisateur à chaque exécution.

**Site, catalogue et documentation : <https://ktrx-site.tiagoantunes2009.workers.dev>**

## Installer

```sh
# macOS (Apple Silicon ou Intel) et Linux (x86_64 ou ARM64)
curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install.sh | sh
```

```powershell
# Windows 10/11, dans PowerShell
irm https://ktrx-site.tiagoantunes2009.workers.dev/install.ps1 | iex
```

Aucun droit administrateur, rien d'autre à installer. L'installateur choisit le binaire de la machine, vérifie son SHA-256 et l'ajoute au PATH ; le relancer met à jour.

## Utiliser

```sh
ktrx search                      # le catalogue
ktrx install anubis              # installer un outil (vérifié par SHA-256)
ktrx run anubis --help           # ses options
ktrx run thoth --encode "Bonjour"
ktrx list · ktrx update · ktrx uninstall anubis
ktrx ui                          # l'interface graphique, dans le navigateur
```

Un outil qui demande le réseau, des fichiers ou des programmes n'obtient rien sans votre accord (`-y` accepte d'avance ce que l'outil déclare, `--write-dir` limite l'écriture à un dossier, `--allow-program` restreint les programmes). Documentation complète : page *Documentation* du site.

## Ce dépôt

| | |
|---|---|
| `engine/` | Le moteur `ktrx` et l'outil d'équipe `ktrx-dev` (Rust) |
| `tools/` | Les outils, chacun un projet Rust indépendant (`tools/MYTHE ( Nom )/`), et le SDK `ktrx-net-sdk` |
| `site/` | Le site et l'API du registre (Cloudflare Worker) |
| `proxy-app/` | Proxy d'interception HTTP/HTTPS, programme séparé |

Le site héberge les outils, leurs sources et les binaires ; GitHub sert uniquement à compiler le moteur et `ktrx-dev` pour chaque plateforme (workflow `release.yml`, déclenché par un tag `v*`). Les deux ne sont pas reliés : l'équipe publie les outils et les versions du moteur sur le site avec `ktrx-dev`.

## Contribuer (équipe)

**Interface graphique :** `ktrx ui` ouvre une page locale (thème du site) pour parcourir le catalogue, installer, mettre à jour, supprimer et lancer les outils, avec les mêmes permissions demandées avant chaque exécution.

**Pour les assistants IA :** `ktrx mcp` met les outils à disposition de Claude, Codex, Gemini, Cursor… (`claude mcp add --scope user ktrx -- ktrx mcp`) ; voir la page `/ia` du site.

Tout se trouve dans **[DEVELOPING.md](DEVELOPING.md)**, aussi lisible sans le dépôt avec `ktrx-dev guide` : installer `ktrx-dev` depuis le site, créer et publier un outil en quelques commandes, instructions à donner à une IA, API du registre, architecture du moteur, publication d'une version. Aucun besoin de Git pour créer un outil : tout passe par le registre. Seule l'équipe peut publier (mot de passe de l'équipe).

```sh
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli -p ktrx-dev
cargo test  --manifest-path engine/Cargo.toml
```

Historique des versions : [CHANGELOG.md](CHANGELOG.md). Licence MIT.
