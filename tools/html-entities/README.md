# HTML Entity Codec

Useful for inspecting HTML payloads and escaping text for HTML contexts. Supports `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;` plus decimal and hexadecimal numeric entities. Encoding escapes `& < > " '`.

```sh
ktrx run tools/html-entities/html-entities.tool --encode '<script>alert(1)</script>'
ktrx run tools/html-entities/html-entities.tool --decode '&lt;script&gt;'
```
