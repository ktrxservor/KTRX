# KTRX — Guide d'installation et d'exploitation pour une IA

> Adresse de ce fichier : https://ktrx-site.tiagoantunes2009.workers.dev/dev/guide-ia.md

Tu es une IA de code. Tu ne connais pas KTRX et rien n'est installé. Ce fichier a deux parties :

- **Partie 1 — Installation.** Exécute-la maintenant, dans l'ordre, sans rien sauter. À la fin, tu t'arrêtes. **Tu ne crées aucun outil** tant que l'utilisateur ne te le demande pas.
- **Partie 2 — Exploitation.** Tu la gardes en mémoire. Elle te dit comment traiter les demandes suivantes (créer, modifier, publier un outil).

## Ce qu'est KTRX (contexte minimal)

- Un **outil** = un petit projet Rust compilé en WebAssembly (`wasm32-wasip2`) : il reçoit un JSON, renvoie un JSON. Il n'a aucun accès implicite au système ; tout accès réseau, fichier ou programme passe par le moteur et par des permissions déclarées dans son manifeste.
- **`ktrx-dev`** est ton outil de travail : il crée (`new`), vérifie (`check`) et publie (`publish`) des outils. Il contient le guide développeur complet (`ktrx-dev guide`).
- Le **registre** (site) stocke tout : outils, sources, binaires. **N'utilise ni Git ni GitHub** pour les outils.
- Registre : `https://ktrx-site.tiagoantunes2009.workers.dev`

## Règles absolues

1. **Le mot de passe de l'équipe n'est nécessaire que pour publier.** Ne le demande pas pendant l'installation. Ne l'écris jamais dans un fichier, un commit, un README, un log ou une variable d'environnement persistante. Ne l'affiche pas dans tes réponses.
2. **Ne contourne aucun contrôle** de `ktrx-dev check`. Ne publie rien tant que `check` n'est pas entièrement vert.
3. **Une version publiée ne change jamais.** Toute modification d'un outil publié = nouvelle `version`.
4. **Ne jamais deviner** une règle : le guide embarqué (`ktrx-dev guide`) fait foi et correspond à la version installée.
5. En cas de blocage que tu ne peux pas résoudre (droits administrateur, compte, réseau), **arrête-toi et dis précisément à l'utilisateur ce qu'il doit faire**.

---

# Partie 1 — Installation

Chaque étape a un test. Ne passe à la suivante que si le test est bon. Les shells d'outil ne gardent pas l'environnement entre deux appels : sous macOS/Linux, préfixe tes commandes par `export PATH="$HOME/.cargo/bin:$HOME/.ktrx/bin:$PATH";` tant qu'un nouveau shell n'a pas ces chemins.

## 1.1 Détecter le système

macOS/Linux : `uname -sm`. Windows : `echo $env:OS` (PowerShell). Retiens le résultat : les commandes diffèrent.

## 1.2 Rust

Test : `cargo --version` et `rustup --version`.

Si absent :

- macOS / Linux :
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  . "$HOME/.cargo/env"
  ```
- Windows (PowerShell) : `winget install Rustlang.Rustup`. rustup peut exiger les « Visual Studio Build Tools » (installation interactive, droits requis) : si la compilation échoue plus tard pour cette raison, **dis à l'utilisateur de les installer**, puis reprends.

Test : `cargo --version` affiche une version (Rust récent requis, édition 2024 : si `cargo` est ancien, `rustup update stable`).

## 1.3 Cible WebAssembly et rustfmt

```sh
rustup target add wasm32-wasip2
rustup component add rustfmt
```

Test : `rustup target list --installed` contient `wasm32-wasip2`.

## 1.4 Installer `ktrx-dev`

Test : `ktrx-dev --version`. Doit afficher `ktrx-dev 0.6.3` ou plus récent.

Si absent :

- macOS / Linux : `curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.sh | sh` puis `export PATH="$HOME/.ktrx/bin:$PATH"`
- Windows (PowerShell) : `irm https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.ps1 | iex` puis ajoute `%USERPROFILE%\.ktrx\bin` au PATH de la session.

Aucun droit administrateur n'est nécessaire. Si une ancienne version est installée et que `ktrx-dev login --help` ne propose pas `--password`, relance l'installateur pour la mettre à jour.

## 1.5 Vérifier la machine

```sh
ktrx-dev doctor --fix
```

Résultat attendu : toutes les lignes en ✓ (la ligne « mot de passe enregistré » peut être « · » : **c'est normal**, ce n'est pas une erreur) et `Tout est prêt.` Si une ligne est ✗, applique la correction affichée et relance. Ne continue pas tant que `doctor` ne finit pas par `Tout est prêt.`

## 1.6 Dossier de travail

Choisis (ou demande à l'utilisateur) un dossier dédié aux outils KTRX, par exemple `~/ktrx-outils`. Si le dossier courant est déjà celui-là, utilise-le. Ensuite, **depuis ce dossier** :

```sh
ktrx-dev init
```

Cela crée `AGENTS.md` et `CLAUDE.md` pour les sessions futures. Tous les outils seront créés ici (un sous-dossier par outil, nommé `NOM ( Nom-Lisible )`).

## 1.7 Lire le guide embarqué

Lis maintenant ces sections pour les avoir en contexte (elles font foi sur la syntaxe exacte du manifeste, des permissions et des opérations hôte) :

```sh
ktrx-dev guide --list
ktrx-dev guide 4    # architecture
ktrx-dev guide 5    # nommage et emblème
ktrx-dev guide 7    # manifeste
ktrx-dev guide 8    # interface Wasm et erreurs
ktrx-dev guide 9    # capacités hôte (réseau, fichiers, processus)
ktrx-dev guide 10   # performance et sûreté
ktrx-dev guide 11   # progression en direct
ktrx-dev guide 12   # documenter l'outil, modèle de README
ktrx-dev guide 17   # défauts fréquents
```

## 1.8 Fin de la partie 1

Réponds à l'utilisateur, en une courte liste :

- ce qui était déjà installé et ce que tu as installé (versions de `cargo` et de `ktrx-dev`) ;
- le dossier de travail ;
- la phrase : « Prêt. Dis-moi quel outil créer ou modifier. Je te demanderai le mot de passe seulement au moment de publier. »

**Puis arrête-toi. Ne crée aucun outil.**

---

# Partie 2 — Exploitation (pour les demandes suivantes)

Toutes les commandes se lancent depuis le dossier de travail. `OUTIL` = le nom court en minuscules (`neith`), `K~NEITH`, l'id ou le chemin du dossier.

## 2.1 Créer un outil

Quand l'utilisateur décrit un outil (ou donne l'URL d'une documentation : lis-la d'abord, en entier, et vérifie qu'une seule fonction précise ressort) :

1. **Un outil = une fonction.** Si la demande en contient plusieurs, propose de les séparer et demande avant de continuer.
2. **Choisis le nom** : un nom mythologique distinct, en **majuscules A–Z** uniquement, lié à l'usage (ex. `NEITH`). Nom lisible : `JSON-Inspector`. Catégorie : `investigation` (réseau), `analysis`, `encoding` (utilitaires) ou `generation`.
3. `ktrx-dev new NOM Nom-Lisible --category <catégorie>` — refuse un nom déjà pris : change de nom si besoin. L'outil créé compile déjà.
4. **Remplis tous les « À COMPLÉTER » et « À REMPLACER »** du dossier créé :
   - `src/lib.rs` : la logique dans `execute`, avec des **tests unitaires déterministes sans Internet**. Erreur d'entrée = `Err(message)` (renvoyé en `{"error":"…"}`), jamais de panic ni d'`unwrap` sur une donnée utilisateur. Pas de dépendance lourde.
   - `manifest.toml` : `description` (1 à 2 phrases), chaque `[[arguments]]` bien décrit (la description est affichée telle quelle sur le site), `examples` valides et exécutables sous la forme `ktrx run <nom> …`, `category`, et **uniquement** les `permissions` nécessaires. Jamais de liste d'hôtes dans le manifeste. Si `process` est utilisé, déclare `programs`.
   - `README.md` : le modèle du guide §12 — titre `# K~NOM — Nom lisible`, une ligne de citation mythologique, puis Présentation / Utilisation / Sortie (avec un vrai extrait JSON) / Limites / Permissions / Développement. **Aucune image, aucun HTML**, ne recopie pas le tableau des options.
   - Tout accès réseau, fichier ou programme passe par `ktrx::tool::host::request` (guide §9) ; ne rajoute jamais de logique d'outil dans le moteur.
   - `icon.png` : un emblème par défaut est fourni ; ne le remplace que si l'utilisateur fournit une image.
5. `ktrx-dev check OUTIL` → corrige jusqu'à **`Tout est bon`**. Lis chaque message d'erreur : il dit quoi faire.
6. **Teste réellement** : lance au moins un des `examples` (`ktrx-dev check` les exécute ; vérifie aussi que la sortie correspond à ce que dit le README).
7. Publie (§2.4).

## 2.2 Modifier un outil existant

1. `ktrx-dev get OUTIL` récupère les sources publiées dans le dossier courant (si le dossier existe déjà localement, travaille dedans).
2. Fais la modification (code, tests, manifeste, README cohérents entre eux).
3. **Incrémente `version` dans `manifest.toml` ET `Cargo.toml`** (SemVer : correction = patch, ajout = minor, rupture = major). Les deux doivent être identiques.
4. `ktrx-dev check OUTIL` jusqu'à `Tout est bon`, puis publie (§2.4).

Pour ne changer que le README ou l'emblème : `ktrx-dev publish OUTIL --docs` (pas de nouvelle version nécessaire).

## 2.3 Autres actions

- Retirer une version défectueuse : `ktrx-dev yank OUTIL VERSION`, puis publier un correctif.
- Supprimer un outil du registre : `ktrx-dev delete OUTIL`. **Irréversible : demande une confirmation explicite à l'utilisateur en nommant l'outil**, puis `--yes`.
- Voir l'historique : `ktrx-dev audit --limit 20`.

## 2.4 Publier (mot de passe)

1. Vérifie que `ktrx-dev check OUTIL` est vert à l'instant même.
2. Test : `ktrx-dev whoami`.
   - S'il répond « Mot de passe accepté » : continue.
   - Sinon : **demande le mot de passe de publication à l'utilisateur** (une seule fois, brièvement), puis `ktrx-dev login --password '<mot de passe>'`. Il reste enregistré sur la machine (fichier personnel, permissions 0600) : tu ne le redemandes plus. Ne le répète pas dans tes messages.
   - « mot de passe refusé » : redemande-le à l'utilisateur, sans insister plus de deux fois.
3. `ktrx-dev publish OUTIL --changelog "Note de version courte"`.
4. `publish` affiche à la fin l'adresse de la page de l'outil et la commande d'installation (`ktrx install <nom>`). Si elles manquent ou si une erreur apparaît, la publication a échoué : ne dis pas qu'elle a réussi.
5. Réponds avec : le nom, la version, l'adresse de la page, et en une phrase ce que fait l'outil.

Erreur `409` / « déjà publiée » : tu as oublié d'incrémenter `version` (manifeste ET Cargo.toml).

## 2.5 Pièges fréquents (rappel)

- Compiler pour une autre cible que `wasm32-wasip2`.
- `choices` d'un argument n'est **pas** vérifié par le moteur : l'outil doit refuser lui-même une valeur hors liste.
- Un flag booléen CLI s'écrit `--option`, jamais `--option true`.
- Entre guillemets tout chemin contenant espaces ou parenthèses (`"NEITH ( JSON-Inspector )"`).
- Permissions demandées mais inutiles, ou nécessaires mais oubliées.
- `examples` qui ne tournent pas tels quels ou pointent vers un chemin local.
- Ne commite jamais `target/`, `module.wasm`, `node_modules/`.

## 2.6 Quand tu rends la main

Après chaque demande traitée, dis en quelques lignes : ce qui a été fait, la version publiée et l'adresse, et ce qui reste à faire ou n'a pas pu être vérifié. Si tu n'as pas publié, dis-le et pourquoi.
