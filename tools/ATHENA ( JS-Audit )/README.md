# K~ATHENA — JS Audit
> Athéna, déesse grecque de la sagesse : cet outil analyse le JavaScript front-end à la recherche de secrets exposés et de fonctions risquées.

Audit statique du JavaScript livré au navigateur, empaqueté comme composant KTRX. Il combine plusieurs familles de recherche et restitue des résultats avec gravité, confiance, ressource, ligne, extrait, explication et piste de correction.

## Ce qui est inspecté

- HTML d’entrée, scripts inline et scripts référencés;
- modules JavaScript locaux déclarés par `import`/`require` et source maps accessibles, y compris `sourcesContent`;
- motifs de clés et jetons courants (AWS, GitHub, Stripe, Slack, Google, OpenAI, SendGrid, JWT, blocs de clés privées);
- chaîne à forte entropie proche de noms sensibles comme `token`, `secret`, `password` et `api_key`;
- fonctions et points d’injection risqués tels que `eval`, `new Function`, timer avec chaîne, `document.write`, `innerHTML`, `insertAdjacentHTML`, `dangerouslySetInnerHTML` et `postMessage` avec origine wildcard;
- indices de contrôles de rôles, règles de prix/remise, drapeaux debug/feature, routes d’administration et endpoints API courants.

Les scripts tiers sont recensés dans `external_hosts_referenced`, mais pas téléchargés par défaut. `--external-scripts` permet de les inclure, avec une limite de huit hôtes externes distincts. `--reveal-secrets` désactive le masquage des valeurs trouvées; par défaut seuls quelques caractères aux extrémités restent visibles. L’option ne change pas les requêtes réseau : le code et le rapport ne sont pas envoyés à un service externe par JS Audit.

## Utilisation

```sh
ktrx run 'tools/ATHENA ( JS-Audit )/K~ATHENA.tool' https://example.com --max-requests 80
ktrx run 'tools/ATHENA ( JS-Audit )/K~ATHENA.tool' https://example.com --external-scripts
ktrx run 'tools/ATHENA ( JS-Audit )/K~ATHENA.tool' https://example.com --reveal-secrets
```

KTRX demande l’accord réseau pour l’exécution. La navigation reste limitée aux scripts et source maps découverts depuis la page, sans soumission de formulaire, requête de mutation, charge active, exécution de JavaScript ou tentative d’exploitation. Le budget de requêtes est de 1 à 150; chaque corps est limité à 512 Kio. La progression est affichée pendant l’analyse.

## Lecture des résultats

- `critical` / `high` : motif de secret très spécifique ou sink dangereux à examiner en priorité;
- `medium` : indice sensible plus contextuel, avec davantage de faux positifs possibles;
- `low` : pratique à examiner selon l’usage;
- `info` : surface ou logique visible, sans jugement de vulnérabilité.

La détection ne prouve pas qu’un secret est actif, qu’un sink est exploitable ou que la logique client fait autorité. Les protections d’accès, prix, quotas et règles métier doivent toujours être appliqués côté serveur. Les scripts transformés, assemblés après interaction, fortement obfusqués ou servis après un défi navigateur peuvent ne pas être visibles dans un scan statique.

## Construction

```sh
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/ktrx_js_audit.wasm module.wasm
ktrx pack . --output K~ATHENA.tool
ktrx validate K~ATHENA.tool
```
