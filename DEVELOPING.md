# KTRX — Guide développeur

Le seul document pour travailler sur KTRX : créer et publier des outils, publier le moteur, utiliser l'API du registre, comprendre l'architecture. La documentation destinée aux utilisateurs est sur le site (`/docs`) ; ce guide s'adresse à l'équipe.

**Sommaire** — [1. Les rôles](#1-les-rôles) · [2. Démarrage rapide](#2-démarrage-rapide) · [3. Instructions pour une IA](#3-instructions-pour-une-ia) · [4–12. Créer un outil](#4-règle-darchitecture) · [13. `ktrx-dev`](#13-ktrx-dev) · [14. Publication et versions](#14-publication-et-versions) · [15. API du registre](#15-api-du-registre) · [16. Le moteur](#16-le-moteur) · [17. Défauts fréquents](#17-défauts-fréquents-à-éviter)

## 1. Les rôles

| | Qui | Ce que c'est |
|---|---|---|
| **Moteur `ktrx`** | Tout le monde | Installe, met à jour et **exécute** des outils (`install`, `update`, `uninstall`, `list`, `search`, `run`, `info`). Il ne sert pas à en créer. |
| **Site et registre** | Tout le monde, en lecture | Catalogue public des outils, téléchargement du moteur, documentation utilisateur. Il **stocke tout** : outils, sources, binaires. |
| **`ktrx-dev`** | L'équipe | Crée, vérifie et publie des outils, récupère leurs sources, publie le moteur, gère les clés. Parle à l'API d'administration avec une clé personnelle. Contient ce guide. |
| **GitHub** | Personne, au quotidien | Sert uniquement à **compiler** le moteur et `ktrx-dev` pour toutes les plateformes (`release.yml`, sur un tag). Aucun outil, aucune publication, aucun parcours d'équipe n'en dépend. |

Ce qui est réservé à l'équipe :

- **L'API d'écriture** (`/api/admin/…`) refuse toute requête sans clé valide. Elle n'est liée nulle part sur le site. Chaque membre a sa propre clé, révocable, et chaque action est enregistrée dans un journal.
- **`ktrx-dev`** n'est pas lié sur le site public (son installateur existe, mais il ne sert à rien sans clé).
- **Le moteur public** n'embarque aucune commande de création : `pack`, `validate`, `describe` et `capabilities` n'existent que dans `ktrx-dev`.

**Tout passe par le registre, rien par Git.** Un outil se crée, se vérifie et se publie depuis n'importe quelle machine, dans n'importe quel dossier. Ses sources sont conservées sur le registre à chaque publication : sur une autre machine, `ktrx-dev get` les récupère pour continuer. Les 20 premiers outils (d'avant 0.6.1) vivent encore dans le dépôt Git ; ils n'ont pas de sources sur le registre tant qu'ils ne sont pas republiés.

## 2. Démarrage rapide

Sur une machine vierge, trois choses à installer, une seule fois :

1. **Rust** (les outils sont écrits en Rust) : <https://rustup.rs> (Windows : `winget install Rustlang.Rustup`, puis les « Visual Studio Build Tools » que rustup propose). Puis `rustup target add wasm32-wasip2`.
2. **`ktrx-dev`**, déjà compilé, sans droits administrateur, depuis le site :

```sh
# macOS, Linux
curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.sh | sh
```
```powershell
# Windows (PowerShell)
irm https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.ps1 | iex
```

3. **Sa clé d'API** : un administrateur la crée avec `ktrx-dev keys create "Alice"` (une clé par personne ou par machine, affichée une seule fois). Dans un terminal : `ktrx-dev login` (saisie masquée, stockée dans `~/.config/ktrx/dev.toml`, `%APPDATA%\ktrx\dev.toml` sous Windows).

Puis `ktrx-dev doctor` vérifie tout et dit quoi faire ; `ktrx-dev doctor --fix` installe seul la cible WebAssembly et rustfmt.

**Créer et publier un outil**, dans n'importe quel dossier :

```sh
ktrx-dev init                          # facultatif : AGENTS.md et CLAUDE.md pour une IA lancée dans ce dossier
ktrx-dev new neith JSON-Inspector --category analysis
# → "NEITH ( JSON-Inspector )/" : code, manifeste, tests, README modèle, emblème par défaut. Il compile déjà.
#   Remplir : src/lib.rs, manifest.toml, README.md (chaque « À COMPLÉTER » / « À REMPLACER »), et icon.png si tu l'as.
ktrx-dev check neith                   # tests, compilation, archive, exemples exécutés, règles de documentation
ktrx-dev publish neith                 # refait check, puis publie archive, README, emblème et sources
```

`publish` affiche l'adresse de la page de l'outil et la commande d'installation. L'outil est visible tout de suite sur le site et installable avec `ktrx install neith`.

**Continuer sur une autre machine :** `ktrx-dev get neith` récupère les sources publiées, dans le dossier courant. On modifie, on incrémente `version` (`manifest.toml` ET `Cargo.toml`), puis `ktrx-dev check neith` et `ktrx-dev publish neith`.

**Lire le guide sans rien d'autre :** `ktrx-dev guide` l'affiche en entier (il est embarqué dans le programme, donc toujours celui de la version installée), `ktrx-dev guide --list` liste les sections, `ktrx-dev guide 9` affiche la section 9.

## 3. Instructions pour une IA

Le parcours d'une IA dans un dossier vide : elle installe `ktrx-dev`, lit le guide avec `ktrx-dev guide`, puis crée l'outil. Prompt à coller dans une session Claude Code (ou autre) ouverte dans un dossier vide, **après** que tu as lancé toi-même `ktrx-dev login` dans ton terminal (la clé ne doit pas passer par la conversation) :

````text
Tu vas créer un outil pour KTRX. Rien n'est installé hors Rust.
1. Installe ktrx-dev si `ktrx-dev --version` échoue :
   macOS/Linux : curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.sh | sh      Windows (PowerShell) : irm https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.ps1 | iex
   (ouvre un nouveau terminal, ou ajoute ~/.ktrx/bin au PATH, si la commande est introuvable)
2. Lance `ktrx-dev doctor --fix`, puis corrige ce qu'il signale encore. Une clé d'API valide est déjà enregistrée : ne la demande pas, ne l'affiche pas.
3. Lis le guide : `ktrx-dev guide` (au minimum les sections 3 à 12 : `ktrx-dev guide 3`, `ktrx-dev guide 7`, `ktrx-dev guide 12`…).
4. `ktrx-dev init`, puis crée l'outil : <décris ici ce qu'il doit faire>.
   `ktrx-dev new <NOM> <Nom-Lisible> --category <investigation|analysis|encoding|generation>` (NOM = nom mythologique en majuscules A-Z, unique ; la commande refuse un nom pris).
5. Dans le dossier créé, remplace TOUS les « À COMPLÉTER » et « À REMPLACER » :
   - src/lib.rs : la logique dans `execute`, avec des tests unitaires déterministes (sans Internet). Erreur d'entrée = `Err(message)`, jamais de panic ni d'unwrap sur une entrée.
   - manifest.toml : `description` (1 à 2 phrases), `[[arguments]]` (chacun bien décrit), `examples` sous la forme `ktrx run <nom> …`, et SEULEMENT les `permissions` nécessaires. Jamais de liste d'hôtes dans le manifeste. Tout accès réseau, fichier ou programme passe par `ktrx::tool::host::request` (guide, section 9) ; l'outil n'a aucun accès implicite.
   - README.md : le modèle de la section 12, sans image ni HTML.
6. `ktrx-dev check <nom>` : corrige tout jusqu'à « Tout est bon ». Ne contourne aucun contrôle.
7. Seulement alors : `ktrx-dev publish <nom>`. Donne-moi l'adresse de la page affichée à la fin.

Règles : un outil = une fonction. Une version publiée ne change jamais : pour modifier un outil, `ktrx-dev get <nom>`, incrémente `version` (manifest.toml ET Cargo.toml), puis check et publish. N'ajoute jamais de clé d'API dans un fichier. N'utilise ni Git ni GitHub.
````

`ktrx-dev check` est le garde-fou : s'il est vert, l'outil respecte les règles, compile, s'installe et ses exemples tournent.

---

# Créer un outil

## 4. Règle d’architecture

Un outil est un composant WebAssembly autonome qui reçoit un objet JSON, effectue son travail, puis renvoie un texte JSON. Le moteur KTRX ne fait que charger, valider et exécuter ce composant, demander les permissions annoncées dans son manifeste et transmettre les résultats et événements.

Toute logique métier appartient à l’outil : protocoles, HTTP, TLS, DNS de haut niveau, stratégie de scan, détection, parsing, limites de requêtes, corrélation, résultats et recommandations. N’ajoute jamais au moteur une fonction propre à un outil, à un protocole ou à une famille d’outils. Le moteur expose seulement les primitives génériques déjà définies ci-dessous. Si une capacité manque, conçois l’outil autour des capacités existantes ; toute proposition de changement d’API doit rester générique, réutilisable et versionnée séparément.

Les composants n’ont aucun accès implicite au système : pas de sockets WASI, de processus, de variables d’environnement, de terminal ni de système de fichiers. Lancer un programme hôte n’est possible qu’avec la permission explicite `process` (moteur 0.4+), approuvée par l’utilisateur à chaque exécution. Les seules entrées/sorties autorisées sont l’argument JSON, le résultat JSON, les événements de progression, et les appels explicitement permis à l’hôte.

## 5. Nommage mythologique obligatoire

Chaque outil, existant ou futur, suit cette présentation :

- Dossier : `tools/<NOM-MYTHOLOGIQUE> ( <Nom lisible> )/` — par exemple `tools/HORUS ( Port-Scanner )/`.
- Archive livrable : `K~<NOM-MYTHOLOGIQUE>.tool` — par exemple `K~HORUS.tool`.
- Nom visible dans le manifeste : `K~<NOM-MYTHOLOGIQUE>`.
- Identifiant technique `id` : slug stable en minuscules ASCII avec lettres, chiffres et tirets, par exemple `port-scanner`. Ne mets pas `K~` dans cet identifiant : il doit respecter le parseur du moteur et rester stable pour éviter de casser les intégrations.
- README : titre `# K~<MYTHE> — <Nom lisible>`, puis, juste dessous, une seule ligne de présentation mythologique en citation (`>`), puis le fonctionnement réel (modèle en §9). Exemple de ligne : `HORUS, « l’œil du faucon » dans la mythologie égyptienne : cet outil observe les ports TCP d’une cible pour révéler les services accessibles.`
- Emblème : `icon.png` à la racine du dossier de l’outil (PNG carré 1:1, 1024 × 1024 px, sans texte), dans le style décrit ci-dessous. Il est affiché par le site du registre (carte et page de l’outil), jamais dans le README : **aucune image ni en-tête HTML dans les README**.

Choisis un nom distinct, mémorisable et lié à l’usage, sans prétendre que l’outil possède une capacité qu’il n’a pas. Conserve l’orthographe du nom choisi partout. Ne renomme pas `K~` et n’utilise pas deux fois le même nom dans le projet. Le SDK partagé `tools/ktrx-net-sdk/` n’est pas un outil `.tool` et garde son nom technique.

### Emblème

Chaque outil a un emblème, `icon.png`, à la racine de son dossier : une relique mythologique monumentale, filmée comme une pièce d'archive classée. `ktrx-dev new` fournit l'emblème par défaut (un monolithe scellé au sigle K) : remplacer `icon.png` suffit, sans toucher au reste. `ktrx-dev publish` envoie une version réduite à 320 px ; l'original reste dans le dépôt.

| Critère | Valeur |
|---|---|
| Nom et emplacement | `icon.png`, à côté de `README.md` et `manifest.toml` |
| Format | PNG, ratio **1:1** (carré), 1024 × 1024 recommandé (256 minimum, vérifié par `check`) |
| Poids | idéalement sous 2 Mio (10 Mio maximum) |
| Texte dans l'image | aucun : le nom est ajouté par le site |
| Archive `.tool` | `icon.png` n'y est pas inclus |

**Direction artistique.** Matière : pierre taillée ou métal terni, fissures, gravures, glyphes. Lumière : basse luminosité, éclairage dramatique, brume volumétrique, ombres profondes. Palette : gris-bleu désaturé, noirs profonds, **un seul accent ambre** (yeux, braise, fissure). Composition : sujet unique, centré, symétrique, avec de l'espace négatif. Ambiance : artefact antique classé, archive secrète, cinématographique et réaliste. À éviter : couleurs vives, cartoon, vecteur plat, technologie moderne, texte, logo, filigrane.

Le sujet évoque le mythe et un détail renvoie à la fonction de l'outil (ANUBIS : masque de chacal et balance de la pesée du cœur ; ARGUS : tête de géant couverte d'yeux sculptés, un seul éveillé en ambre ; HERMES : sandales ailées de métal, caducée…).

Bloc de style commun, à coller dans chaque prompt d'image :

```text
Dark cinematic fantasy-archive style, monumental sculpted grey stone and tarnished metal, carved glyphs and runic marks, low-key dramatic lighting, volumetric fog, desaturated blue-grey palette with a single amber accent glow, heavy black shadows, centered symmetrical composition, gritty realistic texture, ancient mythological artifact, classified secret-archive mood, no text, no letters, no watermark, no logo
```

Modèle : `<SUJET MYTHOLOGIQUE : relique, statue ou créature sculptée dans la pierre ou le métal>, <détail lié à la fonction de l'outil>, <BLOC DE STYLE COMMUN>`. Paramètres : ratio 1:1, image de référence de style avec un poids modéré (`--sref` sur Midjourney), même *seed* si possible. Prompt négatif : `text, letters, watermark, signature, bright colors, cartoon, anime, flat vector, blurry, extra limbs, modern technology`.

## 6. Format distribué

Une archive `.tool` est un ZIP contenant exactement les éléments indispensables à la racine :

```text
manifest.toml
module.wasm
```

Le module est un composant WebAssembly Component Model ciblant `wasm32-wasip2` et l’interface KTRX API 1. Limites moteur : archive 64 MiB, manifeste 256 KiB, composant 48 MiB. N’inclus pas les sources dans l’archive ; ils restent à côté dans le dossier de développement. Le dépôt contient le code source, son `Cargo.lock`, son README, son emblème `icon.png` (absent de l’archive), son manifeste, son dossier `wit/` (copie de `engine/wit/ktrx.wit`) et, une fois compilée, l’archive `K~<MYTHE>.tool`, que `ktrx-dev check` produit et `ktrx-dev publish` envoie au registre (§14). N’ajoute pas `target/` ni `module.wasm` généré au dépôt (le `.gitignore` créé par `ktrx-dev new` les exclut).

Arborescence de développement :

```text
tools/HORUS ( Port-Scanner )/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── icon.png
├── manifest.toml
├── src/lib.rs
├── wit/ktrx.wit
└── K~HORUS.tool   (produit par `ktrx-dev check`, non versionné)
```

Chaque outil est un crate Rust indépendant. Ne l’ajoute pas au workspace Cargo du moteur et n’exécute jamais le plugin comme un binaire natif.

## 7. Manifeste requis

Exemple complet minimal :

```toml
id = "example-tool"
name = "K~NEITH"
description = "Valide et met en forme du JSON fourni en entrée."
version = "1.0.0"
author = "KTRX"
api_version = "1"
permissions = []
# Métadonnées du registre (facultatives, ignorées par le moteur) :
category = "analysis"          # investigation | analysis | encoding | generation
tags = ["json", "format"]
license = "MIT"
examples = [
  "ktrx run neith '{\"a\":1}'",
  "ktrx run neith '{\"a\":1}' --compact",
]

[[arguments]]
name = "json"
description = "Document JSON à analyser."
value_type = "string"
required = true
positional = true

[[arguments]]
name = "compact"
description = "Émet une sortie compacte."
value_type = "boolean"
required = false
positional = false
default = false

[[arguments]]
name = "indent"
description = "Largeur d’indentation de la sortie."
value_type = "string"
required = false
positional = false
default = "2"
choices = ["2", "4", "tab"]
```

Règles :

- `id` : 1 à 80 caractères, minuscules ASCII, chiffres et tirets ; commence par une lettre et finit par une lettre ou un chiffre. Il doit être unique et ne change jamais : c’est la clé de l’outil dans le registre.
- `name` : `K~` suivi du nom mythologique en **majuscules A–Z** (le registre refuse autre chose).
- `name`, `description`, `version`, `author`, `api_version` sont obligatoires et non vides. `api_version` vaut actuellement `"1"`.
- Versionne le plugin en SemVer (`1.0.0`, `1.0.1`, etc.). Une version publiée sur le registre ne peut plus être remplacée : toute modification de l’archive demande une nouvelle version, identique dans `manifest.toml` et `Cargo.toml` (§14).
- `description` : une à deux phrases, qui expliquent ce que fait l’outil. Elle s’affiche sur la carte du catalogue et en tête de la page de l’outil.
- `permissions` ne contient que `"network"`, `"file-read"`, `"file-write"`, `"raw-net"` et/ou `"process"`, sans doublon et seulement si nécessaires (`file-write` : écriture confinée au dossier choisi par l’utilisateur avec `--write-dir` ; `raw-net` : ICMP, ARP et sondes TCP brutes, qui peuvent exiger des droits élevés ; `process` : lancer des programmes de la machine, hors du bac à sable, avec les droits de l’utilisateur). N’ajoute jamais de liste d’hôtes, de destinations permises ou d’autorisation réseau dans le manifeste.
- `programs` (moteur 0.4+, facultatif mais fortement recommandé avec `process`) : noms nus des programmes que l’outil lance, cherchés dans le `PATH` (par exemple `programs = ["git", "nmap"]`, 32 noms au plus, lettres, chiffres, `.`, `_`, `-`, `+`). L’hôte refuse alors tout autre programme et tout chemin, et l’invite d’approbation les nomme. Sans liste, l’invite avertit que *n’importe quel* programme peut être lancé.
- `min_engine_version` (facultatif) : version minimale du moteur, `MAJOR.MINOR.PATCH` (le moteur actuel est en 0.5.x ; la 0.5 n’ajoute aucune opération hôte à la 0.4). Mets `"0.4.0"` dès que l’outil dépend de `process.*`, `host.info`, `socket.starttls` ou `socket.shutdown` ; un moteur plus ancien affichera alors un message clair au lieu d’une erreur d’opération.
- `arguments` : nom en minuscules ASCII/chiffres/tirets, description utile, type parmi `string`, `integer`, `boolean`, `path`; préciser `required`, `positional` et si utile `default` du même type. Chaque nom est unique. La description d’un argument est affichée telle quelle dans le tableau « Arguments » du site : écris-la pour l’utilisateur (que fait l’option, unité, plage de valeurs).
- `choices` (argument, facultatif) : liste des valeurs admises, affichée sur le site. **Le moteur ne l’applique pas** : l’outil doit lui-même refuser une valeur hors liste avec une erreur JSON claire.
- Une option booléenne se passe comme `--compact` ; les options non booléennes prennent une valeur après le nom (`--limit 100`). Les arguments positionnels suivent leur ordre dans le manifeste.
- `execution_timeout_seconds` est optionnel. Absent ou `0` signifie sans durée maximale. Une durée explicite doit être entre 30 et 180 secondes. Ne définis pas de plafond global pour une exécution interactive qui doit progresser en direct.
- `examples` : commandes réellement valides, une par entrée, du plus simple au plus complet. Écris-les sous la forme `ktrx run <mythe-en-minuscules> …` (l’outil installé se lance par son nom). Les exemples de la forme `ktrx run 'tools/X ( Y )/K~X.tool' …` fonctionnent depuis la racine du dépôt et le site les réécrit automatiquement en `ktrx run x …`. Ils s’affichent avec un bouton « Copier » dans l’onglet « Utilisation ».
- `category` (facultatif) : `investigation` (Réseau), `analysis` (Analyse), `encoding` (Utilitaires) ou `generation` (Génération). Lue par le registre à la première publication ; sans elle, l’outil est classé « Analyse ». `tags` (liste de mots-clés) et `license` sont informatifs. Le moteur ignore ces trois champs, comme tout champ inconnu.

## 8. Interface Wasm et gestion d’erreurs

L’interface WIT stable est conceptuellement :

```text
tool.run(input: string) -> string
host.request(operation: string, input: string) -> result<string, string>
```

En Rust, générer les bindings et implémenter `exports::ktrx::tool::tool::Guest::run`. `input` est le JSON produit à partir du manifeste ; le résultat doit toujours être du JSON sérialisé. Renvoie par exemple `{"error":"message clair"}` pour une entrée invalide ou une permission refusée. Ne panique pas pour une erreur utilisateur, ne renvoie pas du texte non-JSON et ne transforme pas une erreur en résultat de succès.

Exemple `Cargo.toml` :

```toml
[package]
name = "ktrx-example-tool"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde_json = "1.0"
wit-bindgen = "0.51"
```

Squelette :

```rust
wit_bindgen::generate!({ path: "wit", world: "ktrx-tool" });
use serde_json::{Value, json};

struct Plugin;

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let output = (|| -> Result<Value, String> {
            let args: Value = serde_json::from_str(&input)
                .map_err(|e| format!("entrée JSON invalide : {e}"))?;
            let text = args.get("text").and_then(Value::as_str)
                .ok_or("argument text manquant")?;
            Ok(json!({"result": text.trim()}))
        })();
        match output {
            Ok(value) => value.to_string(),
            Err(error) => json!({"error": error}).to_string(),
        }
    }
}

export!(Plugin);
```

`ktrx-dev new` génère ce squelette, son `Cargo.toml` et le dossier `wit/`. Le `Cargo.toml` est indépendant avec `crate-type = ["cdylib"]`, `wit-bindgen = "0.51"` et `serde_json`. Ajoute seulement des dépendances justifiées et compatibles WASI. Évite les dépendances lourdes pour une transformation simple.

## 9. Capacités génériques autorisées

Appelle toujours `ktrx::tool::host::request(operation, &json_payload.to_string())`. Parse la réponse et traite l’erreur retournée. L’hôte demande une approbation interactive pour chaque exécution nécessitant réseau ou lecture de fichier ; en mode non interactif l’accès est refusé.

| Opération | Requête JSON | Réponse / contraintes |
|---|---|---|
| `host.info` | `{}` | Aucune permission (moteur 0.4+). Réponse `engine_version`, `api_version`, `os`, `family`, `arch`, `cpus`, `permissions` accordées, `programs` autorisés (`"any"`, liste ou `null`), `operations` (chaque opération, ses permissions et `granted`). Sur un moteur plus ancien l’appel échoue : c’est la réponse. |
| `progress.update` | `phase`, `current` requis ; `total`, `recovered`, `item_label`, `result_label`, `done` facultatifs | Renvoie `{"accepted":true}`. Émissions throttled à l’affichage ; l’événement final est préservé. |
| `file.read` | `path`, `offset`, `length` | Réponse `path`, `offset`, `bytes_base64`. Permission `file-read`; fichiers réguliers ≤64 MiB, blocs de 1 à 65536 octets, max. 8 chemins ouverts. Décoder Base64 et lire par blocs. |
| `network.resolve` | `host`, `port` optionnel (défaut 0) | Réponse `host`, `addresses`. Permission `network`. La résolution n’implémente pas le protocole DNS dans l’outil hôte. |
| `socket.open` | `host`, `port`, `timeout_ms` facultatif (50–10000 ms), `tls` booléen facultatif | Connexion TCP immédiate ; réponse `socket`, `host`, `port`, `address` (IP jointe), `tls`, `timeout_ms`. Si `tls=true`, TLS vérifié par l’hôte. |
| `socket.connect` | `host`, `port`, `timeout_ms` facultatif (50–10000 ms) | Démarre une connexion TCP non bloquante et renvoie `connection`. |
| `socket.poll` | `connection`, `wait_ms` facultatif (0–1000 ms, défaut 1) | État `pending`, `failed` avec `error` et `error_kind`, ou `connected` avec `socket`, `address`, `port`. |
| `socket.wait` | `connections` (identifiants en attente ; absent = toutes), `sockets` (TCP/TLS/UDP ouverts), `timeout_ms` (0–1000, défaut 100) | Attend qu’au moins une connexion se termine ou qu’un socket devienne lisible. Réponse `connections` (mêmes objets que `socket.poll`), `readable`, `pending`. À préférer à une boucle de `socket.poll` pour gérer de nombreuses connexions. |
| `socket.read` | `socket`, `length` (1–65536), `timeout_ms` facultatif (50–10000 ms) | Réponse `bytes_base64`, `eof`, `timeout_ms`. Une lecture peut retourner moins d’octets que demandé. |
| `socket.write` | `socket`, `bytes_base64` | Écrit au plus 65536 octets ; réponse `written`. Encoder exactement les octets du protocole en Base64. |
| `socket.close` | `socket` | Ferme le flux ; réponse `closed`. Toujours fermer les sockets, y compris après erreur. |
| `socket.shutdown` | `socket`, `how` (`write` par défaut, `read`, `both`) | Moteur 0.4+. Demi-fermeture sans libérer l’identifiant (envoie `close_notify` en TLS) ; on peut encore lire la réponse du pair. |
| `socket.starttls` | `socket`, `host` facultatif (défaut : hôte d’ouverture), `timeout_ms`, mêmes options TLS que `socket.open` | Moteur 0.4+. Passe un socket TCP en clair en TLS sur place (STARTTLS SMTP/IMAP/FTP, HTTPS via proxy après `CONNECT` : nommer alors la cible dans `host`). Réponse `tls_info`. Un échec ferme le socket. |
| `process.exec` | `program`, `args` (tableau), `cwd`, `env` (objet ; `null` supprime), `clear_env`, `stdin_base64` (≤16 Mio), `timeout_ms` (0 = sans limite), `max_output` (défaut 1 Mio, ≤16 Mio), `output` (`base64` ou `text`) | Permission `process` (0.4+). Exécute jusqu’au bout, sans shell : arguments transmis tels quels. Réponse `pid`, `program` (chemin résolu), `exit_code`, `signal`, `success`, `timed_out`, `killed`, `duration_ms`, `stdout_base64`/`stdout`, `stderr_base64`/`stderr`, `*_bytes`, `*_truncated`. |
| `process.spawn` | mêmes options de lancement, `stdin` (booléen) | Démarre et renvoie `process` (handle), `pid`, `program`. 16 processus vivants au plus. |
| `process.read` | `process`, `stream` (`stdout`/`stderr`), `length` (≤65536), `timeout_ms` (0–10000) | Réponse `bytes_base64`, `eof`, `timeout` (pas encore de données : pas une erreur). Lire les deux flux d’un programme bavard. |
| `process.write` | `process`, `bytes_base64` (≤64 Kio), `close`, `timeout_ms` | Écrit sur l’entrée standard ; `close: true` la ferme. `[closed]` si fermée, `[timeout]` si le programme ne lit pas. |
| `process.wait` | `process`, `timeout_ms` (0–10000) | Réponse `running` puis `exit_code`, `signal`, `success`, `duration_ms`. |
| `process.kill` / `process.close` | `process` | `kill` termine le programme **et tout ce qu’il a lancé** ; `close` libère le handle (en terminant si besoin). La fin de l’exécution, Ctrl+C et le délai d’`exec` terminent aussi tout l’arbre. |
| `file.list` | `path`, `limit` facultatif (≤5000) | Permission `file-read`. Réponse `entries` (`name`, `type`, `size`, `modified`), `total`, `truncated`. |
| `file.stat` | `path` | Permission `file-read`. Réponse `exists` (jamais une erreur si le chemin manque), `type`, `size`, `modified`, `readonly`, `symlink`. |
| `file.create` | `path`, `overwrite` (défaut `false`), `bytes_base64` facultatif | Permission `file-write`. Crée un fichier sans jamais écraser sauf `overwrite`. Tout chemin est résolu dans le dossier approuvé : `..`, chemins absolus extérieurs, liens symboliques sortants et noms de périphériques Windows sont refusés. |
| `file.write` | `path`, `offset`, `bytes_base64` (≤64 Kio) | Permission `file-write`. Écrit dans un fichier créé pendant l’exécution ; fichier ≤256 Mio, ≤1 Gio écrits par exécution, 8 fichiers ouverts. |
| `file.append` | `path`, `bytes_base64`, `create` (défaut `true`) | Permission `file-write`. Ajoute à la fin (journaux, NDJSON). |
| `file.mkdir` | `path`, `recursive` (défaut `true`) | Permission `file-write`. Crée un dossier dans le dossier approuvé. |
| `file.close` | `path` | Ferme et synchronise le fichier ouvert. |
| `dns.query` | `name`, `type` (défaut `A`), `server`, `port`, `timeout_ms`, `recurse`, `dnssec`, `tcp` facultatifs | Permission `network`. Réponse `rcode` (`NOERROR`, `NXDOMAIN`…), `flags`, `answers`/`authority`/`additional` décodés (A, AAAA, CNAME, NS, PTR, MX, TXT, SOA, SRV, CAA ; sinon `raw_base64`), `rtt_ms`. Résolveurs système par défaut ; repli TCP si tronqué. |
| `udp.open` | `family` (`v4`/`v6`), `broadcast`, `ttl`, `bind_port` facultatifs | Permission `network`. Réponse `socket`, `local_port`. |
| `udp.send` | `socket`, `host`, `port`, `bytes_base64` (≤65507 octets) | Réponse `sent`, `address`. |
| `udp.recv` | `socket`, `length`, `timeout_ms` (50–10000 ms) | Réponse `bytes_base64`, `from`, `timeout`, `refused` (ICMP « port unreachable » reçu). Un délai dépassé n’est pas une erreur. |
| `udp.close` | `socket` | Ferme le socket UDP. |
| `icmp.ping` | `host` (IPv4), `ttl` (1–255, défaut 64), `size`, `timeout_ms` | Permission `raw-net`. Statut `reply`, `ttl_expired` (avec `from` = routeur : sert de traceroute), `unreachable`, `timeout`. |
| `arp.resolve` | `ip` (IPv4 du réseau local) | Permission `raw-net` (Windows, Linux). Réponse `mac`. |
| `tcp.syn_probe` | `host` (IPv4), `port`, `timeout_ms` | Permission `raw-net`. `open`/`closed`/`filtered` ; Linux avec root ou CAP_NET_RAW uniquement. |

**Erreurs typées.** Les messages du système d’exploitation peuvent être traduits (un Windows français n’écrit pas « refused ») : ne les analysez jamais. `socket.poll` et `socket.wait` donnent un champ `error_kind` ; les autres opérations réseau préfixent leur erreur par `[kind]`, par exemple `[timeout] socket read failed: …`. Valeurs : `refused`, `timeout`, `reset`, `unreachable`, `closed`, `denied`, `dns`, `tls`, `cancelled`, `other`. `ktrx_net_sdk::error_kind` extrait le préfixe et `ConnectError::is_refused` teste un refus. Les lectures bloquantes s’interrompent en ~100 ms après Ctrl+C avec `[cancelled]`.

Le moteur limite à 128 connexions ouvertes ou en attente. La résolution et les opérations réseau sont permises par le manifeste et par l’approbation de l’utilisateur. `socket.open` accepte aussi `tls` (vérifié par défaut) avec `sni`, `alpn`, `tls_min`/`tls_max` (`1.0` à `1.3`), `ciphers`, `ciphersuites` et `verify` ; la réponse contient `tls_info` (version, chiffrement, ALPN, SNI, résultat et cause de la vérification, chaîne de certificats envoyée par le serveur avec sujet, émetteur, validité, SAN, clé, empreinte SHA-256 et DER). `verify: false` permet d’inspecter un certificat expiré ou auto-signé tout en rapportant le résultat de vérification ; demander TLS 1.0/1.1 abaisse le niveau de sécurité OpenSSL pour cette seule connexion. Le moteur ne décode que deux formats génériques (messages DNS et résumé X.509) et ne rend aucun verdict : il ne fournit pas d’opération HTTP, de scan, de détection de service, de recherche CVE ni de proxy. Un traceroute, un scan ou une note de certificat se construit dans l’outil avec ces primitives. L’écriture de fichiers est confinée au dossier que l’utilisateur désigne (`ktrx run --write-dir DIR`, dossier courant par défaut).

Exemple d’appel host :

```rust
let raw = ktrx::tool::host::request(
    "socket.open",
    &json!({"host": host, "port": 443, "tls": true, "timeout_ms": 4000}).to_string(),
).map_err(|e| format!("connexion refusée ou échouée : {e}"))?;
let opened: Value = serde_json::from_str(&raw)
    .map_err(|e| format!("réponse host invalide : {e}"))?;
let socket = opened.get("socket").and_then(Value::as_u64)
    .ok_or("réponse socket.open sans identifiant")?;
```

Ne contacte jamais de cible cachée ou codée en dur. Le nom de cible doit venir des arguments fournis par l’utilisateur. Ne suppose pas qu’une approbation prouve la propriété de la cible ; garde les scans raisonnablement bornés et documente leur portée.

### 9.1 Lancer des programmes (`process`)

Déclare `permissions = ["process"]` **et** `programs = [...]` : l’utilisateur voit ce qui sera lancé et l’hôte refuse le reste. Passe toujours les arguments sous forme de tableau ; ne construis jamais une ligne de commande pour `sh -c` ou `cmd /c` à partir d’une entrée utilisateur (injection). Valide les arguments venant de l’utilisateur (par exemple refuser une cible commençant par `-`, qui serait prise pour une option). Fixe un `timeout_ms` raisonnable sur `process.exec`, borne `max_output` et traite `stdout_truncated`. L’utilisateur peut restreindre encore avec `ktrx run --allow-program NOM`. Chaque lancement est affiché et publié comme événement `process` : n’en lance pas plus que nécessaire.

```rust
let raw = ktrx::tool::host::request("process.exec", &json!({
    "program": "git",
    "args": ["-C", repo, "log", "--oneline", "-n", "20"],
    "timeout_ms": 30_000,
    "output": "text",
}).to_string())?;
let done: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
if done["success"] != true { /* exit_code, stderr */ }
```

### 9.2 Client HTTP du SDK (HTTP/1.1 et HTTP/2)

Pour le web, n’écris plus ton propre client : `ktrx_net_sdk::http` (SDK 0.2) fournit un client complet sur les sockets génériques — HTTP/2 négocié par ALPN (ou h2c), multiplexage de requêtes vers une même origine (`Client::send_all`), HTTP/1.1 keep-alive, chunked et trailers, décompression gzip/deflate bornée, redirections (désactivées par défaut), proxy HTTP (HTTPS via `CONNECT` + `socket.starttls`, moteur 0.4+), délais et tailles bornés. Les erreurs portent un `ErrorKind` stable (`connect`, `tls`, `timeout`, `protocol`, `closed`, `too_large`, `cancelled`, `denied`…).

```rust
use ktrx_net_sdk::http::{Client, Config, Request, VersionPolicy};
let host = |op: &str, input: &str| ktrx::tool::host::request(op, input);
let mut client = Client::with_config(&host, Config { follow_redirects: 3, ..Config::default() });
let response = client.send(Request::get(url).header("Accept", "text/html"))
    .map_err(|e| e.to_string())?;
// response.version (HTTP/1.1 ou HTTP/2), status, headers, body (décodé), tls, redirects…
let pages = client.send_all(urls.iter().map(|u| Request::get(u.as_str())).collect());
```

Les anciennes fonctions (`http_get`, `open_socket`…) restent disponibles et inchangées. `K~HERMES` montre un usage complet.

## 10. Performance, sûreté et qualité produit

- Concevoir les limites avant d’écrire le parseur : taille d’entrée, taille de réponse, nombre de cibles, profondeur, concurrence, retries, délai par opération et mémoire.
- Préférer des algorithmes linéaires, des buffers bornés, du streaming par blocs et une concurrence bornée. Aucune croissance sans limite en fonction de l’entrée distante.
- Respecter les erreurs partielles et annuler/fermer les ressources sur toutes les branches. Une erreur DNS, permission, TLS, socket ou parsing ne doit pas faire planter le composant.
- Ne pas croire les noms, versions, en-têtes, bannières ou contenus fournis par la cible ; les traiter comme données non fiables. Échapper correctement ce qui apparaît dans les logs ou résultats.
- Ne pas stocker de secrets, tokens, cookies ou captures complètes sans que l’utilisateur en ait besoin. Limiter les extraits et expliquer toute donnée sensible affichée.
- Produire un schéma JSON documenté, stable et utile ; inclure statut, limites atteintes, métriques, résultats structurés et avertissements lorsque cela aide l’interprétation.
- Un outil d’analyse donne des observations et leurs preuves. Ne présente pas une hypothèse comme une vulnérabilité confirmée.
- Ajouter des tests unitaires déterministes des entrées valides, invalides, limites, erreurs de parsing, déduplication et cas limites. Les tests ne doivent pas contacter Internet ou une cible réelle ; injecter des réponses fictives.
- Documentation : un README et un manifeste complets, selon le §12. Documenter explicitement toute absence de validation cryptographique ou toute détection heuristique.

## 11. Progression en direct

Les outils longs n’imposent pas une durée globale ; le moteur peut transmettre la progression pendant leur exécution. Émettre régulièrement `progress.update` après un lot significatif, et une dernière mise à jour avec `done=true`. N’envoie pas un événement pour chaque élément ; garde une granularité utile (par exemple chaque 1–5 % ou à intervalle de quelques centaines de ms). L’appel host est opportuniste : une erreur de progression ne doit pas invalider le résultat métier.

```rust
let _ = ktrx::tool::host::request(
    "progress.update",
    &json!({
        "phase": "analyse", "current": completed, "total": total,
        "recovered": findings, "item_label": "éléments traités",
        "result_label": "observations", "done": completed == total
    }).to_string(),
);
```

Le terminal peut réduire la fréquence d’affichage ; `ktrx run '<fichier.tool>' --events` expose des événements JSON Lines. La file est bornée à 256 événements et peut abandonner des snapshots intermédiaires sous forte pression. `Ctrl+C` demande l’annulation ; un appel host déjà en cours peut aller jusqu’à son délai d’I/O.

## 12. Documenter l’outil

La page d’un outil sur le site est assemblée à partir de trois sources. Chacune a son rôle : n’écris pas deux fois la même information.

| Source | Ce qu’elle alimente sur le site | À y mettre |
|---|---|---|
| `manifest.toml` | Description (en tête et sur la carte), onglet **Utilisation** (tableau des arguments, valeurs admises, valeurs par défaut, exemples copiables), encadré **Permissions** et **Informations** (catégorie, licence, moteur requis) | Tout ce qui décrit l’interface : arguments, exemples, permissions, `programs`, `min_engine_version`, `category`. |
| `README.md` | Onglet **Aperçu** | Les explications : à quoi sert l’outil, comment il raisonne, format de sortie, limites. **Ne recopie pas la liste des options** : elle est déjà générée depuis le manifeste. |
| `icon.png` | Carte et en-tête de la page | L’emblème (§5). |

Le site retire du README le titre `#` (il affiche le nom de l’outil lui-même) et affiche la ligne en citation qui suit comme présentation mythologique, sous la description. Le reste est rendu en Markdown (GFM : titres, listes, tableaux, blocs de code avec bouton « Copier »).

### Modèle de README

````markdown
# K~NEITH — JSON Inspector
> Neith, déesse tisserande de l’Égypte ancienne : cet outil démêle un document JSON pour en révéler la structure.

## Présentation
Deux ou trois phrases : le problème que l’outil résout, ce qu’il fait et ne fait pas.

## Utilisation
```sh
ktrx install neith
ktrx run neith '{"a":1}'
```
Un exemple par bloc, copiable tel quel. Les options sont détaillées dans l’onglet « Utilisation » : n’en refais pas le tableau ici.

## Sortie
Le schéma JSON renvoyé : champs, types, valeurs possibles, exemple réel.

## Limites
Ce que l’outil ne vérifie pas, bornes de taille, durée, concurrence, détections heuristiques.

## Permissions
Pourquoi chaque permission est demandée et ce que l’outil en fait (ou « Aucune permission »).

## Développement
Commandes de test et de compilation de cet outil (§13).
````

Règles :

- **Aucune image, aucun HTML d’en-tête** dans les README (ni dans les sources, ni sur le site). Un lien ou une image relatif (`icon.png`, `../autre/README.md`) ne fonctionne pas sur le site : utilise une URL absolue, ou rien.
- Un titre `#` unique, au tout début ; les sections en `##`. Le site s’appuie sur cet ordre : titre, citation mythologique, puis le corps.
- Les exemples de commande utilisent le nom installé (`ktrx run neith …`), pas un chemin de dossier.
- Même langue dans tout l’outil (README, descriptions des arguments, messages d’erreur) ; le français est la langue par défaut du projet.
- Documente la sortie JSON avec un extrait réaliste et la signification des champs, pas seulement la liste de leurs noms.
- Si l’outil est sensible (génération de contenu offensif, brute force, scan), dis clairement le cadre d’usage légitime : tests autorisés, périmètre, limites intégrées.
- Mets à jour le README en même temps que le comportement : il est republié à chaque `ktrx-dev publish` (ou `publish --docs`), même sans changement de version.

---

## 13. `ktrx-dev`

L'outil de l'équipe. Un seul exécutable, déjà compilé, installé depuis le site par `/install-dev.sh` ou `/install-dev.ps1` (§2). Il est stocké sur le registre à côté du moteur (`/dl/dev/<plateforme>`), publié par `ktrx-dev engine publish` (§16).

| Commande | Rôle |
|---|---|
| `login [--registry URL]` | Enregistre la clé d'API sur la machine après l'avoir testée. |
| `logout`, `whoami` | Supprime la clé locale ; affiche la clé utilisée et ses droits. |
| `doctor [--fix]` | Vérifie Rust, la cible `wasm32-wasip2`, rustfmt, le registre et la clé, et indique la correction de chaque problème ; `--fix` installe seul la cible et rustfmt. |
| `guide [SECTION] [--list]` | Affiche ce guide (embarqué dans le programme). |
| `init` | Crée `AGENTS.md` et `CLAUDE.md` dans le dossier courant : consignes pour une IA lancée là. |
| `get OUTIL [--dir D]` | Récupère les sources publiées d'un outil, sur n'importe quelle machine. |
| `new NOM Nom-Lisible [--category C] [--dir D]` | Crée un outil complet qui compile (§2). Pour une IA : §3. Refuse un nom ou un id déjà pris, localement ou sur le registre. |
| `check OUTIL [--offline]` | Contrôle complet, voir ci-dessous. |
| `publish OUTIL [--changelog "…"] [--docs]` | Refait `check`, puis publie archive, README, emblème **et sources** (zip du dossier sans `target/` ni archive). `--docs` met seulement à jour README, emblème et sources d'une version déjà en ligne. |
| `yank OUTIL VERSION` | Retire une version : elle n'est plus installable, et la précédente redevient la dernière. |
| `delete OUTIL [--yes]` | Supprime un outil et toutes ses versions (clé admin). |
| `engine publish X.Y.Z` | Met sur le site les binaires d'une release GitHub : le moteur et `ktrx-dev` (clé admin, §16). |
| `keys list`, `keys create "Nom" [--admin]`, `keys revoke ID` | Gère les clés de l'équipe (clé admin). |
| `audit [--limit N]` | Dernières actions sur le registre (clé admin). |
| `pack`, `validate`, `describe`, `capabilities` | Outils de bas niveau : empaqueter, valider, décrire une archive, lister les opérations hôte. |

`OUTIL` est un nom (`neith`), `K~NEITH`, un id (`json-inspector`) ou un chemin. L'adresse du registre vient de `ktrx-dev login --registry`, de `KTRX_REGISTRY`, sinon du registre officiel ; la clé peut aussi être donnée par `KTRX_API_KEY` (utile pour un script).

### Ce que `check` vérifie

1. **Manifeste** : règles du moteur, `name` en `K~NOM`, dossier `NOM ( Nom-Lisible )`, `category` valide, description sans « À COMPLÉTER », description de chaque argument, version identique dans `manifest.toml` et `Cargo.toml`, `default` inclus dans `choices`, au moins un exemple.
2. **Documentation** : README au format du §12 (titre, ligne mythologique, sections, sans image ni HTML, sans « À COMPLÉTER »), emblème PNG carré d'au moins 256 px, code sans « À REMPLACER ».
3. **Registre** : id et nom libres (ou appartenant bien à cet outil) ; pour un outil existant, la version doit être supérieure à celle en ligne.
4. **Compilation** : `cargo fmt --check`, `cargo test`, compilation `wasm32-wasip2`, archive `K~NOM.tool`, validation du composant par le moteur.
5. **Exemples** : chaque exemple du manifeste est exécuté avec le vrai moteur et doit renvoyer du JSON sans `error`. Les exemples d'un outil qui demande des permissions ne sont que validés (arguments), pas lancés.

Chaque échec est suivi de la correction à faire (`→ …`). `publish` refuse de continuer tant que `check` n'est pas entièrement vert.

## 14. Publication et versions

- **Une version publiée est immuable.** Republier la même version est refusé (« déjà publiée : incrémentez `version` »). Pour corriger un outil : incrémenter `version` dans `manifest.toml` et `Cargo.toml` (SemVer), puis `ktrx-dev publish`. Les utilisateurs la reçoivent avec `ktrx update`.
- **README et emblème** se remplacent sans changer de version : `ktrx-dev publish OUTIL --docs`.
- **Retirer une version défectueuse** : `ktrx-dev yank OUTIL VERSION`. Elle répond « retirée » et n'est plus installable ; si c'était la seule, l'outil disparaît du catalogue. Puis publier un correctif.
- **Catégorie** : lue dans le manifeste à la publication. `check` refuse une catégorie inconnue.
- **Intégrité** : l'archive est protégée par HTTPS et par son SHA-256, que `ktrx install` revérifie avant d'écrire quoi que ce soit. Les archives ne sont pas signées cryptographiquement : `ktrx install` ne prouve pas l'identité de l'auteur, seulement l'intégrité du fichier publié.
- **Ce que le serveur vérifie** à l'envoi : archive ZIP valide avec `manifest.toml` et `module.wasm` à la racine, tailles dans les limites du moteur (64 Mio, 256 Kio, 48 Mio), champs requis, `id` et `name` valides, `api_version` `"1"`, permissions connues, nom et id non pris par un autre outil. Il lit lui-même les métadonnées dans le manifeste.

## 15. API du registre

L'adresse par défaut est celle du site. Réponses en JSON ; les erreurs ont la forme `{"error": "message"}`.

### Publique (lecture seule)

| Requête | Réponse |
|---|---|
| `GET /api/tools` | `{ tools, categories }` : outils non retirés (id, `name`, `myth_name`, description, catégorie, permissions, dernière version, `page_url`, `download_url`, `icon_url`). |
| `GET /api/tools/<id\|nom>` | La fiche, le `readme`, les `versions` (version, `sha256`, taille, date, `yanked`) et `usage` (arguments, exemples, programmes, moteur minimal, licence). |
| `GET /dl/<id\|nom>[?version=X]` | L'archive `.tool` ; en-tête `x-ktrx-sha256`. `404` inconnu, `410` version retirée. |
| `GET /icons/<id>.png` | L'emblème, réduit à 320 px. |
| `GET /api/engine` | Dernière version du moteur et ses archives par plateforme (`filename`, `sha256`, `size_bytes`, `url`). |
| `GET /dl/engine/<windows\|linux\|linux-arm64\|macos>[?version=X]` | L'archive du moteur ; en-tête `x-ktrx-sha256`. |
| `GET /install.sh`, `/install.ps1` | Les installateurs du moteur (texte UTF-8). |
| `GET /install-dev.sh`, `/install-dev.ps1`, `/dl/dev/<plateforme>` | L'installateur et les binaires de `ktrx-dev` (non liés sur le site ; inutiles sans clé). |

Toute autre méthode que `GET` hors de `/api/admin/` répond `405`. Les téléchargements sont comptés (outil, version, pays, user-agent), sans adresse IP.

### Administration (équipe)

Toutes les routes exigent `Authorization: Bearer <clé>`. Sans clé valide : `401`. Avec une clé qui n'a pas le droit : `403`.

| Requête | Droit | Effet |
|---|---|---|
| `GET /api/admin/whoami` | publish | Identité et droits de la clé. |
| `POST /api/admin/tools[?changelog=…]` | publish | Corps : l'archive `.tool` brute. Crée l'outil ou ajoute une version. `201` / `200`, `409` si version, id ou nom pris, `400` si l'archive est invalide. |
| `PUT /api/admin/tools/<id>/readme` | publish | Corps : le Markdown (≤ 512 Kio). |
| `PUT /api/admin/tools/<id>/icon` | publish | Corps : un PNG (≤ 10 Mio). |
| `PUT /api/admin/tools/<id>/source[?version=X]` | publish | Corps : le zip des sources (≤ 20 Mio). Gardé pour l'équipe seulement. |
| `GET /api/admin/tools/<id\|nom>/source` | publish | Le zip des sources (en-tête `x-ktrx-version`) ; `404` pour un outil publié sans sources. |
| `PATCH /api/admin/tools/<id>` | publish | JSON : `category`, `deprecated`. |
| `POST /api/admin/tools/<id>/versions/<v>/yank` | publish | Retire une version. |
| `DELETE /api/admin/tools/<id>` | admin | Supprime l'outil, ses versions et ses fichiers. |
| `POST /api/admin/engine?version=X.Y.Z&platform=P&filename=F` | admin | Corps : l'archive du moteur ou de `ktrx-dev` (≤ 90 Mio) ; `platform` : `windows`, `linux`, `linux-arm64`, `macos`, ou `dev-<plateforme>` pour `ktrx-dev`. `409` si déjà publiée. |
| `GET`, `POST`, `DELETE /api/admin/keys[/<id>]` | admin | Liste, crée (`{label, scope}`), révoque. |
| `GET /api/admin/audit?limit=N` | admin | Journal des actions. |

**Clés.** Un identifiant, un libellé, un droit : `publish` (publier, mettre à jour la documentation, retirer une version) ou `admin` (tout, dont supprimer, publier le moteur et gérer les clés). Le registre ne conserve que l'empreinte SHA-256 de la clé : une clé perdue ne se récupère pas, on en crée une autre et on révoque l'ancienne. Chaque écriture est inscrite au journal avec la clé utilisée et l'adresse IP. Première clé administrateur : `node site/scripts/create-api-key.mjs "Nom" admin`, puis exécuter le SQL affiché avec `wrangler d1 execute ktrx-catalog --remote --command "…"`.

**Hébergement.** Un Worker Cloudflare (`site/`) : API et site statique. Les fichiers sont dans un bucket R2 (`ktrx-tools` : `tools/<id>/<version>/K~NOM.tool`, `tools/<id>/source.zip`, `icons/<id>.png`, `engine/<version>/<fichier>`), le catalogue dans une base D1 (`ktrx-catalog` : `tools`, `tool_versions`, `engine_releases`, `categories`, `download_events`, `api_keys`, `audit_log`). Déploiement du site : `cd site && npm install && npx wrangler deploy` (nécessite Node et `wrangler login` sur le compte Cloudflare du registre).

**Limites connues.** Pas de limitation de débit configurée. Archives non signées (§14). Les binaires du moteur ne sont pas signés Apple/Microsoft : l'installation en une ligne n'est pas concernée, un fichier téléchargé avec un navigateur peut être bloqué par Gatekeeper ou SmartScreen.

## 16. Le moteur

### Architecture

Frontière stricte entre le mécanisme générique (moteur) et le métier (outils) :

- `ktrx-core` charge et valide manifestes et archives, et transforme la ligne de commande en JSON.
- `ktrx-runtime` compile et exécute les composants WebAssembly (Wasmtime) : un store par exécution, mémoire limitée à 64 Mio, échéances, annulation et pont d'appels hôte génériques. Il ne connaît aucun outil, protocole, scan, empreinte, CVE ni algorithme de hachage.
- `ktrx-cli` (binaire `ktrx`) : exécuter (`run`, `info`) et gérer les outils (`install`, `update`, `uninstall`, `list`, `search`), demander les permissions, afficher résultats et événements. Avec `ktrx-dev`, seul endroit qui fait des requêtes HTTP (client du registre).
- `ktrx-dev` (binaire `ktrx-dev`) : outil de l'équipe (§13).
- `tools/` : les outils et le SDK invité `ktrx-net-sdk` (HTTP/1.1 et HTTP/2, HPACK, redirections…), construit sur les sockets génériques.
- `proxy-app/` : programme natif séparé (proxy d'interception HTTP/HTTPS), non lié au moteur.

Exécution : le moteur ouvre l'archive, valide manifeste et composant, convertit les arguments validés en JSON et appelle `ktrx:tool/tool.run`. Les composants compilés sont mis en cache par SHA-256. Wasmtime reçoit un contexte WASI vide : ni sockets, ni stdio, ni environnement, ni système de fichiers. Les capacités du §9 sont les seules ouvertures, chacune soumise à la déclaration du manifeste et à l'approbation de l'exécution. Une seule table (`ktrx_runtime::capabilities`) liste les opérations et leurs permissions ; elle alimente `host.info` et `ktrx-dev capabilities`.

`process` est la seule capacité qui sort du bac à sable : programmes lancés directement (jamais par un shell), arguments transmis tels quels, liste `programs` du manifeste et `--allow-program` de l'utilisateur, chaque programme dans son propre arbre de processus (objet de job sous Windows, groupe de processus sous Unix) terminé à l'arrêt, à l'annulation et en fin d'exécution, démarrages affichés et publiés comme événements.

Événements : file bornée de 256 entrées ; `progress.update` produit des événements `progress`, chaque programme lancé un événement `process`, `ktrx run --events` les écrit en JSON ligne par ligne. Les instantanés intermédiaires peuvent être abandonnés sous pression, l'événement final est conservé. Les lectures de sockets et `socket.wait` observent l'annulation en ~100 ms.

**Garde d'architecture.** Un test d'intégration (`engine/crates/ktrx-runtime/tests/architecture_boundary.rs`) vérifie que le runtime ne contient aucune API propre à un protocole ou à un outil (HTTP, scan, empreinte…) et que le moteur public n'embarque pas le proxy. Toute nouvelle capacité hôte doit rester un mécanisme générique réutilisable ; le parsing, la politique et l'interprétation restent dans l'outil.

### Compiler et tester

```sh
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli -p ktrx-dev
cargo test  --manifest-path engine/Cargo.toml
```

Rust 1.97 ou plus récent (Wasmtime 50). OpenSSL est compilé depuis les sources et lié statiquement sur toutes les plateformes ; sous Windows il faut les outils MSVC et un Perl natif (Strawberry Perl, pas celui de Git ou MSYS2) et un chemin de compilation ASCII (`set CARGO_TARGET_DIR=C:\ktrx-build`).

Les tests d'intégration passent par `engine/tests/host-probe/`, un petit composant `wasm32-wasip2` qui relaie n'importe quelle opération hôte : permissions déclarées et approuvées, confinement des écritures, UDP, DNS (serveur factice local), TLS (serveur auto-signé local), ICMP, `process.*`, `host.info`, `socket.starttls`, `socket.shutdown`. Comme il relaie tout nom d'opération, une nouvelle opération n'exige pas de reconstruire la fixture. Pour la reconstruire :

```sh
cargo build --manifest-path engine/tests/host-probe/Cargo.toml --release --target wasm32-wasip2
cp engine/tests/host-probe/target/wasm32-wasip2/release/ktrx_host_probe.wasm engine/tests/fixtures/host-probe.component.wasm
```

Les chemins propres à Unix (ICMP brut, sondes SYN, `/proc/net/arp`) se testent sous Linux en root.

### Publier une version du moteur

1. Incrémenter `version` dans `engine/Cargo.toml`, relancer `cargo update --workspace` (met `Cargo.lock` à jour), ajouter l'entrée du `CHANGELOG.md`.
2. Commit, puis `git tag -a vX.Y.Z -m "KTRX X.Y.Z" && git push origin vX.Y.Z`.
3. Le workflow **Release** lance les tests, compile `ktrx` et `ktrx-dev` pour Windows x64, Linux x86_64 et ARM64 (statiques, musl), macOS (binaire universel Apple Silicon + Intel signé ad hoc), vérifie que chaque binaire ne dépend de rien d'installé (statique, ou seulement des bibliothèques du système, ou aucun runtime Visual C++) et qu'il démarre, puis crée la release GitHub avec les archives et `SHA256SUMS.txt`. Le lancer à la main depuis l'onglet Actions construit et vérifie sans publier.
4. Mettre la release sur le site : `ktrx-dev engine publish X.Y.Z`. Il télécharge les archives de la release (moteur **et** `ktrx-dev`), vérifie leurs empreintes avec `SHA256SUMS.txt` et les envoie au registre. Le site propose alors la nouvelle version du moteur, et les installateurs (`/install.sh`, `/install-dev.sh`…) servent les nouveaux binaires. C'est la seule étape qui touche à GitHub : elle lit une release publique, sans identifiant.

Les binaires sont autonomes : OpenSSL intégré, Linux 100 % statique, runtime C statique sous Windows, bibliothèques système seulement sous macOS. La confiance TLS utilise les certificats de la machine ; les racines de Mozilla, embarquées, servent de repli sur Unix sans certificats, et sont toujours ajoutées sous Windows (un Windows neuf n'a qu'une partie des racines publiques). `KTRX_EMBEDDED_CA=1` force les racines embarquées (test).

### Le proxy d'interception (`proxy-app/`)

Programme natif séparé du moteur : un composant `.tool` isolé ne peut ni ouvrir un port d'écoute ni tenir une session de navigateur. Il capture les requêtes HTTP/1.1, y compris HTTPS déchiffré si le navigateur de test fait confiance à l'autorité locale. Chaque requête peut être transmise, modifiée, abandonnée ou rejouée (les 100 dernières, en mémoire seulement).

```sh
cargo run --manifest-path proxy-app/Cargo.toml -- init-ca    # crée ~/.ktrx/proxy-ca/ca.crt et ca.key
cargo run --manifest-path proxy-app/Cargo.toml -- start      # écoute sur 127.0.0.1:8080, confirmation interactive
```

Importer seulement `ca.crt` dans le navigateur de test, puis retirer l'autorité et supprimer la clé après usage. Commandes par requête : `f` transmettre, `e` éditer (`m METHOD /chemin`, `h En-tête: valeur`, `x En-tête`, `s ancien => nouveau`, `done`), `r N` rejouer, `d` abandonner (403), `q` quitter. Limites : HTTP/1.1 seulement (ni HTTP/2 ni WebSocket), corps de requête ≤ 8 Mio, réponses ≤ 32 Mio, pas d'édition des réponses, pas de fichiers d'historique. Les aperçus peuvent contenir des cookies ou jetons : ne l'utiliser que sur des tests autorisés.

## 17. Défauts fréquents à éviter

- Une archive ZIP avec `manifest.toml` imbriqué dans un sous-dossier au lieu de la racine.
- Compiler pour `x86_64`, `aarch64` ou `wasm32-unknown-unknown` plutôt que `wasm32-wasip2`.
- Nommer un champ option différemment dans le manifeste et dans le JSON lu par `run`.
- Oublier qu’un flag booléen CLI ne prend pas `true` en valeur séparée.
- Sortir un JSON d’erreur incorrectement échappé, paniquer sur UTF-8, ou `unwrap()` sur une donnée utilisateur.
- Demander `network` / `file-read` / `file-write` / `raw-net` sans usage réel, ou oublier la permission nécessaire.
- Faire des requêtes HTTP/scan en ajoutant du code au moteur au lieu d’utiliser les sockets génériques.
- Contourner `ktrx-dev check` : il détecte tout ce qui précède avant la publication.
- Passer une entrée utilisateur à `sh -c` / `cmd /c` au lieu d’un tableau `args`, ou déclarer `process` sans liste `programs`.
- Créer une progression purement décorative, publier trop d’événements ou ne pas fournir le résultat final.
- Oublier de fermer les connexions, ignorer les erreurs de socket ou charger une réponse entière sans limite.
- Oublier les guillemets autour des chemins de dossier contenant espaces et parenthèses.
- Corriger un outil sans incrémenter `version` : `ktrx-dev publish` refuse (409), l’ancienne archive reste en ligne.
- Croire que `choices` est vérifié par le moteur : c’est de la documentation, l’outil doit refuser lui-même une valeur hors liste.
- Remettre une image ou un en-tête HTML dans un README, ou un lien relatif (`icon.png`) qui ne marchera pas sur le site.
- Recopier la liste des options dans le README au lieu de bien remplir les `description` des `[[arguments]]` : les deux divergent vite.
- Écrire des `examples` qui ne tournent pas tels quels, ou qui pointent vers un chemin local de ta machine.
- Oublier `category` : l’outil est classé « Analyse » par défaut.

En cas de doute, préserver l’interface publique du moteur et la compatibilité de l’outil ; documenter précisément toute limitation plutôt que de prétendre à une détection garantie.
