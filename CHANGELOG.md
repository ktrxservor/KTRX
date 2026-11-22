# Changelog

## 0.6.3

**Un mot de passe d'équipe à la place des clés d'API.** Cassant pour `ktrx-dev` : à mettre à jour.

- L'API d'administration (`/api/admin/…`) n'accepte plus qu'un seul mot de passe, secret du Worker `ADMIN_PASSWORD`, qui donne tous les droits. Les clés, leurs droits (`publish` / `admin`), la table `api_keys`, les routes `/api/admin/keys` et `ktrx-dev keys` sont supprimés.
- `ktrx-dev login [--password MDP]` enregistre le mot de passe (saisie masquée, entrée standard ou option) ; variable `KTRX_PASSWORD` à la place de `KTRX_API_KEY`. Seule la publication en a besoin : `new`, `check`, `guide`, `doctor` fonctionnent sans. Une IA peut donc lire une documentation, installer et configurer, coder, puis demander le mot de passe pour publier.

## 0.6.2

- **Mémoire par outil : 512 Mio** (au lieu de 64 Mio). C'est une borne de ressources générique : elle permet à un outil de traiter une image ou un fichier volumineux (un outil de conversion d'images, par exemple) sans rien changer au bac à sable. La mémoire n'est allouée qu'à l'usage ; un outil qui en demande davantage est interrompu.

## 0.6.1

**Plus aucune dépendance à GitHub pour travailler sur les outils.**

- **`ktrx-dev` s'installe depuis le site** : `/install-dev.sh` et `/install-dev.ps1`, binaires servis par `/dl/dev/<plateforme>` (publiés par `ktrx-dev engine publish`, qui envoie désormais le moteur et `ktrx-dev`).
- **`ktrx-dev guide [SECTION] [--list]`** : le guide développeur est embarqué dans le programme. Plus besoin de cloner le dépôt pour le lire.
- **Sources sur le registre** : `ktrx-dev publish` envoie aussi le zip des sources de l'outil ; `ktrx-dev get OUTIL` les récupère sur n'importe quelle machine. API : `PUT` et `GET /api/admin/tools/<id>/source` (clé d'équipe).
- `ktrx-dev init` (AGENTS.md et CLAUDE.md pour une IA lancée dans un dossier), `ktrx-dev doctor --fix` (installe la cible WebAssembly et rustfmt) ; `doctor` ne demande plus de dépôt.
- Les scripts `scripts/install-dev.*` du dépôt sont supprimés (remplacés par ceux du site).

## 0.6.0

**Le moteur public n'exécute plus que des outils ; l'équipe publie avec `ktrx-dev`.** Cassant : les commandes `pack`, `validate`, `describe` et `capabilities` ne sont plus dans `ktrx` (elles sont dans `ktrx-dev`).

- **`ktrx-dev`** (nouveau, livré compilé dans chaque release, installable par `scripts/install-dev.sh` ou `.ps1`) : `login`, `doctor`, `new` (crée un outil complet qui compile), `check` (tests, compilation, archive, exemples exécutés, règles de documentation), `publish`, `yank`, `delete`, `engine publish`, `keys`, `audit`.
- **API d'administration** sous `/api/admin/…`, réservée aux clés de l'équipe (droits `publish` et `admin`, une clé par personne, révocables, journal des actions). Les anciennes routes d'écriture (`POST /api/tools`…) n'existent plus ; l'API publique reste identique.
- **Site refondu** : accueil, catalogue (`/outils`), page d'un outil (`/outils/<nom>`, README rendu, arguments et exemples tirés du manifeste), téléchargement, documentation utilisateur. Le code du site est dans `site/`.
- **Documentation** regroupée : `README.md`, `DEVELOPING.md` (le seul guide développeur, avec instructions pour une IA) et ce fichier.
- **GitHub** réduit à un workflow, `release.yml` : tests, compilation de `ktrx` et `ktrx-dev` pour Windows, Linux x86_64 et ARM64, macOS universel, contrôles « aucune dépendance » et démarrage, release avec `SHA256SUMS.txt`. Plus de publication automatique vers le site.
- Correctif macOS : lire un socket dont le pair venait de fermer la connexion pouvait échouer avec « Invalid argument (os error 22) » ; la lecture renvoie maintenant les données restantes ou la fin de flux.
- `ktrx install`, `update` et `search` honorent `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`.

## 0.5.3 — engine

- **One-line installers.** `curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install.sh | sh` (macOS, Linux) and `irm https://ktrx-site.tiagoantunes2009.workers.dev/install.ps1 | iex` (Windows) pick the right build for the machine, verify its SHA-256, install it in `~/.ktrx/bin` (no administrator rights) and add it to the PATH. Unlike a browser download, they do not trigger macOS Gatekeeper's quarantine or Windows' downloaded-file mark.
- **Linux ARM64** (Raspberry Pi 64-bit, ARM servers): new static musl binary, `ktrx-linux-arm64.tar.gz`.
- **macOS**: a single universal binary (Apple Silicon and Intel, macOS 10.15+) replaces the two separate archives.
- **Windows TLS trust**: Mozilla's embedded roots are now always added to the Windows certificate stores. A fresh Windows only holds part of the public roots (it fetches the others on demand, which OpenSSL never triggers), so some sites failed verification in tools.
- **Proxies**: `ktrx install`, `update` and `search` honour `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY`.
- A weekly workflow (`installers.yml`) runs both installers on fresh runners and bare containers (Alpine, Debian, Fedora, Ubuntu 20.04, Windows PowerShell 5.1 and 7).

## 0.5.2 — engine

Released binaries are now self-contained: they start on a clean machine with nothing installed. 0.5.0 and 0.5.1 were not (the macOS binaries needed Homebrew's OpenSSL, the Linux one needed `libssl.so.3` and glibc 2.39).

- **OpenSSL is built in on every platform** (built from source, linked statically).
- **Linux** is a fully static musl binary: no glibc, no OpenSSL, runs on any distribution and in minimal containers.
- **Windows** links the C runtime statically (`.cargo/config.toml`): no Visual C++ Redistributable needed.
- **macOS** only links system libraries; builds target macOS 11 (Apple Silicon) and 10.15 (Intel).
- **TLS trust**: the machine's CA certificates are used when present (`SSL_CERT_FILE`/`SSL_CERT_DIR` first, then the usual system locations; the Windows certificate stores on Windows). A machine with no CA bundle at all falls back to Mozilla's root list embedded in the binary, for tools (`socket.open` with TLS) and for `ktrx install` alike. `KTRX_EMBEDDED_CA=1` forces the embedded roots (testing).
- The release workflow now checks that each binary has no external dependency and smoke-tests it (including a run on a bare busybox container without CA certificates) before publishing; pushing a `release-test/*` branch runs the same checks without publishing.

## 0.5.1 — engine

- Release builds now include a macOS Intel (x86_64) binary next to the Apple Silicon one (`ktrx-macos-intel.tar.gz`). No engine change.

## 0.5.0 — engine

Host API stays `1`: every existing tool runs unmodified.

### Tool registry in the CLI

- `ktrx install <name>[@version]` downloads a tool from the KTRX registry into `~/.ktrx/tools` (`$KTRX_HOME/tools`, or `--dir`). The name can be `anubis`, `K~ANUBIS` or the technical id.
- `ktrx update [name]`, `ktrx uninstall <name>`, `ktrx list` and `ktrx search [query]` complete the set.
- Installed tools resolve by name: `ktrx run anubis --help`.
- Every download is verified against the SHA-256 published by the registry and opened as a real `.tool` archive (manifest, engine version, matching id) before it is written; on any failure nothing is installed. The registry must use HTTPS (`$KTRX_REGISTRY` or `--registry URL` to change it).

### Distribution

- The catalog and the engine downloads now live on the KTRX registry site; the GitHub Pages catalog was removed.

## 0.4.0 — engine

Host API stays `1` (WIT `ktrx:tool@1.0.0` unchanged): every 0.3 tool runs unmodified. New capabilities are additive.

### Running programs — new `process` permission

- `process.exec` runs a program to completion: arguments passed verbatim (never through a shell), working directory, environment overrides or a clean environment, standard input, timeout, bounded output capture (Base64 or text), exit code, signal, duration.
- `process.spawn`, `process.read`, `process.write`, `process.wait`, `process.kill`, `process.close` drive a program interactively (streamed stdout/stderr, stdin with close, polling, termination). Up to 16 live processes per run.
- Each program runs in its own process tree — a kill-on-close job object on Windows (created suspended, resumed only inside the job), a process group on Unix. Kill, close, Ctrl+C, an `exec` timeout, and the end of the run terminate the whole tree; `exec` also ends background leftovers that would otherwise hold its output open.
- Manifests may list the programs a tool runs (`programs = ["git", "nmap"]`). The host then refuses any other program and any path, and resolves names on `PATH` (with `PATHEXT` on Windows).
- `ktrx run --allow-program NAME` (repeatable) narrows a run further. The approval prompt names the programs, or warns that any program may run.
- Every start is printed on the terminal (`K~TOOL ▸ exécute : …`) and published as a `process` event in `ktrx run --events`.

### Other host additions

- `host.info` (no permission): engine and API version, OS, architecture, CPU count, granted permissions and programs, and every operation with whether it is granted — capability negotiation for tools.
- `socket.starttls`: upgrade an open plaintext socket to TLS in place (STARTTLS, HTTPS through an HTTP proxy), with the same TLS options and `tls_info` report as `socket.open`.
- `socket.shutdown`: half-close a TCP/TLS stream (`close_notify` on TLS) without releasing it.
- `nodelay` option on `socket.open` and `socket.connect`.

### Manifest and CLI

- New permission `process`; new optional fields `programs` and `min_engine_version` (checked when an archive is opened, with a clear message on older-than-required engines).
- `ktrx capabilities [--json]` lists every host operation and the permission it needs.
- `ktrx info` shows a process tool's programs and its minimum engine version.
- `-y` documents that it also approves `process`, still bounded by `programs` and `--allow-program`.

### Quality

- 17 new end-to-end tests (`engine/crates/ktrx-runtime/tests/process.rs`) through the WebAssembly probe: execution, exit codes, verbatim arguments, stdin, cwd/env, truncation, timeouts, background leftovers, interactive sessions, process-tree termination on kill and at run end, manifest and user allow-lists, audit events, `host.info`, STARTTLS with half-close, failed upgrades.
- New unit tests for manifest validation (`programs`, `min_engine_version`, round-trip serialisation).

## ktrx-net-sdk 0.2.0 — web stack

- New `http` module: a complete HTTP client over the host's socket capabilities.
  - HTTP/2 (RFC 9113) negotiated with ALPN, or h2c by prior knowledge: flow control both ways, CONTINUATION, padding, PING, SETTINGS, GOAWAY and RST_STREAM handling, Content-Length checking, trailers, informational responses, server push disabled.
  - `Client::send_all` multiplexes many requests to one origin over a single HTTP/2 connection (up to the server's stream limit); responses come back in request order.
  - HTTP/1.1 (RFC 9112) with keep-alive pooling, chunked coding and trailers, 1xx handling, close-delimited bodies, obsolete line folding, strict status-line and Content-Length validation.
  - gzip/deflate decoding bounded against decompression bombs; redirects (opt-in) with RFC method rewriting and credential stripping across origins; HTTP proxies (absolute-form for `http://`, `CONNECT` + `socket.starttls` for `https://`).
  - Idempotent requests are retried once when a reused connection turns out to be closed, or the server refuses a stream (GOAWAY, REFUSED_STREAM).
  - Stable error kinds (`connect`, `tls`, `timeout`, `protocol`, `closed`, `too_large`, `cancelled`, `denied`, `invalid_request`, `other`).
- New `hpack` module (RFC 7541): full decoder with dynamic table and Huffman coding, stateless encoder; verified against the RFC's appendix C examples.
- The existing helpers (`http_get`, `open_socket`, …) are unchanged; every tool that uses the SDK builds as before.
- 35 tests, including end-to-end runs against local HTTP/1.1 and HTTP/2 servers.

## K~HERMES 2.0.0

Rebuilt on the new client: HTTP/2 or HTTP/1.1 (`--http-version`), `--compare` across both versions, methods, headers and bodies, redirects, body samples, decoding control, TLS summary, timing, proxies.
