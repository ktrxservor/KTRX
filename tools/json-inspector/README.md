# JSON Inspector

Validate and format pasted JSON as readable indented output or compact output. The result also reports the root value type and, for objects, its top-level keys.

```sh
ktrx run tools/json-inspector/json-inspector.tool --pretty '{"ok":true,"items":[1,2]}'
ktrx run tools/json-inspector/json-inspector.tool --compact '{ "ok": true }'
```
