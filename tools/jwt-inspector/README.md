# JWT Inspector

Decodes the header and payload of a compact JWT locally and reports `exp`, `nbf`, and `iat` timestamps when present. Decoding is **not** signature verification; never treat this output as proof that a token is authentic.

```sh
ktrx run tools/jwt-inspector/jwt-inspector.tool '<header>.<payload>.<signature>'
```
