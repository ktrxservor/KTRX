<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~APATE — Webshell Forge">
</p>

<p align="center"><sub><code>DOSSIER K~APATE</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~APATE

APATE — « la déesse de la tromperie » chez les Grecs — forge des webshells pour
PHP, JSP et ASPX. Elle assemble un shell à partir de briques indépendantes
(transport, chiffrement, authentification, armement), randomise chaque
identifiant, puis mesure sa propre exposition contre une liste de motifs
recherchés par les moteurs de fichiers et les WAF.

## Capacités

| action | protocole | effet |
|---|---|---|
| `exec` | `e:COMMANDE` | exécution système, sortie renvoyée |
| `eval` | `x:CODE` | exécution de code arbitraire |
| `file` | `f:r:PATH` / `f:w:PATH:B64` / `f:l:PATH` / `f:d:PATH` | lecture, écriture, listage, suppression |
| `all` | les trois | — |

## Matrice de furtivité

- **Transport** — `post`, `cookie`, `header` (le plus discret : la plupart des WAF
  ne loggent que le corps).
- **Chiffrement** — `none`, `b64`, `gzinflate`, `xor`, `rot13`, `layered`
  (rot13 → xor → base64 → gzinflate).
- **Armement** — `simple`, `advanced` (garde-fous, variante d'exécution tirée au
  hasard), `stealth` (noms reconstruits par `chr()`, superglobales concaténées,
  `hash()` au lieu de `md5()`).
- **Camouflage** — `--comments`, `--decoy`, `--pad`.
- **Authentification** — `none`, `plain`, `md5`, `sha1`. Le shell ne stocke qu'un
  hash sauf en `plain` ; le rapport expose toujours le secret en clair pour le
  client.

## Linter

Chaque exécution renvoie `signatures` et `risk`. C'est un **indicateur
d'exposition relative**, pas un verdict de détection : le seul juge reste un scan
sur la cible.

## Exemples

```bash
ktrx run 'tools/APATE ( Webshell-Forge )/K~APATE.tool' php --token MonSecret
ktrx run 'tools/APATE ( Webshell-Forge )/K~APATE.tool' php --action all --transport header --cipher layered --mode stealth --decoy --minify --output bundle
ktrx run 'tools/APATE ( Webshell-Forge )/K~APATE.tool' jsp --action file --cipher gzinflate --auth sha1 --token MonSecret