# K~HEPHAESTUS — HTML Entities
> Héphaïstos, artisan des dieux grecs : cet outil encode et décode les entités HTML courantes.

Useful for inspecting HTML payloads and escaping text for HTML contexts. Supports `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;` plus decimal and hexadecimal numeric entities. Encoding escapes `& < > " '`.

```sh
ktrx run 'tools/HEPHAESTUS ( HTML-Entities )/K~HEPHAESTUS.tool' --encode '<script>alert(1)</script>'
ktrx run 'tools/HEPHAESTUS ( HTML-Entities )/K~HEPHAESTUS.tool' --decode '&lt;script&gt;'
```
