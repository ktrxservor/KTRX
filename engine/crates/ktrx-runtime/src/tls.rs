//! TLS options and handshake reporting for `socket.open`.
//!
//! A tool can pick the server name (SNI), ALPN protocols, protocol-version
//! bounds, and cipher lists, and can disable certificate verification to
//! inspect a server whose certificate is expired or self-signed. Whatever the
//! choice, the host reports the negotiated parameters, whether verification
//! succeeded, and the certificate chain the server sent. Interpreting that
//! data (grading ciphers, flagging expiry, …) is the tool's job.

use base64::Engine as _;
use openssl::{
    asn1::{Asn1Time, Asn1TimeRef},
    hash::MessageDigest,
    pkey::Id,
    ssl::{SslConnector, SslMethod, SslRef, SslStream, SslVerifyMode, SslVersion},
    x509::{X509, X509NameRef, X509Ref, X509VerifyResult},
};
use serde_json::{Value, json};
use std::net::TcpStream;

const MAX_ALPN_PROTOCOLS: usize = 16;
const MAX_CIPHER_LIST: usize = 2_048;

#[derive(Clone)]
pub(crate) struct TlsOptions {
    pub(crate) verify: bool,
    pub(crate) sni: Option<String>,
    pub(crate) alpn: Vec<String>,
    pub(crate) min: Option<SslVersion>,
    pub(crate) max: Option<SslVersion>,
    pub(crate) ciphers: Option<String>,
    pub(crate) ciphersuites: Option<String>,
}

impl Default for TlsOptions {
    fn default() -> Self {
        Self {
            verify: true,
            sni: None,
            alpn: Vec::new(),
            min: None,
            max: None,
            ciphers: None,
            ciphersuites: None,
        }
    }
}

impl TlsOptions {
    pub(crate) fn from_request(request: &Value) -> Result<Self, String> {
        let text = |key: &str| -> Result<Option<String>, String> {
            match request.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value)) if value.is_empty() => Ok(None),
                Some(Value::String(value)) => {
                    if value.len() > MAX_CIPHER_LIST || value.contains('\0') {
                        Err(format!("{key} is too long or contains a NUL byte"))
                    } else {
                        Ok(Some(value.clone()))
                    }
                }
                Some(_) => Err(format!("{key} must be a string")),
            }
        };
        let alpn = match request.get("alpn") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => {
                if items.len() > MAX_ALPN_PROTOCOLS {
                    return Err("alpn accepts at most 16 protocols".into());
                }
                items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .filter(|p| !p.is_empty() && p.len() <= 255)
                            .map(str::to_owned)
                            .ok_or_else(|| {
                                "alpn entries must be 1-255 character strings".to_owned()
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            Some(_) => return Err("alpn must be an array of strings".into()),
        };
        let min = text("tls_min")?.map(|v| parse_version(&v)).transpose()?;
        let max = text("tls_max")?.map(|v| parse_version(&v)).transpose()?;
        if let (Some(low), Some(high)) = (min, max) {
            if version_rank(low) > version_rank(high) {
                return Err("tls_min must not be higher than tls_max".into());
            }
        }
        Ok(Self {
            verify: request
                .get("verify")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            sni: text("sni")?,
            alpn,
            min,
            max,
            ciphers: text("ciphers")?,
            ciphersuites: text("ciphersuites")?,
        })
    }

    pub(crate) fn connector(&self) -> Result<SslConnector, String> {
        let mut builder = SslConnector::builder(SslMethod::tls())
            .map_err(|e| format!("TLS setup failed: {e}"))?;
        // Trust roots are loaded even when verification is not enforced, so the
        // reported verification result stays accurate for an inspection run.
        #[cfg(windows)]
        for root in windows_roots()? {
            let _ = builder.cert_store_mut().add_cert(root.clone());
        }
        builder.set_verify(if self.verify {
            SslVerifyMode::PEER
        } else {
            SslVerifyMode::NONE
        });
        builder
            .set_min_proto_version(self.min)
            .map_err(|e| format!("unsupported tls_min: {e}"))?;
        builder
            .set_max_proto_version(self.max)
            .map_err(|e| format!("unsupported tls_max: {e}"))?;
        // OpenSSL 3 refuses TLS 1.0/1.1 and weak ciphers at its default
        // security level. A tool that asks for those versions is auditing a
        // legacy server, so relax the level for that request only.
        let legacy = self
            .min
            .is_some_and(|version| version_rank(version) < version_rank(SslVersion::TLS1_2));
        let list = match (&self.ciphers, legacy) {
            (Some(list), true) => Some(format!("{list}:@SECLEVEL=0")),
            (Some(list), false) => Some(list.clone()),
            (None, true) => Some("ALL:@SECLEVEL=0".to_owned()),
            (None, false) => None,
        };
        if let Some(list) = list {
            builder
                .set_cipher_list(&list)
                .map_err(|e| format!("invalid cipher list: {e}"))?;
        }
        if let Some(suites) = &self.ciphersuites {
            builder
                .set_ciphersuites(suites)
                .map_err(|e| format!("invalid TLS 1.3 cipher suites: {e}"))?;
        }
        if !self.alpn.is_empty() {
            let mut wire = Vec::new();
            for protocol in &self.alpn {
                wire.push(protocol.len() as u8);
                wire.extend_from_slice(protocol.as_bytes());
            }
            builder
                .set_alpn_protos(&wire)
                .map_err(|e| format!("invalid alpn list: {e}"))?;
        }
        Ok(builder.build())
    }

    /// Run the TLS handshake over an established TCP stream.
    pub(crate) fn handshake(
        &self,
        host: &str,
        stream: TcpStream,
    ) -> Result<SslStream<TcpStream>, String> {
        let connector = self.connector()?;
        let mut configuration = connector
            .configure()
            .map_err(|e| format!("TLS setup failed: {e}"))?;
        configuration.set_verify_hostname(self.verify);
        let name = self.sni.as_deref().unwrap_or(host);
        configuration
            .connect(name, stream)
            .map_err(|e| format!("TLS handshake failed: {e}"))
    }
}

fn parse_version(text: &str) -> Result<SslVersion, String> {
    match text
        .trim()
        .trim_start_matches("TLSv")
        .trim_start_matches("TLS")
    {
        "1.0" | "1" => Ok(SslVersion::TLS1),
        "1.1" => Ok(SslVersion::TLS1_1),
        "1.2" => Ok(SslVersion::TLS1_2),
        "1.3" => Ok(SslVersion::TLS1_3),
        other => Err(format!(
            "unsupported TLS version '{other}'; use 1.0, 1.1, 1.2, or 1.3"
        )),
    }
}

fn version_rank(version: SslVersion) -> u8 {
    if version == SslVersion::TLS1 {
        0
    } else if version == SslVersion::TLS1_1 {
        1
    } else if version == SslVersion::TLS1_2 {
        2
    } else {
        3
    }
}

/// Describe a completed handshake.
pub(crate) fn describe(ssl: &SslRef, options: &TlsOptions, host: &str) -> Value {
    let verification = ssl.verify_result();
    let verified = verification == X509VerifyResult::OK;
    let chain: Vec<Value> = match ssl.peer_cert_chain() {
        Some(stack) if !stack.is_empty() => stack.iter().map(certificate).collect(),
        _ => ssl
            .peer_certificate()
            .map(|leaf| vec![certificate(&leaf)])
            .unwrap_or_default(),
    };
    json!({
        "version": ssl.version_str(),
        "cipher": ssl.current_cipher().map(|c| c.name()),
        "cipher_bits": ssl.current_cipher().map(|c| c.bits().secret),
        "alpn": ssl.selected_alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned()),
        "sni": options.sni.as_deref().unwrap_or(host),
        "verification_enforced": options.verify,
        "verified": verified,
        "verify_error": (!verified).then(|| verification.error_string().to_owned()),
        "peer_certificates": chain,
    })
}

fn certificate(cert: &X509Ref) -> Value {
    let der = cert.to_der().unwrap_or_default();
    let public_key = cert.public_key().ok();
    let key_json = public_key.as_ref().map(|key| {
        let kind = match key.id() {
            Id::RSA => "RSA",
            Id::EC => "EC",
            Id::DSA => "DSA",
            Id::DH => "DH",
            Id::ED25519 => "Ed25519",
            Id::ED448 => "Ed448",
            _ => "other",
        };
        let curve = key
            .ec_key()
            .ok()
            .and_then(|ec| ec.group().curve_name())
            .and_then(|nid| nid.short_name().ok());
        json!({"type": kind, "bits": key.bits(), "curve": curve})
    });
    json!({
        "subject": name_string(cert.subject_name()),
        "issuer": name_string(cert.issuer_name()),
        "self_issued": cert.issued(cert) == X509VerifyResult::OK,
        "serial": cert
            .serial_number()
            .to_bn()
            .ok()
            .and_then(|bn| bn.to_hex_str().ok())
            .map(|hex| hex.to_ascii_lowercase()),
        "not_before": cert.not_before().to_string(),
        "not_before_unix": unix_seconds(cert.not_before()),
        "not_after": cert.not_after().to_string(),
        "not_after_unix": unix_seconds(cert.not_after()),
        "subject_alt_names": alt_names(cert),
        "signature_algorithm": cert.signature_algorithm().object().nid().long_name().ok(),
        "public_key": key_json,
        "sha256": cert
            .digest(MessageDigest::sha256())
            .map(|digest| digest.iter().map(|b| format!("{b:02x}")).collect::<String>())
            .ok(),
        "der_base64": base64::engine::general_purpose::STANDARD.encode(der),
    })
}

fn name_string(name: &X509NameRef) -> String {
    name.entries()
        .map(|entry| {
            let key = entry.object().nid().short_name().unwrap_or("?");
            let value = String::from_utf8_lossy(entry.data().as_slice()).into_owned();
            format!("{key}={value}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn alt_names(cert: &X509Ref) -> Vec<String> {
    let Some(names) = cert.subject_alt_names() else {
        return Vec::new();
    };
    names
        .iter()
        .filter_map(|name| {
            if let Some(dns) = name.dnsname() {
                Some(format!("DNS:{dns}"))
            } else if let Some(bytes) = name.ipaddress() {
                match bytes.len() {
                    4 => Some(format!(
                        "IP:{}",
                        std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3])
                    )),
                    16 => {
                        let mut octets = [0u8; 16];
                        octets.copy_from_slice(bytes);
                        Some(format!("IP:{}", std::net::Ipv6Addr::from(octets)))
                    }
                    _ => None,
                }
            } else if let Some(email) = name.email() {
                Some(format!("email:{email}"))
            } else {
                name.uri().map(|uri| format!("URI:{uri}"))
            }
        })
        .collect()
}

fn unix_seconds(time: &Asn1TimeRef) -> Option<i64> {
    let epoch = Asn1Time::from_unix(0).ok()?;
    let diff = epoch.diff(time).ok()?;
    Some(i64::from(diff.days) * 86_400 + i64::from(diff.secs))
}

/// Root certificates installed on this Windows machine, loaded once.
///
/// The vendored OpenSSL used on Windows starts with an empty trust store, so
/// the machine's trusted roots must be added explicitly. Either system store
/// may be unavailable (opening the local-machine store can be denied to an
/// unprivileged process), so a store that cannot be opened is skipped; the
/// current-user "Root" store normally carries the public trust anchors. The two
/// stores overlap, so certificates are deduplicated by DER encoding.
#[cfg(windows)]
fn windows_roots() -> Result<&'static [X509], String> {
    use schannel::cert_store::CertStore;
    use std::{collections::HashSet, sync::OnceLock};
    static ROOTS: OnceLock<Vec<X509>> = OnceLock::new();
    let roots = ROOTS.get_or_init(|| {
        let mut seen = HashSet::new();
        let mut roots = Vec::new();
        for store in [
            CertStore::open_current_user("Root"),
            CertStore::open_local_machine("Root"),
        ]
        .into_iter()
        .flatten()
        {
            for certificate in store.certs() {
                let der = certificate.to_der();
                if !seen.insert(der.to_vec()) {
                    continue;
                }
                if let Ok(parsed) = X509::from_der(der) {
                    roots.push(parsed);
                }
            }
        }
        roots
    });
    if roots.is_empty() {
        Err("TLS setup failed: no Windows root certificates could be loaded".to_owned())
    } else {
        Ok(roots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tls_options_from_a_request() {
        let options = TlsOptions::from_request(&json!({
            "verify": false,
            "sni": "example.test",
            "alpn": ["h2", "http/1.1"],
            "tls_min": "1.2",
            "tls_max": "TLSv1.3",
            "ciphers": "ECDHE-RSA-AES128-GCM-SHA256",
        }))
        .unwrap();
        assert!(!options.verify);
        assert_eq!(options.sni.as_deref(), Some("example.test"));
        assert_eq!(options.alpn, ["h2", "http/1.1"]);
        assert!(options.min == Some(SslVersion::TLS1_2));
        assert!(options.max == Some(SslVersion::TLS1_3));
        let defaults = TlsOptions::from_request(&json!({})).unwrap();
        assert!(defaults.verify && defaults.alpn.is_empty() && defaults.min.is_none());
    }

    #[test]
    fn rejects_invalid_tls_options() {
        for bad in [
            json!({"tls_min": "9.9"}),
            json!({"tls_min": "1.3", "tls_max": "1.2"}),
            json!({"alpn": "h2"}),
            json!({"alpn": [""]}),
            json!({"sni": 7}),
            json!({"ciphers": "A\u{0}B"}),
        ] {
            assert!(TlsOptions::from_request(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn builds_connectors_for_each_option_set() {
        TlsOptions::default()
            .connector()
            .expect("verifying connector");
        TlsOptions {
            verify: false,
            alpn: vec!["h2".into()],
            ..TlsOptions::default()
        }
        .connector()
        .expect("non-verifying connector with ALPN");
        TlsOptions {
            min: Some(SslVersion::TLS1),
            ciphers: Some("AES128-SHA".into()),
            ..TlsOptions::default()
        }
        .connector()
        .expect("legacy connector");
        assert!(
            TlsOptions {
                ciphers: Some("NOT-A-CIPHER".into()),
                ..TlsOptions::default()
            }
            .connector()
            .is_err()
        );
    }

    #[test]
    fn describes_a_certificate_in_detail() {
        use openssl::{
            asn1::Asn1Time,
            bn::BigNum,
            hash::MessageDigest,
            pkey::PKey,
            rsa::Rsa,
            x509::{X509Builder, X509NameBuilder, extension::SubjectAlternativeName},
        };
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "tls.test").unwrap();
        let name = name.build();
        let mut builder = X509Builder::new().unwrap();
        builder.set_version(2).unwrap();
        builder
            .set_serial_number(&BigNum::from_u32(0x1234).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        builder.set_subject_name(&name).unwrap();
        builder.set_issuer_name(&name).unwrap();
        builder.set_pubkey(&key).unwrap();
        builder
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        builder
            .set_not_after(&Asn1Time::days_from_now(30).unwrap())
            .unwrap();
        let san = SubjectAlternativeName::new()
            .dns("tls.test")
            .ip("192.0.2.7")
            .build(&builder.x509v3_context(None, None))
            .unwrap();
        builder.append_extension(san).unwrap();
        builder.sign(&key, MessageDigest::sha256()).unwrap();
        let cert = builder.build();
        let described = certificate(&cert);
        assert_eq!(described["subject"], "CN=tls.test");
        assert_eq!(described["issuer"], "CN=tls.test");
        assert_eq!(described["self_issued"], true);
        assert_eq!(described["serial"], "1234");
        assert_eq!(described["public_key"]["type"], "RSA");
        assert_eq!(described["public_key"]["bits"], 2048);
        assert_eq!(
            described["subject_alt_names"],
            json!(["DNS:tls.test", "IP:192.0.2.7"])
        );
        let begin = described["not_before_unix"].as_i64().unwrap();
        let end = described["not_after_unix"].as_i64().unwrap();
        assert_eq!(end - begin, 30 * 86_400);
        assert_eq!(described["sha256"].as_str().unwrap().len(), 64);
        let der = base64::engine::general_purpose::STANDARD
            .decode(described["der_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(X509::from_der(&der).unwrap().to_der().unwrap(), der);
    }
}
