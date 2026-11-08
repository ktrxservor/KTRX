<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~HEPHAESTUS — HTML Entities">
</p>

<p align="center"><sub><code>DOSSIER K~HEPHAESTUS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~HEPHAESTUS — HTML Entities
> Héphaïstos, artisan des dieux grecs : cet outil encode et décode les entités HTML courantes.

Useful for inspecting HTML payloads and escaping text for HTML contexts. Supports `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;` plus decimal and hexadecimal numeric entities. Encoding escapes `& < > " '`.

```sh
ktrx run 'tools/HEPHAESTUS ( HTML-Entities )/K~HEPHAESTUS.tool' --encode '<script>alert(1)</script>'
ktrx run 'tools/HEPHAESTUS ( HTML-Entities )/K~HEPHAESTUS.tool' --decode '&lt;script&gt;'
```
