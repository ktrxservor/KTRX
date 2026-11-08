<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~JANUS — JWT Inspector">
</p>

<p align="center"><sub><code>DOSSIER K~JANUS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~JANUS — JWT Inspector
> Janus, dieu romain aux deux visages : cet outil décode l’en-tête et les claims d’un JWT sans vérifier sa signature.

Decodes the header and payload of a compact JWT locally and reports `exp`, `nbf`, and `iat` timestamps when present. Decoding is **not** signature verification; never treat this output as proof that a token is authentic.

```sh
ktrx run 'tools/JANUS ( JWT-Inspector )/K~JANUS.tool' '<header>.<payload>.<signature>'
```
