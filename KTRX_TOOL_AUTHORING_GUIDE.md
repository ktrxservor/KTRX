# KTRX — Guide autonome de création d’outils

Ce fichier est le cahier des charges de référence pour créer un nouvel outil KTRX, du plus petit encodeur à un scanner réseau avancé. Il décrit le format, l’architecture, les noms, les permissions, l’interface Wasm, les performances, la validation et la livraison. Une IA ou une personne doit pouvoir s’y conformer sans inventer d’API moteur.

## 1. Règle d’architecture

Un outil est un composant WebAssembly autonome qui reçoit un objet JSON, effectue son travail, puis renvoie un texte JSON. Le moteur KTRX ne fait que charger, valider et exécuter ce composant, demander les permissions annoncées dans son manifeste et transmettre les résultats et événements.

Toute logique métier appartient à l’outil : protocoles, HTTP, TLS, DNS de haut niveau, stratégie de scan, détection, parsing, limites de requêtes, corrélation, résultats et recommandations. N’ajoute jamais au moteur une fonction propre à un outil, à un protocole ou à une famille d’outils. Le moteur expose seulement les primitives génériques déjà définies ci-dessous. Si une capacité manque, conçois l’outil autour des capacités existantes ; toute proposition de changement d’API doit rester générique, réutilisable et versionnée séparément.

Les composants n’ont aucun accès implicite au système : pas de sockets WASI, de processus, de variables d’environnement, de terminal ni de système de fichiers. Les seules entrées/sorties autorisées sont l’argument JSON, le résultat JSON, les événements de progression, et les appels explicitement permis à l’hôte.

## 2. Nommage mythologique obligatoire

Chaque outil, existant ou futur, suit cette présentation :

- Dossier : `tools/<NOM-MYTHOLOGIQUE> ( <Nom lisible> )/` — par exemple `tools/HORUS ( Port-Scanner )/`.
- Archive livrable : `K~<NOM-MYTHOLOGIQUE>.tool` — par exemple `K~HORUS.tool`.
- Nom visible dans le manifeste : `K~<NOM-MYTHOLOGIQUE>`.
- Identifiant technique `id` : slug stable en minuscules ASCII avec lettres, chiffres et tirets, par exemple `port-scanner`. Ne mets pas `K~` dans cet identifiant : il doit respecter le parseur du moteur et rester stable pour éviter de casser les intégrations.
- README : ajouter sous le titre une seule ligne de présentation mythologique, puis décrire clairement le fonctionnement réel. Exemple : `HORUS, « l’œil du faucon » dans la mythologie égyptienne : cet outil observe les ports TCP d’une cible pour révéler les services accessibles.`
- Emblème : `icon.png` à la racine du dossier de l’outil (PNG carré 1:1, 1024 × 1024 px, sans texte), dans le style défini par [`docs/ICON_STYLE_GUIDE.md`](docs/ICON_STYLE_GUIDE.md). Le README commence par le bloc d’en-tête décrit dans cette charte, placé avant le titre.

Choisis un nom distinct, mémorisable et lié à l’usage, sans prétendre que l’outil possède une capacité qu’il n’a pas. Conserve l’orthographe du nom choisi partout. Ne renomme pas `K~` et n’utilise pas deux fois le même nom dans le projet. Le SDK partagé `tools/ktrx-net-sdk/` n’est pas un outil `.tool` et garde son nom technique.

## 3. Format distribué

Une archive `.tool` est un ZIP contenant exactement les éléments indispensables à la racine :

```text
manifest.toml
module.wasm
```

Le module est un composant WebAssembly Component Model ciblant `wasm32-wasip2` et l’interface KTRX API 1. Limites moteur : archive 64 MiB, manifeste 256 KiB, composant 48 MiB. N’inclus pas les sources dans l’archive ; ils restent à côté dans le dossier de développement. Le dépôt contient le code source, son `Cargo.lock`, son README, son emblème `icon.png` (absent de l’archive), son manifeste et l’archive compilée `K~<MYTHE>.tool`. N’ajoute pas `target/` ni `module.wasm` généré au dépôt.

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
examples = ["ktrx run 'tools/NEITH ( Example-Tool )/K~NEITH.tool' --text 'hello'"]

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
```

Règles :

- `id` : 1 à 80 caractères, minuscules ASCII, chiffres et tirets ; commence par une lettre et finit par une lettre ou un chiffre. Il doit être unique.
- `name`, `description`, `version`, `author`, `api_version` sont obligatoires et non vides. `api_version` vaut actuellement `"1"`.
- Versionne le plugin en SemVer (`1.0.0`, `1.0.1`, etc.).
- `permissions` ne contient que `"network"`, `"file-read"`, `"file-write"` et/ou `"raw-net"`, sans doublon et seulement si nécessaires (`file-write` : écriture confinée au dossier choisi par l’utilisateur avec `--write-dir` ; `raw-net` : ICMP, ARP et sondes TCP brutes, qui peuvent exiger des droits élevés). N’ajoute jamais de liste d’hôtes, de destinations permises ou d’autorisation dans le manifeste.
- `arguments` : nom en minuscules ASCII/chiffres/tirets, description utile, type parmi `string`, `integer`, `boolean`, `path`; préciser `required`, `positional` et si utile `default` du même type. Chaque nom est unique.
- Une option booléenne se passe comme `--compact` ; les options non booléennes prennent une valeur après le nom (`--limit 100`). Les arguments positionnels suivent leur ordre dans le manifeste.
- `execution_timeout_seconds` est optionnel. Absent ou `0` signifie sans durée maximale. Une durée explicite doit être entre 30 et 180 secondes. Ne définis pas de plafond global pour une exécution interactive qui doit progresser en direct.
- `examples` doit montrer des commandes réellement valides et citer le chemin avec guillemets, car le dossier contient des espaces et parenthèses.

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

## 7. Performance, sûreté et qualité produit

- Concevoir les limites avant d’écrire le parseur : taille d’entrée, taille de réponse, nombre de cibles, profondeur, concurrence, retries, délai par opération et mémoire.
- Préférer des algorithmes linéaires, des buffers bornés, du streaming par blocs et une concurrence bornée. Aucune croissance sans limite en fonction de l’entrée distante.
- Respecter les erreurs partielles et annuler/fermer les ressources sur toutes les branches. Une erreur DNS, permission, TLS, socket ou parsing ne doit pas faire planter le composant.
- Ne pas croire les noms, versions, en-têtes, bannières ou contenus fournis par la cible ; les traiter comme données non fiables. Échapper correctement ce qui apparaît dans les logs ou résultats.
- Ne pas stocker de secrets, tokens, cookies ou captures complètes sans que l’utilisateur en ait besoin. Limiter les extraits et expliquer toute donnée sensible affichée.
- Produire un schéma JSON documenté, stable et utile ; inclure statut, limites atteintes, métriques, résultats structurés et avertissements lorsque cela aide l’interprétation.
- Un outil d’analyse donne des observations et leurs preuves. Ne présente pas une hypothèse comme une vulnérabilité confirmée.
- Ajouter des tests unitaires déterministes des entrées valides, invalides, limites, erreurs de parsing, déduplication et cas limites. Les tests ne doivent pas contacter Internet ou une cible réelle ; injecter des réponses fictives.
- README : bloc d’en-tête avec l’emblème `icon.png`, une ligne mythologique obligatoire, objectif, capacités, options, exemples copiable-collable, formats de sortie, limitations et permissions sollicitées. Documenter explicitement toute absence de validation cryptographique ou toute détection heuristique.

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

## 9. Compilation, paquetage et vérification

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

À chaque changement de source ou de manifeste, reconstruire et repackager l’archive livrée. Valider le manifeste et le composant. Vérifier aussi une exécution fonctionnelle locale sans réseau quand possible. Pour les capacités réseau, tester avec fixtures ou services locaux si l’environnement l’autorise, sans faire d’accès externe implicite. Les commandes de test et d’exécution doivent pointer vers le fichier `K~NOM.tool`, pas l’ancien nom.

Avant livraison, contrôler :

1. Le nommage mythologique du dossier, du manifeste visible, du README et de l’archive est cohérent.
2. L’identifiant `id` reste un slug valide et distinct ; API version `1`.
3. Aucun accès système natif, permission superflue, hôte en allowlist ou operation moteur spécifique.
4. Chaque entrée est validée ; les erreurs sont JSON ; les tailles, durées et boucles sont bornées.
5. Tests ciblés passent ; `cargo fmt --check`, compilation Wasm, `ktrx validate` et `ktrx describe` passent.
6. L’archive `.tool` est la version compilée de la source présente, et `module.wasm` / `target/` ne sont pas ajoutés par erreur.
7. `icon.png` est présent et conforme à `docs/ICON_STYLE_GUIDE.md`, le README commence par le bloc d’en-tête, et l’outil figure dans la galerie du README racine.

## 10. Défauts fréquents à éviter

- Une archive ZIP avec `manifest.toml` imbriqué dans un sous-dossier au lieu de la racine.
- Compiler pour `x86_64`, `aarch64` ou `wasm32-unknown-unknown` plutôt que `wasm32-wasip2`.
- Nommer un champ option différemment dans le manifeste et dans le JSON lu par `run`.
- Oublier qu’un flag booléen CLI ne prend pas `true` en valeur séparée.
- Sortir un JSON d’erreur incorrectement échappé, paniquer sur UTF-8, ou `unwrap()` sur une donnée utilisateur.
- Demander `network` / `file-read` / `file-write` / `raw-net` sans usage réel, ou oublier la permission nécessaire.
- Faire des requêtes HTTP/scan en ajoutant du code au moteur au lieu d’utiliser les sockets génériques.
- Créer une progression purement décorative, publier trop d’événements ou ne pas fournir le résultat final.
- Oublier de fermer les connexions, ignorer les erreurs de socket ou charger une réponse entière sans limite.
- Mettre à jour la source sans reconstruire l’archive `.tool` correspondante.
- Oublier les guillemets autour des chemins de dossier contenant espaces et parenthèses.

En cas de doute, préserver l’interface publique du moteur et la compatibilité de l’outil ; documenter précisément toute limitation plutôt que de prétendre à une détection garantie.
