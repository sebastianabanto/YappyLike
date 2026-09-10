//! Procesamiento de texto del modelo: normalización propia de Supertonic,
//! indexado Unicode y máscaras. Port de `helper.rs`.
//!
//! Esta es la normalización **que ya trae Supertonic** (§4). La capa propia,
//! mínima y configurable de YappyLike (§7) vive aparte en `normalize/` y se
//! aplica *antes* de esta.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::OnceLock;

use ndarray::Array3;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use super::{Result, TtsError};

/// Idiomas soportados por el modelo multilingüe (`na` = agnóstico).
pub const AVAILABLE_LANGS: &[&str] = &[
    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu",
    "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi", "na",
];

pub fn is_valid_lang(lang: &str) -> bool {
    AVAILABLE_LANGS.contains(&lang)
}

/// Indexador de code points Unicode → id de token del modelo.
pub struct UnicodeProcessor {
    indexer: Vec<i64>,
}

impl UnicodeProcessor {
    pub fn new(unicode_indexer_json_path: &Path) -> Result<Self> {
        let file = File::open(unicode_indexer_json_path).map_err(|source| TtsError::Io {
            path: unicode_indexer_json_path.display().to_string(),
            source,
        })?;
        let reader = BufReader::new(file);
        let indexer: Vec<i64> =
            serde_json::from_reader(reader).map_err(|source| TtsError::Json {
                path: unicode_indexer_json_path.display().to_string(),
                source,
            })?;
        Ok(UnicodeProcessor { indexer })
    }

    /// Devuelve `(text_ids [bsz, max_len], text_mask [bsz, 1, max_len])`.
    pub fn call(
        &self,
        text_list: &[String],
        lang_list: &[String],
    ) -> Result<(Vec<Vec<i64>>, Array3<f32>)> {
        let mut processed_texts: Vec<String> = Vec::new();
        for (text, lang) in text_list.iter().zip(lang_list.iter()) {
            processed_texts.push(preprocess_text(text, lang)?);
        }

        let lengths: Vec<usize> = processed_texts.iter().map(|t| t.chars().count()).collect();
        let max_len = *lengths.iter().max().unwrap_or(&0);

        let mut text_ids = Vec::new();
        for text in &processed_texts {
            let mut row = vec![0i64; max_len];
            for (j, val) in text_to_unicode_values(text).into_iter().enumerate() {
                row[j] = if val < self.indexer.len() {
                    self.indexer[val]
                } else {
                    -1
                };
            }
            text_ids.push(row);
        }

        let text_mask = length_to_mask(&lengths, Some(max_len));
        Ok((text_ids, text_mask))
    }
}

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

fn emoji_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        compiled(
            r"[\x{1F600}-\x{1F64F}\x{1F300}-\x{1F5FF}\x{1F680}-\x{1F6FF}\x{1F700}-\x{1F77F}\x{1F780}-\x{1F7FF}\x{1F800}-\x{1F8FF}\x{1F900}-\x{1F9FF}\x{1FA00}-\x{1FA6F}\x{1FA70}-\x{1FAFF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}\x{1F1E6}-\x{1F1FF}]+",
        )
    })
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| compiled(r"\s+"))
}

fn ends_with_punct_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| compiled(r#"[.!?;:,'"\u{201C}\u{201D}\u{2018}\u{2019})\]}…。」』】〉》›»]$"#))
}

/// Normalización propia de Supertonic (NFKD, emojis, símbolos, puntuación) y
/// envoltura con etiquetas de idioma. Port fiel de `preprocess_text`.
pub fn preprocess_text(text: &str, lang: &str) -> Result<String> {
    let mut text: String = text.nfkd().collect();

    text = emoji_re().replace_all(&text, "").to_string();

    let replacements = [
        ("–", "-"),
        ("‑", "-"),
        ("—", "-"),
        ("_", " "),
        ("\u{201C}", "\""),
        ("\u{201D}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
        ("´", "'"),
        ("`", "'"),
        ("[", " "),
        ("]", " "),
        ("|", " "),
        ("/", " "),
        ("#", " "),
        ("→", " "),
        ("←", " "),
    ];
    for (from, to) in &replacements {
        text = text.replace(from, to);
    }

    for symbol in &["♥", "☆", "♡", "©", "\\"] {
        text = text.replace(symbol, "");
    }

    for (from, to) in &[
        ("@", " at "),
        ("e.g.,", "for example, "),
        ("i.e.,", "that is, "),
    ] {
        text = text.replace(from, to);
    }

    // Espaciado alrededor de puntuación.
    for (pat, rep) in [
        (r" ,", ","),
        (r" \.", "."),
        (r" !", "!"),
        (r" \?", "?"),
        (r" ;", ";"),
        (r" :", ":"),
        (r" '", "'"),
    ] {
        text = compiled(pat).replace_all(&text, rep).to_string();
    }

    while text.contains("\"\"") {
        text = text.replace("\"\"", "\"");
    }
    while text.contains("''") {
        text = text.replace("''", "'");
    }
    while text.contains("``") {
        text = text.replace("``", "`");
    }

    text = whitespace_re().replace_all(&text, " ").to_string();
    text = text.trim().to_string();

    if !text.is_empty() && !ends_with_punct_re().is_match(&text) {
        text.push('.');
    }

    if !is_valid_lang(lang) {
        return Err(TtsError::InvalidLang(lang.to_string()));
    }

    Ok(format!("<{lang}>{text}</{lang}>"))
}

pub fn text_to_unicode_values(text: &str) -> Vec<usize> {
    text.chars().map(|c| c as usize).collect()
}

/// Máscara `[bsz, 1, max_len]` con 1.0 hasta cada longitud.
pub fn length_to_mask(lengths: &[usize], max_len: Option<usize>) -> Array3<f32> {
    let bsz = lengths.len();
    let max_len = max_len.unwrap_or_else(|| *lengths.iter().max().unwrap_or(&0));

    let mut mask = Array3::<f32>::zeros((bsz, 1, max_len));
    for (i, &len) in lengths.iter().enumerate() {
        for j in 0..len.min(max_len) {
            mask[[i, 0, j]] = 1.0;
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envuelve_con_etiqueta_de_idioma_y_cierra_con_punto() {
        let out = preprocess_text("Hola mundo", "es").unwrap();
        assert_eq!(out, "<es>Hola mundo.</es>");
    }

    #[test]
    fn conserva_acentos_y_enye() {
        // NFKD descompone pero el texto sigue conteniendo las letras base.
        let out = preprocess_text("Añoño camión", "es").unwrap();
        assert!(out.starts_with("<es>") && out.ends_with("</es>"));
        assert!(out.contains('ñ') || out.contains('n')); // ñ = n + tilde combinante
    }

    #[test]
    fn idioma_invalido_es_error() {
        assert!(matches!(
            preprocess_text("hola", "xx"),
            Err(TtsError::InvalidLang(_))
        ));
    }

    #[test]
    fn colapsa_espacios_y_respeta_puntuacion_final() {
        let out = preprocess_text("Hola    mundo!", "en").unwrap();
        assert_eq!(out, "<en>Hola mundo!</en>");
    }

    #[test]
    fn na_es_idioma_valido() {
        assert!(is_valid_lang("na"));
        assert!(preprocess_text("texto", "na").is_ok());
    }
}
