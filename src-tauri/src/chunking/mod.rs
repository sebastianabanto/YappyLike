//! Segmentación de texto en fragmentos (§6, base para el streaming de M5).
//!
//! Port directo de `chunk_text` / `split_sentences` del ejemplo oficial de
//! Supertonic (`rust/src/helper.rs`): divide por párrafos, luego por frases
//! (respetando abreviaturas), y si una frase excede el máximo, por comas y por
//! último por palabras. El objetivo es ~200–300 caracteres por fragmento sin
//! partir oraciones cuando se puede.

use regex::Regex;
use std::sync::OnceLock;

/// Máximo de caracteres por fragmento por defecto (idiomas latinos).
pub const MAX_CHUNK_LENGTH: usize = 300;

/// Máximo de caracteres por fragmento según el idioma. Coreano y japonés usan
/// un tope menor (mismo criterio que el ejemplo oficial de Supertonic).
pub fn max_len_for_lang(lang: &str) -> usize {
    if lang == "ko" || lang == "ja" {
        120
    } else {
        MAX_CHUNK_LENGTH
    }
}

/// Abreviaturas que **no** deben tratarse como fin de frase.
const ABBREVIATIONS: &[&str] = &[
    "Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St.", "Ave.", "Rd.", "Blvd.", "Dept.",
    "Inc.", "Ltd.", "Co.", "Corp.", "etc.", "vs.", "i.e.", "e.g.", "Ph.D.",
];

fn paragraph_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\n\s*\n").unwrap())
}

fn sentence_boundary_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([.!?])\s+").unwrap())
}

/// Segmenta `text` en fragmentos de a lo sumo `max_len` caracteres (por
/// defecto [`MAX_CHUNK_LENGTH`]). Nunca devuelve un vector vacío.
pub fn chunk_text(text: &str, max_len: Option<usize>) -> Vec<String> {
    let max_len = max_len.unwrap_or(MAX_CHUNK_LENGTH);
    let text = text.trim();

    if text.is_empty() {
        return vec![String::new()];
    }

    let paragraphs: Vec<&str> = paragraph_re().split(text).collect();
    let mut chunks = Vec::new();

    for para in paragraphs {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }

        if para.len() <= max_len {
            chunks.push(para.to_string());
            continue;
        }

        let sentences = split_sentences(para);
        let mut current = String::new();
        let mut current_len = 0;

        for sentence in sentences {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }

            let sentence_len = sentence.len();
            if sentence_len > max_len {
                // Frase más larga que el máximo: cerrar lo acumulado y partir
                // por comas, y como último recurso por palabras.
                if !current.is_empty() {
                    chunks.push(current.trim().to_string());
                    current.clear();
                    current_len = 0;
                }

                let parts: Vec<&str> = sentence.split(',').collect();
                for part in parts {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }

                    let part_len = part.len();
                    if part_len > max_len {
                        let words: Vec<&str> = part.split_whitespace().collect();
                        let mut word_chunk = String::new();
                        let mut word_chunk_len = 0;

                        for word in words {
                            let word_len = word.len();
                            if word_chunk_len + word_len + 1 > max_len && !word_chunk.is_empty() {
                                chunks.push(word_chunk.trim().to_string());
                                word_chunk.clear();
                                word_chunk_len = 0;
                            }

                            if !word_chunk.is_empty() {
                                word_chunk.push(' ');
                                word_chunk_len += 1;
                            }
                            word_chunk.push_str(word);
                            word_chunk_len += word_len;
                        }

                        if !word_chunk.is_empty() {
                            chunks.push(word_chunk.trim().to_string());
                        }
                    } else {
                        if current_len + part_len + 1 > max_len && !current.is_empty() {
                            chunks.push(current.trim().to_string());
                            current.clear();
                            current_len = 0;
                        }

                        if !current.is_empty() {
                            current.push_str(", ");
                            current_len += 2;
                        }
                        current.push_str(part);
                        current_len += part_len;
                    }
                }
                continue;
            }

            if current_len + sentence_len + 1 > max_len && !current.is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
                current_len = 0;
            }

            if !current.is_empty() {
                current.push(' ');
                current_len += 1;
            }
            current.push_str(sentence);
            current_len += sentence_len;
        }

        if !current.is_empty() {
            chunks.push(current.trim().to_string());
        }
    }

    if chunks.is_empty() {
        vec![String::new()]
    } else {
        chunks
    }
}

/// Segmentación para **streaming**: **una oración por fragmento** (no se agrupan
/// varias como en [`chunk_text`]).
///
/// Con esto el primer audio suena en cuanto se sintetiza la primera oración
/// (baja latencia) y, al ser los fragmentos de tamaño parecido, la síntesis del
/// siguiente termina antes de que acabe de sonar el actual (el modelo va a ~0.4×
/// tiempo real en CPU), así que la cola no se queda sin audio entre frases. Un
/// bloque de ~300 caracteres tardaría varios segundos en sonar por primera vez;
/// por oración, arranca en ~1 s.
///
/// Las oraciones que superan `max_len` (raras: sin puntuación) se trocean con
/// [`chunk_text`] (comas y, en último caso, palabras). Nunca parte a mitad de
/// una oración normal.
pub fn chunk_text_streaming(text: &str, max_len: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut out = Vec::new();
    for para in paragraph_re().split(text) {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        for sentence in split_sentences(para) {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }
            if sentence.len() <= max_len {
                out.push(sentence.to_string());
            } else {
                // Oración larguísima (sin puntuación): trocea por comas/palabras.
                out.extend(chunk_text(sentence, Some(max_len)));
            }
        }
    }

    if out.is_empty() {
        vec![String::new()]
    } else {
        out
    }
}

/// Divide en frases por `.`/`!`/`?` seguidos de espacio, respetando
/// abreviaturas. La regex de Rust no soporta lookbehind, así que se comprueba
/// a mano si el corte cae tras una abreviatura conocida.
fn split_sentences(text: &str) -> Vec<String> {
    let re = sentence_boundary_re();

    let matches: Vec<_> = re.find_iter(text).collect();
    if matches.is_empty() {
        return vec![text.to_string()];
    }

    let mut sentences = Vec::new();
    let mut last_end = 0;

    for m in matches {
        let before_punc = &text[last_end..m.start()];

        let mut is_abbrev = false;
        for abbrev in ABBREVIATIONS {
            let combined = format!("{}{}", before_punc.trim(), &text[m.start()..m.start() + 1]);
            if combined.ends_with(abbrev) {
                is_abbrev = true;
                break;
            }
        }

        if !is_abbrev {
            sentences.push(text[last_end..m.end()].to_string());
            last_end = m.end();
        }
    }

    if last_end < text.len() {
        sentences.push(text[last_end..].to_string());
    }

    if sentences.is_empty() {
        vec![text.to_string()]
    } else {
        sentences
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_corto_es_un_solo_fragmento() {
        let chunks = chunk_text("Hola mundo.", None);
        assert_eq!(chunks, vec!["Hola mundo.".to_string()]);
    }

    #[test]
    fn texto_vacio_devuelve_un_fragmento_vacio() {
        assert_eq!(chunk_text("   ", None), vec![String::new()]);
    }

    #[test]
    fn agrupa_varias_frases_bajo_el_maximo() {
        // Con un máximo pequeño, cada frase cae en su propio fragmento.
        let text = "Uno dos tres. Cuatro cinco seis. Siete ocho nueve.";
        let chunks = chunk_text(text, Some(20));
        assert!(chunks.len() >= 2, "debe partir en varias: {chunks:?}");
        for c in &chunks {
            assert!(!c.is_empty());
        }
    }

    #[test]
    fn no_corta_en_abreviaturas() {
        // "Dr." no debe terminar la frase; queda todo junto (cabe en el máximo).
        let text = "El Dr. Pérez llegó tarde.";
        let chunks = chunk_text(text, None);
        assert_eq!(chunks, vec!["El Dr. Pérez llegó tarde.".to_string()]);
    }

    #[test]
    fn frase_larguisima_sin_puntuacion_se_parte_por_palabras() {
        let long = "palabra ".repeat(100); // ~700 bytes, sin puntuación
        let chunks = chunk_text(&long, Some(50));
        assert!(chunks.len() > 1, "debe partir por palabras");
        for c in &chunks {
            assert!(c.len() <= 50, "cada fragmento respeta el máximo: {:?}", c);
        }
    }

    #[test]
    fn streaming_una_oracion_por_fragmento() {
        // Tres oraciones cortas caben en un solo fragmento normal; en streaming
        // cada oración va en su propio fragmento (arranque rápido, sin huecos).
        let text = "Uno dos tres. Cuatro cinco seis. Siete ocho nueve.";
        assert_eq!(chunk_text(text, Some(300)).len(), 1, "normal: un fragmento");

        let stream = chunk_text_streaming(text, 300);
        assert_eq!(
            stream,
            vec![
                "Uno dos tres.".to_string(),
                "Cuatro cinco seis.".to_string(),
                "Siete ocho nueve.".to_string(),
            ]
        );
    }

    #[test]
    fn streaming_una_sola_oracion_es_un_fragmento() {
        let text = "Esta es una única oración de prueba.";
        assert_eq!(chunk_text_streaming(text, 300), vec![text.to_string()]);
    }

    #[test]
    fn streaming_oracion_larguisima_se_trocea() {
        // Sin puntuación y por encima del máximo: cae al troceo por palabras.
        let long = "palabra ".repeat(100);
        let stream = chunk_text_streaming(&long, 50);
        assert!(stream.len() > 1);
        for c in &stream {
            assert!(c.len() <= 50);
        }
    }

    #[test]
    fn respeta_parrafos() {
        let text = "Primer párrafo.\n\nSegundo párrafo.";
        let chunks = chunk_text(text, Some(200));
        assert_eq!(
            chunks,
            vec![
                "Primer párrafo.".to_string(),
                "Segundo párrafo.".to_string()
            ]
        );
    }
}
