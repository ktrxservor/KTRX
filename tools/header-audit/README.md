# HTTP Header Audit

Paste an HTTP response status line and headers to check common browser security headers and `Set-Cookie` attributes. It works offline and reports observations only; missing headers are context-dependent and are not automatically vulnerabilities.

```sh
ktrx run tools/header-audit/header-audit.tool --headers $'HTTP/1.1 200 OK\nServer: nginx\nSet-Cookie: sid=abc'
```
