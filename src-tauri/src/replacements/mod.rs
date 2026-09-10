//! Diccionario de reemplazos de texto (feature #7).
//!
//! Sustituye términos en el texto **antes** de sintetizar, para que anglicismos
//! y jerga técnica suenen bien sin trocear la síntesis (lo que destruiría la
//! entonación). Cada regla es `from → to`; el reemplazo puede ser una
//! reescritura fonética (`deadline` → `dedláin`) o una traducción
//! (`deadline` → `fecha límite`). Coincide por **palabra completa** y sin
//! distinguir mayúsculas, preservando la mayúscula inicial del original.

use serde::{Deserialize, Serialize};

/// Una regla de reemplazo del diccionario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    /// Término a buscar (palabra completa, sin distinguir mayúsculas).
    pub from: String,
    /// Texto por el que se sustituye.
    pub to: String,
    /// Si está activa. Permite desactivar una regla sin borrarla.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Replacement {
    fn new(from: &str, to: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            enabled: true,
        }
    }
}

/// Lista inicial de anglicismos frecuentes en tecnología e ingeniería, con una
/// reescritura fonética aproximada al español. **Editable** por el usuario desde
/// Ajustes; son solo un punto de partida.
pub fn seed() -> Vec<Replacement> {
    [
        ("deadline", "dedláin"),
        ("meeting", "mítin"),
        ("deploy", "diplói"),
        ("deployment", "diplóiment"),
        ("software", "sóftuer"),
        ("hardware", "járduer"),
        ("bug", "bag"),
        ("debug", "dibág"),
        ("feedback", "fídbak"),
        ("login", "lóguin"),
        ("logout", "logáut"),
        ("release", "rilís"),
        ("commit", "comít"),
        ("update", "apdéit"),
        ("backup", "bákap"),
        ("framework", "fréimwork"),
        ("online", "onláin"),
        ("offline", "ofláin"),
        ("email", "ímeil"),
        ("password", "pásuord"),
        ("streaming", "estrímin"),
        ("query", "cuíri"),
        ("cache", "cash"),
        ("request", "ricuést"),
        ("default", "difólt"),
    ]
    .into_iter()
    .map(|(f, t)| Replacement::new(f, t))
    .collect()
}

/// Aplica las reglas activas a `text`. Coincide por palabra completa,
/// sin distinguir mayúsculas, y preserva la mayúscula inicial del original.
///
/// Las reglas se aplican de `from` más largo a más corto para reducir solapes.
/// Compila las expresiones en cada llamada; para el uso previsto (una vez por
/// lectura, pocas reglas) es despreciable frente al coste de la síntesis.
pub fn apply(text: &str, rules: &[Replacement]) -> String {
    let mut active: Vec<&Replacement> = rules
        .iter()
        .filter(|r| r.enabled && !r.from.trim().is_empty())
        .collect();
    active.sort_by_key(|r| std::cmp::Reverse(r.from.trim().chars().count()));

    let mut out = text.to_string();
    for rule in active {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(rule.from.trim()));
        let re = match regex::Regex::new(&pattern) {
            Ok(re) => re,
            Err(_) => continue,
        };
        let to = rule.to.clone();
        let replaced = re
            .replace_all(&out, |caps: &regex::Captures| {
                capitalize_like(&caps[0], &to)
            })
            .into_owned();
        out = replaced;
    }
    out
}

/// Si `matched` empieza en mayúscula, devuelve `replacement` con su primera
/// letra en mayúscula; si no, lo devuelve tal cual.
fn capitalize_like(matched: &str, replacement: &str) -> String {
    let first_upper = matched
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false);
    if !first_upper {
        return replacement.to_string();
    }
    let mut chars = replacement.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => replacement.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<Replacement> {
        vec![
            Replacement::new("deadline", "dedláin"),
            Replacement::new("meeting", "mítin"),
        ]
    }

    #[test]
    fn reemplaza_palabra_completa() {
        assert_eq!(
            apply("El deadline del meeting es hoy", &rules()),
            "El dedláin del mítin es hoy"
        );
    }

    #[test]
    fn no_toca_subcadenas() {
        // "deadliner" contiene "deadline" pero no es palabra completa.
        assert_eq!(apply("un deadliner raro", &rules()), "un deadliner raro");
    }

    #[test]
    fn es_case_insensitive_y_preserva_mayuscula_inicial() {
        assert_eq!(apply("Deadline urgente", &rules()), "Dedláin urgente");
        assert_eq!(apply("DEADLINE", &rules()), "Dedláin");
    }

    #[test]
    fn respeta_puntuacion_adyacente() {
        assert_eq!(apply("¿el deadline?", &rules()), "¿el dedláin?");
        assert_eq!(apply("meeting, ya", &rules()), "mítin, ya");
    }

    #[test]
    fn regla_desactivada_no_aplica() {
        let mut r = rules();
        r[0].enabled = false;
        assert_eq!(apply("el deadline", &r), "el deadline");
    }

    #[test]
    fn tabla_vacia_no_cambia_nada() {
        assert_eq!(apply("texto intacto", &[]), "texto intacto");
    }

    #[test]
    fn ignora_reglas_con_from_vacio() {
        let r = vec![Replacement::new("  ", "x")];
        assert_eq!(apply("nada que hacer", &r), "nada que hacer");
    }

    #[test]
    fn la_semilla_es_valida_y_no_rompe() {
        // Todas las reglas de la semilla compilan y no truenan.
        let s = seed();
        assert!(!s.is_empty());
        let _ = apply("deploy del software con bug", &s);
    }
}
