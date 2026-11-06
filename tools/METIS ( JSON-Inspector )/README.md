# K~METIS — JSON Inspector
> Métis, déesse grecque de l’intelligence : cet outil valide, met en forme et résume des documents JSON.

Validate and format pasted JSON as readable indented output or compact output. The result also reports the root value type and, for objects, its top-level keys.

```sh
ktrx run 'tools/METIS ( JSON-Inspector )/K~METIS.tool' --pretty '{"ok":true,"items":[1,2]}'
ktrx run 'tools/METIS ( JSON-Inspector )/K~METIS.tool' --compact '{ "ok": true }'
```
