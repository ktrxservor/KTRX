//! Linter de signatures : confronte le code produit à une liste de motifs
//! classiquement recherchés par les moteurs de fichiers et les WAF.

pub struct Sig {
    pub id: &'static str,
    pub severity: &'static str,
    pub needle: &'static str,
    pub hint: &'static str,
}

pub const SIGNATURES: &[Sig] = &[
    Sig {
        id: "php.open",
        severity: "low",
        needle: "<?php",
        hint: "impossible à retirer ; reste couvert par .php/.phtml/.phar",
    },
    Sig {
        id: "php.eval",
        severity: "high",
        needle: "@eval(",
        hint: "action eval : remplacer par exec si possible",
    },
    Sig {
        id: "php.eval.bare",
        severity: "medium",
        needle: "eval(",
        hint: "action eval",
    },
    Sig {
        id: "php.system",
        severity: "medium",
        needle: "system(",
        hint: "mode stealth reconstruit le nom via chr()",
    },
    Sig {
        id: "php.passthru",
        severity: "medium",
        needle: "passthru(",
        hint: "mode stealth",
    },
    Sig {
        id: "php.shell_exec",
        severity: "high",
        needle: "shell_exec(",
        hint: "préférer system ou passthru",
    },
    Sig {
        id: "php.popen",
        severity: "medium",
        needle: "popen(",
        hint: "variante d'exécution tirée au hasard",
    },
    Sig {
        id: "php.exec",
        severity: "medium",
        needle: "@exec(",
        hint: "variante d'exécution",
    },
    Sig {
        id: "php.b64decode",
        severity: "high",
        needle: "base64_decode(",
        hint: "cipher gzinflate ou layered",
    },
    Sig {
        id: "php.gzinflate",
        severity: "high",
        needle: "gzinflate(",
        hint: "cipher xor ou layered",
    },
    Sig {
        id: "php.rot13",
        severity: "medium",
        needle: "str_rot13(",
        hint: "cipher layered",
    },
    Sig {
        id: "php.post",
        severity: "high",
        needle: "$_POST[",
        hint: "transport header ou cookie",
    },
    Sig {
        id: "php.request",
        severity: "high",
        needle: "$_REQUEST[",
        hint: "aucun : $_REQUEST n'est pas utilisé",
    },
    Sig {
        id: "php.cookie",
        severity: "medium",
        needle: "$_COOKIE[",
        hint: "transport post ou header",
    },
    Sig {
        id: "php.globals",
        severity: "low",
        needle: "$GLOBALS[",
        hint: "mode stealth : superglobales concaténées",
    },
    Sig {
        id: "php.getallheaders",
        severity: "medium",
        needle: "getallheaders(",
        hint: "transport post ou cookie",
    },
    Sig {
        id: "php.md5",
        severity: "medium",
        needle: "md5(",
        hint: "mode stealth utilise hash('md5',...)",
    },
    Sig {
        id: "php.scandir",
        severity: "medium",
        needle: "scandir(",
        hint: "action exec seule",
    },
    Sig {
        id: "php.fileget",
        severity: "medium",
        needle: "file_get_contents(",
        hint: "action exec seule",
    },
    Sig {
        id: "php.fileput",
        severity: "medium",
        needle: "file_put_contents(",
        hint: "action exec seule",
    },
    Sig {
        id: "jsp.directive",
        severity: "low",
        needle: "<%@ page",
        hint: "incontournable en JSP",
    },
    Sig {
        id: "jsp.getparam",
        severity: "high",
        needle: "getParameter(",
        hint: "transport header via getHeader",
    },
    Sig {
        id: "jsp.runtime",
        severity: "high",
        needle: "getRuntime",
        hint: "mode stealth passe par la réflexion",
    },
    Sig {
        id: "jsp.classforname",
        severity: "high",
        needle: "Class.forName(",
        hint: "mode stealth",
    },
    Sig {
        id: "jsp.process",
        severity: "medium",
        needle: "Process ",
        hint: "mode stealth",
    },
    Sig {
        id: "jsp.compiler",
        severity: "high",
        needle: "getSystemJavaCompiler",
        hint: "action eval exige un JDK",
    },
    Sig {
        id: "jsp.b64",
        severity: "high",
        needle: "Base64.getDecoder",
        hint: "cipher gzinflate ou layered",
    },
    Sig {
        id: "aspx.directive",
        severity: "low",
        needle: "<%@ Page",
        hint: "incontournable en ASPX",
    },
    Sig {
        id: "aspx.request",
        severity: "high",
        needle: "Request.Params",
        hint: "transport header via Request.Headers",
    },
    Sig {
        id: "aspx.process",
        severity: "high",
        needle: "ProcessStartInfo",
        hint: "mode stealth passe par la réflexion",
    },
    Sig {
        id: "aspx.codecompiler",
        severity: "high",
        needle: "CSharpCodeProvider",
        hint: "action eval uniquement",
    },
    Sig {
        id: "aspx.fromb64",
        severity: "medium",
        needle: "FromBase64String(",
        hint: "cipher gzinflate ou layered",
    },
    Sig {
        id: "generic.cmd",
        severity: "medium",
        needle: "cmd.exe",
        hint: "indicateur plateforme Windows",
    },
    Sig {
        id: "generic.sh",
        severity: "medium",
        needle: "/bin/sh",
        hint: "indicateur plateforme Unix",
    },
];

pub fn scan(code: &str) -> Vec<(String, String, String)> {
    let mut hits = Vec::new();
    for s in SIGNATURES {
        if code.contains(s.needle) {
            hits.push((s.id.to_string(), s.severity.to_string(), s.hint.to_string()));
        }
    }
    hits
}

pub fn risk(hits: &[(String, String, String)]) -> &'static str {
    let mut score = 0u32;
    for (_, sev, _) in hits {
        score += match sev.as_str() {
            "high" => 3,
            "medium" => 2,
            _ => 1,
        };
    }
    if score >= 12 {
        "high"
    } else if score >= 6 {
        "medium"
    } else {
        "low"
    }
}
