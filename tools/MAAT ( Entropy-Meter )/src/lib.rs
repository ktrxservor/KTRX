//! K~MAAT — Entropy-Meter
//!
//! Mesure l'entropie de Shannon d'un texte (ou de ses octets UTF-8) et décrit les familles de
//! caractères qu'il contient. Pur calcul : aucune permission, aucun accès réseau ni fichier.

wit_bindgen::generate!({ path: "wit", world: "ktrx-tool" });

use serde_json::{Value, json};
use std::collections::HashMap;

/// Taille maximale du texte analysé (au-delà, on refuse plutôt que de consommer sans limite).
const MAX_INPUT_BYTES: usize = 1024 * 1024;

struct Tool;

impl exports::ktrx::tool::tool::Guest for Tool {
    fn run(input: String) -> String {
        let result = serde_json::from_str::<Value>(&input)
            .map_err(|e| format!("entrée JSON invalide : {e}"))
            .and_then(|args| execute(&args));
        match result {
            Ok(value) => value.to_string(),
            Err(message) => json!({ "error": message }).to_string(),
        }
    }
}

export!(Tool);

/// Calcule l'entropie du texte reçu.
fn execute(args: &Value) -> Result<Value, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or("argument « text » manquant")?;
    if text.is_empty() {
        return Err("le texte est vide : rien à mesurer".into());
    }
    if text.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "texte trop long ({} octets) : {} octets au maximum",
            text.len(),
            MAX_INPUT_BYTES
        ));
    }
    let by_bytes = args.get("bytes").and_then(Value::as_bool) == Some(true);

    // Les symboles sont les caractères Unicode, ou les octets UTF-8 avec --bytes.
    let symbols: Vec<u32> = if by_bytes {
        text.bytes().map(u32::from).collect()
    } else {
        text.chars().map(u32::from).collect()
    };
    let length = symbols.len();
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for symbol in &symbols {
        *counts.entry(*symbol).or_insert(0) += 1;
    }
    let unique = counts.len();
    let entropy = shannon(&counts, length);
    let max_entropy = (unique as f64).log2();

    Ok(json!({
        "unit": if by_bytes { "bytes" } else { "characters" },
        "length": length,
        "unique_symbols": unique,
        "entropy_bits_per_symbol": round(entropy),
        "max_entropy_bits_per_symbol": round(max_entropy),
        // 1.0 = tous les symboles présents sont équiprobables ; proche de 0 = très répétitif.
        "evenness": if unique > 1 { round(entropy / max_entropy) } else { 0.0 },
        "total_entropy_bits": round(entropy * length as f64),
        "classes": classes(text),
    }))
}

/// H = -Σ p·log2(p), en bits par symbole.
fn shannon(counts: &HashMap<u32, usize>, total: usize) -> f64 {
    counts
        .values()
        .map(|&count| {
            let p = count as f64 / total as f64;
            -p * p.log2()
        })
        .sum()
}

/// Nombre de caractères de chaque famille.
fn classes(text: &str) -> Value {
    let (mut lower, mut upper, mut digits, mut symbols, mut spaces, mut other) = (0, 0, 0, 0, 0, 0);
    for c in text.chars() {
        if c.is_ascii_lowercase() {
            lower += 1;
        } else if c.is_ascii_uppercase() {
            upper += 1;
        } else if c.is_ascii_digit() {
            digits += 1;
        } else if c.is_ascii_punctuation() {
            symbols += 1;
        } else if c.is_whitespace() {
            spaces += 1;
        } else {
            other += 1;
        }
    }
    json!({
        "lowercase": lower,
        "uppercase": upper,
        "digits": digits,
        "symbols": symbols,
        "whitespace": spaces,
        "other": other,
    })
}

/// Arrondi à 4 décimales, pour une sortie lisible et stable.
fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: Value) -> Result<Value, String> {
        execute(&args)
    }

    #[test]
    fn a_repeated_symbol_has_no_entropy() {
        let out = run(json!({ "text": "aaaaaaaa" })).unwrap();
        assert_eq!(out["entropy_bits_per_symbol"], 0.0);
        assert_eq!(out["unique_symbols"], 1);
        assert_eq!(out["length"], 8);
    }

    #[test]
    fn two_equally_likely_symbols_give_one_bit() {
        let out = run(json!({ "text": "abab" })).unwrap();
        assert_eq!(out["entropy_bits_per_symbol"], 1.0);
        assert_eq!(out["evenness"], 1.0);
        assert_eq!(out["total_entropy_bits"], 4.0);
    }

    #[test]
    fn sixteen_distinct_symbols_give_four_bits() {
        let out = run(json!({ "text": "0123456789abcdef" })).unwrap();
        assert_eq!(out["entropy_bits_per_symbol"], 4.0);
        assert_eq!(out["max_entropy_bits_per_symbol"], 4.0);
    }

    #[test]
    fn counts_characters_by_default_and_bytes_on_request() {
        let chars = run(json!({ "text": "é" })).unwrap();
        assert_eq!(chars["unit"], "characters");
        assert_eq!(chars["length"], 1);
        let bytes = run(json!({ "text": "é", "bytes": true })).unwrap();
        assert_eq!(bytes["unit"], "bytes");
        assert_eq!(bytes["length"], 2);
    }

    #[test]
    fn classifies_characters() {
        let out = run(json!({ "text": "aB3! é" })).unwrap();
        let classes = &out["classes"];
        assert_eq!(classes["lowercase"], 1);
        assert_eq!(classes["uppercase"], 1);
        assert_eq!(classes["digits"], 1);
        assert_eq!(classes["symbols"], 1);
        assert_eq!(classes["whitespace"], 1);
        assert_eq!(classes["other"], 1);
    }

    #[test]
    fn rejects_missing_empty_and_oversized_input() {
        assert!(run(json!({})).is_err());
        assert!(run(json!({ "text": "" })).is_err());
        let huge = "a".repeat(MAX_INPUT_BYTES + 1);
        assert!(
            run(json!({ "text": huge }))
                .unwrap_err()
                .contains("trop long")
        );
    }

    #[test]
    fn run_returns_json_errors_instead_of_panicking() {
        use exports::ktrx::tool::tool::Guest;
        let out: Value = serde_json::from_str(&Tool::run("pas du json".into())).unwrap();
        assert!(out["error"].as_str().unwrap().contains("JSON invalide"));
    }
}
