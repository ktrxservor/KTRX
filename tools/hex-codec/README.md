# Hex Codec

Convert pasted request fragments, payloads, or text to and from hexadecimal. Decoding accepts optional spaces and `0x` prefix and requires valid UTF-8 output.

```sh
ktrx run tools/hex-codec/hex-codec.tool --encode 'GET / HTTP/1.1'
ktrx run tools/hex-codec/hex-codec.tool --decode '474554202f'
```
