# URL Codec

Percent-encodes UTF-8 text as a URL component or decodes `%HH` sequences. Spaces encode as `%20` (not `+`).

```sh
ktrx run tools/url-codec/url-codec.tool --encode 'admin/test?x=1'
ktrx run tools/url-codec/url-codec.tool --decode 'admin%2Ftest%3Fx%3D1'
```
