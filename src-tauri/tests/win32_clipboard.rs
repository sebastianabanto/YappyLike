//! Verificación del backend Win32 real del portapapeles, sin `SendInput` ni
//! foco de ventanas (para no depender de apps GUI). Ejercita de verdad
//! `GlobalAlloc` / `SetClipboardData` / `GetClipboardData` / `GlobalLock`.
//!
//! Modifica el portapapeles del sistema, así que va marcado `#[ignore]`: no
//! corre en el gate normal. Ejecutar a mano con:
//! ```text
//! cargo test --test win32_clipboard -- --ignored
//! ```

#![cfg(windows)]

use yappylike_lib::capture::win32::Win32Clipboard;
use yappylike_lib::capture::{Clipboard, ClipboardSnapshot};

/// Codifica una cadena como bytes de `CF_UNICODETEXT` (UTF-16LE + NUL final).
fn unicode_text_bytes(s: &str) -> Vec<u8> {
    let mut units: Vec<u16> = s.encode_utf16().collect();
    units.push(0); // terminador NUL
    let mut bytes = Vec::with_capacity(units.len() * 2);
    for u in units {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    bytes
}

const CF_UNICODETEXT: u32 = 13;

#[test]
#[ignore = "toca el portapapeles del sistema; correr con --ignored"]
fn roundtrip_texto_por_portapapeles_real() {
    let clip = Win32Clipboard::for_tests();

    // Respalda lo que el usuario tuviera, para dejarlo como estaba al terminar.
    let original = clip.snapshot().expect("snapshot inicial");

    let esperado = "PRUEBA M2 · acentos ñ é ü · símbolo € · 漢字";
    let inyectado = ClipboardSnapshot {
        formats: vec![(CF_UNICODETEXT, unicode_text_bytes(esperado))],
    };

    // restore() ejercita EmptyClipboard + GlobalAlloc + SetClipboardData reales.
    clip.restore(&inyectado).expect("inyectar texto");

    // read_text() ejercita GetClipboardData + GlobalLock reales.
    let leido = clip.read_text().expect("leer texto");
    assert_eq!(leido.as_deref(), Some(esperado));

    // El sequence number es un u32 vivo del sistema (solo comprobamos que existe).
    let _ = clip.sequence_number();

    // Deja el portapapeles como estaba.
    let _ = clip.restore(&original);
}
