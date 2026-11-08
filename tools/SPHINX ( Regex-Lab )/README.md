<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~SPHINX — Regex Lab">
</p>

<p align="center"><sub><code>DOSSIER K~SPHINX</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~SPHINX — Regex Lab
> Le Sphinx, gardien mythique des énigmes : cet outil teste des expressions régulières et affiche leurs correspondances et captures.

Search text with Rust's regular-expression syntax and inspect match positions and capture groups. Rust regex guarantees linear-time matching and does not support look-around or backreferences.

```sh
ktrx run 'tools/SPHINX ( Regex-Lab )/K~SPHINX.tool' 'user=(\\w+)' --text 'user=alice'
ktrx run 'tools/SPHINX ( Regex-Lab )/K~SPHINX.tool' '(?i)token' --text 'Token: abc'
```
