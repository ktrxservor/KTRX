<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~IRIS — URL Codec">
</p>

<p align="center"><sub><code>DOSSIER K~IRIS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~IRIS — URL Codec
> Iris, messagère grecque reliant ciel et terre : cet outil encode et décode les composants textuels d’URL.

Percent-encodes UTF-8 text as a URL component or decodes `%HH` sequences. Spaces encode as `%20` (not `+`).

```sh
ktrx run 'tools/IRIS ( URL-Codec )/K~IRIS.tool' --encode 'admin/test?x=1'
ktrx run 'tools/IRIS ( URL-Codec )/K~IRIS.tool' --decode 'admin%2Ftest%3Fx%3D1'
```
