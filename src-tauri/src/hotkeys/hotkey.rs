//! Tipo [`Hotkey`]: parsing y serialización de combinaciones de teclas,
//! independiente del sistema operativo y del plugin de atajos.
//!
//! Formato de texto (el que verá el usuario y se guardará en config):
//! `Ctrl+Shift+Space`, `Alt+F4`, `Ctrl+Win+Up`… Los modificadores van primero
//! y hay **exactamente una** tecla principal. La tecla se guarda internamente
//! con su nombre canónico W3C (`Space`, `KeyS`, `ArrowUp`) para poder mapearla
//! al `Code` del plugin, pero se muestra en forma amigable (`Space`, `S`, `Up`).

use std::fmt;
use std::str::FromStr;

use super::{HotkeyError, Result};

/// Combinación de teclas: modificadores + una tecla principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Tecla Windows / Super / Cmd.
    pub meta: bool,
    /// Nombre canónico W3C de la tecla (`Space`, `KeyS`, `ArrowUp`, `F5`…).
    pub key: String,
}

/// Teclas con nombre: `(alias en minúsculas, código W3C, etiqueta amigable)`.
const NAMED_KEYS: &[(&str, &str, &str)] = &[
    ("space", "Space", "Space"),
    ("enter", "Enter", "Enter"),
    ("return", "Enter", "Enter"),
    ("tab", "Tab", "Tab"),
    ("escape", "Escape", "Escape"),
    ("esc", "Escape", "Escape"),
    ("backspace", "Backspace", "Backspace"),
    ("delete", "Delete", "Delete"),
    ("del", "Delete", "Delete"),
    ("insert", "Insert", "Insert"),
    ("ins", "Insert", "Insert"),
    ("home", "Home", "Home"),
    ("end", "End", "End"),
    ("pageup", "PageUp", "PageUp"),
    ("pagedown", "PageDown", "PageDown"),
    ("up", "ArrowUp", "Up"),
    ("down", "ArrowDown", "Down"),
    ("left", "ArrowLeft", "Left"),
    ("right", "ArrowRight", "Right"),
];

impl Hotkey {
    /// Atajo `Ctrl+Shift+<key>`, con `key` ya en forma canónica W3C. Uso interno
    /// para construir los defaults sin pasar por el parser.
    pub(super) fn ctrl_shift(key: &str) -> Self {
        Self {
            ctrl: true,
            shift: true,
            alt: false,
            meta: false,
            key: key.to_string(),
        }
    }
}

/// Convierte un token de tecla (lo que el usuario escribe) a su código W3C.
fn key_to_w3c(token: &str) -> Result<String> {
    let t = token.trim();
    if t.is_empty() {
        return Err(HotkeyError::UnknownKey(token.to_string()));
    }

    // Un único carácter: letra o dígito.
    if t.chars().count() == 1 {
        let c = t.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Ok(format!("Key{}", c.to_ascii_uppercase()));
        }
        if c.is_ascii_digit() {
            return Ok(format!("Digit{c}"));
        }
    }

    // Formas canónicas W3C escritas tal cual (`KeyS`, `Digit1`, `ArrowUp`).
    if let Some(rest) = t.strip_prefix("Key") {
        if rest.chars().count() == 1 && rest.chars().all(|c| c.is_ascii_alphabetic()) {
            return Ok(format!("Key{}", rest.to_ascii_uppercase()));
        }
    }
    if let Some(rest) = t.strip_prefix("Digit") {
        if rest.chars().count() == 1 && rest.chars().all(|c| c.is_ascii_digit()) {
            return Ok(format!("Digit{rest}"));
        }
    }
    if NAMED_KEYS.iter().any(|(_, w3c, _)| *w3c == t) {
        return Ok(t.to_string());
    }

    let lower = t.to_ascii_lowercase();

    // Teclas de función F1..F24.
    if let Some(num) = lower.strip_prefix('f') {
        if let Ok(n) = num.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Ok(format!("F{n}"));
            }
        }
    }

    // Teclas con nombre.
    if let Some((_, w3c, _)) = NAMED_KEYS.iter().find(|(alias, _, _)| *alias == lower) {
        return Ok(w3c.to_string());
    }

    Err(HotkeyError::UnknownKey(token.to_string()))
}

/// Etiqueta amigable de un código W3C (`KeyS` → `S`, `ArrowUp` → `Up`).
fn w3c_to_label(w3c: &str) -> String {
    if let Some(rest) = w3c.strip_prefix("Key") {
        if rest.chars().count() == 1 {
            return rest.to_string();
        }
    }
    if let Some(rest) = w3c.strip_prefix("Digit") {
        if rest.chars().count() == 1 {
            return rest.to_string();
        }
    }
    if let Some((_, _, label)) = NAMED_KEYS.iter().find(|(_, code, _)| *code == w3c) {
        return label.to_string();
    }
    w3c.to_string()
}

impl FromStr for Hotkey {
    type Err = HotkeyError;

    fn from_str(s: &str) -> Result<Self> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut meta = false;
        let mut key: Option<String> = None;

        for raw in s.split('+') {
            let token = raw.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "win" | "super" | "meta" | "cmd" | "command" => meta = true,
                _ => {
                    let w3c = key_to_w3c(token)?;
                    if key.is_some() {
                        return Err(HotkeyError::MultipleKeys(s.to_string()));
                    }
                    key = Some(w3c);
                }
            }
        }

        let key = key.ok_or_else(|| HotkeyError::NoKey(s.to_string()))?;
        Ok(Hotkey {
            ctrl,
            shift,
            alt,
            meta,
            key,
        })
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.meta {
            parts.push("Win");
        }
        let label = w3c_to_label(&self.key);
        parts.push(&label);
        write!(f, "{}", parts.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Hotkey {
        s.parse().unwrap()
    }

    #[test]
    fn parsea_ctrl_shift_space() {
        let hk = parse("Ctrl+Shift+Space");
        assert!(hk.ctrl && hk.shift && !hk.alt && !hk.meta);
        assert_eq!(hk.key, "Space");
    }

    #[test]
    fn letra_suelta_se_normaliza_a_key() {
        let hk = parse("ctrl+shift+s");
        assert_eq!(hk.key, "KeyS");
        assert!(hk.ctrl && hk.shift);
    }

    #[test]
    fn roundtrip_display_parse() {
        for s in [
            "Ctrl+Shift+Space",
            "Ctrl+Shift+S",
            "Ctrl+Shift+P",
            "Ctrl+Shift+O",
            "Alt+F4",
            "Ctrl+Win+Up",
            "F1",
        ] {
            let hk = parse(s);
            // La forma mostrada vuelve a parsear al mismo Hotkey.
            assert_eq!(parse(&hk.to_string()), hk, "roundtrip de {s}");
        }
    }

    #[test]
    fn display_es_amigable() {
        assert_eq!(parse("ctrl+shift+s").to_string(), "Ctrl+Shift+S");
        assert_eq!(parse("control+win+up").to_string(), "Ctrl+Win+Up");
        assert_eq!(parse("alt+f4").to_string(), "Alt+F4");
    }

    #[test]
    fn acepta_forma_canonica_w3c() {
        assert_eq!(parse("Ctrl+KeyS").key, "KeyS");
        assert_eq!(parse("ArrowLeft").key, "ArrowLeft");
        assert_eq!(parse("Digit1").key, "Digit1");
    }

    #[test]
    fn error_si_falta_tecla_principal() {
        assert!(matches!(
            "Ctrl+Shift".parse::<Hotkey>(),
            Err(HotkeyError::NoKey(_))
        ));
    }

    #[test]
    fn error_si_hay_dos_teclas() {
        assert!(matches!(
            "Ctrl+A+B".parse::<Hotkey>(),
            Err(HotkeyError::MultipleKeys(_))
        ));
    }

    #[test]
    fn error_si_tecla_desconocida() {
        assert!(matches!(
            "Ctrl+Shift+Wat".parse::<Hotkey>(),
            Err(HotkeyError::UnknownKey(_))
        ));
    }

    #[test]
    fn sin_modificadores_es_valido() {
        let hk = parse("F5");
        assert!(!hk.ctrl && !hk.shift && !hk.alt && !hk.meta);
        assert_eq!(hk.key, "F5");
    }
}
