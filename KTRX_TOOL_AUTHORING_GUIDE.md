# KTRX — Guide autonome de création d’outils

Ce fichier est le cahier des charges de référence pour créer un nouvel outil KTRX, du plus petit encodeur à un scanner réseau avancé. Il décrit le format, l’architecture, les noms, les permissions, l’interface Wasm, les performances, la documentation de l’outil, la validation, la publication sur le registre KTRX et la livraison. Une IA ou une personne doit pouvoir s’y conformer sans inventer d’API moteur.

**Parcours d’un outil, en bref** : le code vit dans `tools/<MYTHE> ( <Nom> )/` → on compile et on empaquette `K~<MYTHE>.tool` (§10) → un push sur `main` le publie sur le registre (§11) → il apparaît sur le site avec sa page, et s’installe avec `ktrx install <mythe>`. Le site construit la page de l’outil à partir du **manifeste** (arguments, exemples, permissions) et du **README** (§9) : c’est donc là que se joue la qualité de la documentation.

## 1. Règle d’architecture

Un outil est un composant WebAssembly autonome qui reçoit un objet JSON, effectue son travail, puis renvoie un texte JSON. Le moteur KTRX ne fait que charger, valider et exécuter ce composant, demander les permissions annoncées dans son manifeste et transmettre les résultats et événements.

Toute logique métier appartient à l’outil : protocoles, HTTP, TLS, DNS de haut niveau, stratégie de scan, détection, parsing, limites de requêtes, corrélation, résultats et recommandations. N’ajoute jamais au moteur une fonction propre à un outil, à un protocole ou à une famille d’outils. Le moteur expose seulement les primitives génériques déjà définies ci-dessous. Si une capacité manque, conçois l’outil autour des capacités existantes ; toute proposition de changement d’API doit rester générique, réutilisable et versionnée séparément.

Les composants n’ont aucun accès implicite au système : pas de sockets WASI, de processus, de variables d’environnement, de terminal ni de système de fichiers. Lancer un programme hôte n’est possible qu’avec la permission explicite `process` (moteur 0.4+), approuvée par l’utilisateur à chaque exécution. Les seules entrées/sorties autorisées sont l’argument JSON, le résultat JSON, les événements de progression, et les appels explicitement permis à l’hôte.

## 2. Nommage mythologique obligatoire

Chaque outil, existant ou futur, suit cette présentation :

- Dossier : `tools/<NOM-MYTHOLOGIQUE> ( <Nom lisible> )/` — par exemple `tools/HORUS ( Port-Scanner )/`.
- Archive livrable : `K~<NOM-MYTHOLOGIQUE>.tool` — par exemple `K~HORUS.tool`.
- Nom visible dans le manifeste : `K~<NOM-MYTHOLOGIQUE>`.
- Identifiant technique `id` : slug stable en minuscules ASCII avec lettres, chiffres et tirets, par exemple `port-scanner`. Ne mets pas `K~` dans cet identifiant : il doit respecter le parseur du moteur et rester stable pour éviter de casser les intégrations.
- README : titre `# K~<MYTHE> — <Nom lisible>`, puis, juste dessous, une seule ligne de présentation mythologique en citation (`>`), puis le fonctionnement réel (modèle en §9). Exemple de ligne : `HORUS, « l’œil du faucon » dans la mythologie égyptienne : cet outil observe les ports TCP d’une cible pour révéler les services accessibles.`
- Emblème : `icon.png` à la racine du dossier de l’outil (PNG carré 1:1, 1024 × 1024 px, sans texte), dans le style défini par [`docs/ICON_STYLE_GUIDE.md`](docs/ICON_STYLE_GUIDE.md). Il est affiché par le site du registre (carte et page de l’outil), jamais dans le README : **aucune image ni en-tête HTML dans les README**.

Choisis un nom distinct, mémorisable et lié à l’usage, sans prétendre que l’outil possède une capacité qu’il n’a pas. Conserve l’orthographe du nom choisi partout. Ne renomme pas `K~` et n’utilise pas deux fois le même nom dans le projet. Le SDK partagé `tools/ktrx-net-sdk/` n’est pas un outil `.tool` et garde son nom technique.

## 3. Format distribué

Une archive `.tool` est un ZIP contenant exactement les éléments indispensables à la racine :

```text
manifest.toml
module.wasm
```

Le module est un composant WebAssembly Component Model ciblant `wasm32-wasip2` et l’interface KTRX API 1. Limites moteur : archive 64 MiB, manifeste 256 KiB, composant 48 MiB. N’inclus pas les sources dans l’archive ; ils restent à côté dans le dossier de développement. Le dépôt contient le code source, son `Cargo.lock`, son README, son emblème `icon.png` (absent de l’archive), son manifeste et l’archive compilée `K~<MYTHE>.tool`, que `scripts/publish-tools.sh` envoie au registre (§11). N’ajoute pas `target/` ni `module.wasm` généré au dépôt.

Arborescence de développement :

```text
tools/HORUS ( Port-Scanner )/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── icon.png
├── manifest.toml
├── src/lib.rs
└── K~HORUS.tool
```

Chaque outil est un crate Rust indépendant. Ne l’ajoute pas au workspace Cargo du moteur et n’exécute jamais le plugin comme un binaire natif.

## 4. Manifeste requis

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
- Versionne le plugin en SemVer (`1.0.0`, `1.0.1`, etc.). Une version publiée sur le registre ne peut plus être remplacée : toute modification de l’archive demande une nouvelle version (§11).
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

## 5. Interface Wasm et gestion d’erreurs

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
wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
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

Ajoute un `Cargo.toml` indépendant avec `crate-type = ["cdylib"]`, `wit-bindgen = "0.51"` et `serde_json`. Ajoute seulement des dépendances justifiées et compatibles WASI. Évite les dépendances lourdes pour une transformation simple.

## 6. Capacités génériques autorisées

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

### 6.1 Lancer des programmes (`process`)

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

### 6.2 Client HTTP du SDK (HTTP/1.1 et HTTP/2)

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

## 7. Performance, sûreté et qualité produit

- Concevoir les limites avant d’écrire le parseur : taille d’entrée, taille de réponse, nombre de cibles, profondeur, concurrence, retries, délai par opération et mémoire.
- Préférer des algorithmes linéaires, des buffers bornés, du streaming par blocs et une concurrence bornée. Aucune croissance sans limite en fonction de l’entrée distante.
- Respecter les erreurs partielles et annuler/fermer les ressources sur toutes les branches. Une erreur DNS, permission, TLS, socket ou parsing ne doit pas faire planter le composant.
- Ne pas croire les noms, versions, en-têtes, bannières ou contenus fournis par la cible ; les traiter comme données non fiables. Échapper correctement ce qui apparaît dans les logs ou résultats.
- Ne pas stocker de secrets, tokens, cookies ou captures complètes sans que l’utilisateur en ait besoin. Limiter les extraits et expliquer toute donnée sensible affichée.
- Produire un schéma JSON documenté, stable et utile ; inclure statut, limites atteintes, métriques, résultats structurés et avertissements lorsque cela aide l’interprétation.
- Un outil d’analyse donne des observations et leurs preuves. Ne présente pas une hypothèse comme une vulnérabilité confirmée.
- Ajouter des tests unitaires déterministes des entrées valides, invalides, limites, erreurs de parsing, déduplication et cas limites. Les tests ne doivent pas contacter Internet ou une cible réelle ; injecter des réponses fictives.
- Documentation : un README et un manifeste complets, selon le §9. Documenter explicitement toute absence de validation cryptographique ou toute détection heuristique.

## 8. Progression en direct

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

## 9. Documenter l’outil

La page d’un outil sur le site est assemblée à partir de trois sources. Chacune a son rôle : n’écris pas deux fois la même information.

| Source | Ce qu’elle alimente sur le site | À y mettre |
|---|---|---|
| `manifest.toml` | Description (en tête et sur la carte), onglet **Utilisation** (tableau des arguments, valeurs admises, valeurs par défaut, exemples copiables), encadré **Permissions** et **Informations** (catégorie, licence, moteur requis) | Tout ce qui décrit l’interface : arguments, exemples, permissions, `programs`, `min_engine_version`, `category`. |
| `README.md` | Onglet **Aperçu** | Les explications : à quoi sert l’outil, comment il raisonne, format de sortie, limites. **Ne recopie pas la liste des options** : elle est déjà générée depuis le manifeste. |
| `icon.png` | Carte et en-tête de la page | L’emblème (§2). |

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
Commandes de test et de compilation de cet outil (§10).
````

Règles :

- **Aucune image, aucun HTML d’en-tête** dans les README (ni sur GitHub, ni sur le site). Un lien ou une image relatif (`icon.png`, `../autre/README.md`) ne fonctionne pas sur le site : utilise une URL absolue, ou rien.
- Un titre `#` unique, au tout début ; les sections en `##`. Le site s’appuie sur cet ordre : titre, citation mythologique, puis le corps.
- Les exemples de commande utilisent le nom installé (`ktrx run neith …`), pas le chemin du dépôt.
- Même langue dans tout l’outil (README, descriptions des arguments, messages d’erreur) ; le français est la langue par défaut du projet.
- Documente la sortie JSON avec un extrait réaliste et la signification des champs, pas seulement la liste de leurs noms.
- Si l’outil est sensible (génération de contenu offensif, brute force, scan), dis clairement le cadre d’usage légitime : tests autorisés, périmètre, limites intégrées.
- Mets à jour le README en même temps que le comportement : il est republié à chaque push sur `main`, même sans changement de version.

Le tableau des outils du README racine est généré : après avoir ajouté ou modifié un outil, lance `python3 scripts/readme_tools.py` (la CI vérifie que le tableau est à jour avec `--check`).

## 10. Compilation, paquetage et vérification

Depuis la racine du dépôt, remplacer `<DOSSIER>` par le dossier complet contenant espaces/parenthèses et `<CRATE>` par le nom de la bibliothèque Cargo avec tirets changés en underscores :

```sh
rustup target add wasm32-wasip2
cargo fmt --manifest-path 'tools/<DOSSIER>/Cargo.toml' -- --check
cargo test --manifest-path 'tools/<DOSSIER>/Cargo.toml'
cargo build --manifest-path 'tools/<DOSSIER>/Cargo.toml' --release --target wasm32-wasip2
cp 'tools/<DOSSIER>/target/wasm32-wasip2/release/<CRATE>.wasm' 'tools/<DOSSIER>/module.wasm'
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli
engine/target/release/ktrx pack 'tools/<DOSSIER>' --output 'tools/<DOSSIER>/K~<MYTHE>.tool'
rm 'tools/<DOSSIER>/module.wasm'
engine/target/release/ktrx validate 'tools/<DOSSIER>/K~<MYTHE>.tool'
engine/target/release/ktrx describe 'tools/<DOSSIER>/K~<MYTHE>.tool' --json
```

À chaque changement de source ou de manifeste, reconstruire et repackager l’archive livrée. Pour tester comme un utilisateur, lance exactement les commandes de tes `examples` (en remplaçant le nom installé par `'<chemin>/K~NOM.tool'` tant que l’outil n’est pas publié). Valider le manifeste et le composant. Vérifier aussi une exécution fonctionnelle locale sans réseau quand possible. Pour les capacités réseau, tester avec fixtures ou services locaux si l’environnement l’autorise, sans faire d’accès externe implicite. Les commandes de test et d’exécution doivent pointer vers le fichier `K~NOM.tool`, pas l’ancien nom.

Avant livraison, contrôler :

1. Le nommage mythologique du dossier, du manifeste visible, du README et de l’archive est cohérent.
2. L’identifiant `id` reste un slug valide et distinct ; API version `1`.
3. Aucun accès système natif hors de la permission `process` (avec liste `programs`), aucune permission superflue, hôte en allowlist ou operation moteur spécifique.
4. Chaque entrée est validée ; les erreurs sont JSON ; les tailles, durées et boucles sont bornées.
5. Tests ciblés passent ; `cargo fmt --check`, compilation Wasm, `ktrx validate` et `ktrx describe` passent.
6. L’archive `.tool` est la version compilée de la source présente, et `module.wasm` / `target/` ne sont pas ajoutés par erreur.
7. `icon.png` est présent et conforme à `docs/ICON_STYLE_GUIDE.md` ; le README suit le modèle du §9 (sans image) ; `examples`, `choices`, `category` sont renseignés ; `python3 scripts/readme_tools.py` a été relancé.
8. `version` du manifeste (et de `Cargo.toml`) a été incrémentée si l’archive a changé depuis la dernière publication.

## 11. Publier sur le registre KTRX

Le registre (voir [`docs/REGISTRY.md`](docs/REGISTRY.md)) est ce qui alimente le site, `ktrx install` et `ktrx update`.

**Publier un nouvel outil ou une nouvelle version**

1. Incrémente `version` dans `manifest.toml` et `Cargo.toml` (SemVer).
2. Reconstruis l’archive (§10) et vérifie-la avec `ktrx validate`.
3. Commit et push sur `main`. Le workflow `registry-publish.yml` exécute `scripts/publish-tools.sh` : il envoie l’archive (nouvelle version), l’emblème et le README. Sans le secret GitHub `KTRX_REGISTRY_KEY`, il ne publie rien ; le même script peut aussi être lancé à la main avec `KTRX_REGISTRY_KEY=… scripts/publish-tools.sh`.
4. L’outil apparaît sur le site ; les utilisateurs récupèrent la nouvelle version avec `ktrx update <nom>`.

**Ce que le registre vérifie.** À l’envoi, le serveur ouvre l’archive : ZIP valide, `manifest.toml` et `module.wasm` à la racine, tailles dans les limites du moteur, champs requis, `id` et `name` valides, `api_version` `"1"`, permissions connues. Il lit lui-même les métadonnées dans le manifeste : on ne les saisit nulle part ailleurs. Il calcule le SHA-256, que `ktrx install` revérifie avant d’écrire quoi que ce soit.

**Règles de version.**

- Une version publiée est immuable. Republier la même version est ignoré (réponse `409`, « already published ») : si tu as corrigé l’outil et oublié d’incrémenter `version`, ta correction n’est pas partie.
- Le README et l’emblème, eux, sont remplacés à chaque publication, sans changement de version.
- Une version défectueuse se retire du registre (`DELETE /api/tools/<id>/versions/<v>` avec une clé `delete`) ; elle répond alors « retirée » et n’est plus installable. On publie ensuite un correctif.
- La catégorie n’est lue qu’à la première publication : pour changer celle d’un outil déjà en ligne, `PATCH /api/tools/<id>` avec `{"category": "…"}` (clé `upload`).

**Intégrité.** Les archives sont protégées par HTTPS et par le SHA-256 publié par le registre. Elles ne sont pas encore signées cryptographiquement : ne présente pas `ktrx install` comme une garantie d’authenticité de l’auteur.

**Le moteur.** Un tag `v*` déclenche `engine-release.yml` : quatre archives (Linux, Windows, macOS Apple Silicon, macOS Intel) sont jointes à la release GitHub et, avec le secret `KTRX_REGISTRY_KEY`, envoyées au registre, d’où le site les propose au téléchargement. Une release de moteur suppose la version incrémentée dans `engine/Cargo.toml`, `engine/Cargo.lock` et le `CHANGELOG.md`.

## 12. Défauts fréquents à éviter

- Une archive ZIP avec `manifest.toml` imbriqué dans un sous-dossier au lieu de la racine.
- Compiler pour `x86_64`, `aarch64` ou `wasm32-unknown-unknown` plutôt que `wasm32-wasip2`.
- Nommer un champ option différemment dans le manifeste et dans le JSON lu par `run`.
- Oublier qu’un flag booléen CLI ne prend pas `true` en valeur séparée.
- Sortir un JSON d’erreur incorrectement échappé, paniquer sur UTF-8, ou `unwrap()` sur une donnée utilisateur.
- Demander `network` / `file-read` / `file-write` / `raw-net` sans usage réel, ou oublier la permission nécessaire.
- Faire des requêtes HTTP/scan en ajoutant du code au moteur au lieu d’utiliser les sockets génériques.
- Passer une entrée utilisateur à `sh -c` / `cmd /c` au lieu d’un tableau `args`, ou déclarer `process` sans liste `programs`.
- Créer une progression purement décorative, publier trop d’événements ou ne pas fournir le résultat final.
- Oublier de fermer les connexions, ignorer les erreurs de socket ou charger une réponse entière sans limite.
- Mettre à jour la source sans reconstruire l’archive `.tool` correspondante.
- Oublier les guillemets autour des chemins de dossier contenant espaces et parenthèses.
- Corriger un outil sans incrémenter `version` : le registre garde l’ancienne archive (409) et les utilisateurs ne reçoivent rien.
- Croire que `choices` est vérifié par le moteur : c’est de la documentation, l’outil doit refuser lui-même une valeur hors liste.
- Remettre une image ou un en-tête HTML dans un README, ou un lien relatif (`icon.png`) qui ne marchera pas sur le site.
- Recopier la liste des options dans le README au lieu de bien remplir les `description` des `[[arguments]]` : les deux divergent vite.
- Écrire des `examples` qui ne tournent pas tels quels, ou qui pointent vers un chemin local de ta machine.
- Oublier `category` : l’outil est classé « Analyse » par défaut.

En cas de doute, préserver l’interface publique du moteur et la compatibilité de l’outil ; documenter précisément toute limitation plutôt que de prétendre à une détection garantie.
