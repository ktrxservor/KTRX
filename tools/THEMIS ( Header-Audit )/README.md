<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~THEMIS — Header Audit">
</p>

<p align="center"><sub><code>DOSSIER K~THEMIS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~THEMIS — Header Audit
> Thémis, figure grecque de la loi et de l’ordre : cet outil examine les en-têtes HTTP de sécurité et les attributs des cookies.

Paste an HTTP response status line and headers to check common browser security headers and `Set-Cookie` attributes. It works offline and reports observations only; missing headers are context-dependent and are not automatically vulnerabilities.

```sh
ktrx run 'tools/THEMIS ( Header-Audit )/K~THEMIS.tool' --headers $'HTTP/1.1 200 OK\nServer: nginx\nSet-Cookie: sid=abc'
```
