# Regex Lab

Search text with Rust's regular-expression syntax and inspect match positions and capture groups. Rust regex guarantees linear-time matching and does not support look-around or backreferences.

```sh
ktrx run tools/regex-lab/regex-lab.tool 'user=(\\w+)' --text 'user=alice'
ktrx run tools/regex-lab/regex-lab.tool '(?i)token' --text 'Token: abc'
```
