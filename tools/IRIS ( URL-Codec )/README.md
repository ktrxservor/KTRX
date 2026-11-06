# K~IRIS — URL Codec
> Iris, messagère grecque reliant ciel et terre : cet outil encode et décode les composants textuels d’URL.

Percent-encodes UTF-8 text as a URL component or decodes `%HH` sequences. Spaces encode as `%20` (not `+`).

```sh
ktrx run 'tools/IRIS ( URL-Codec )/K~IRIS.tool' --encode 'admin/test?x=1'
ktrx run 'tools/IRIS ( URL-Codec )/K~IRIS.tool' --decode 'admin%2Ftest%3Fx%3D1'
```
