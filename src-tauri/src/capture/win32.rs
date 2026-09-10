//! Implementación de [`Clipboard`], [`KeySender`] y [`Sleeper`] sobre Win32.
//!
//! Todas las llamadas a la API van envueltas para garantizar dos invariantes
//! del §6: **nunca** hacer panic y **siempre** cerrar el portapapeles
//! (`CloseClipboard`) en todos los caminos, incluido error — para eso está el
//! guard RAII [`ClipboardGuard`].

use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

use super::{
    CaptureConfig, CaptureError, Clipboard, ClipboardSnapshot, KeySender, Result, SelectionCapture,
    Sleeper,
};

/// `CF_UNICODETEXT`. Se define aquí como literal para no depender de en qué
/// módulo del crate `windows` viven las constantes de formato.
const CF_UNICODETEXT: u32 = 13;

/// Guard RAII: abre el portapapeles (con reintentos) y garantiza su cierre.
struct ClipboardGuard;

impl ClipboardGuard {
    /// Abre el portapapeles reintentando si otra app lo tiene bloqueado.
    fn open(retries: u32, backoff_ms: u64) -> Result<Self> {
        let mut attempt = 0u32;
        loop {
            // SAFETY: `OpenClipboard(None)` asocia el portapapeles al hilo
            // actual; su cierre lo cubre el `Drop` de este guard.
            let opened = unsafe { OpenClipboard(None) }.is_ok();
            if opened {
                return Ok(Self);
            }
            attempt += 1;
            if attempt >= retries {
                return Err(CaptureError::ClipboardBusy(retries));
            }
            std::thread::sleep(Duration::from_millis(backoff_ms));
        }
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: correspondencia 1:1 con el `OpenClipboard` de `open`.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Registra (o resuelve) un formato de portapapeles con nombre.
fn register_format(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` está terminado en NUL y vive durante toda la llamada.
    unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
}

/// Lee los bytes crudos de un formato ya con el portapapeles abierto.
///
/// SAFETY: requiere que el portapapeles esté abierto (guard vivo).
unsafe fn read_format_bytes(fmt: u32) -> Option<Vec<u8>> {
    let handle = GetClipboardData(fmt).ok()?;
    if handle.is_invalid() {
        return None;
    }
    let hglobal = HGLOBAL(handle.0);
    let ptr = GlobalLock(hglobal) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let size = GlobalSize(hglobal);
    let bytes = if size == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(ptr, size).to_vec()
    };
    let _ = GlobalUnlock(hglobal);
    Some(bytes)
}

/// Reserva un `HGLOBAL` movible y copia `bytes` dentro.
///
/// SAFETY: el handle devuelto queda **sin** liberar; lo libera quien lo reciba
/// (normalmente `SetClipboardData` toma la propiedad al tener éxito).
unsafe fn alloc_global(bytes: &[u8]) -> Option<HGLOBAL> {
    let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
    let ptr = GlobalLock(hmem) as *mut u8;
    if ptr.is_null() {
        let _ = GlobalFree(Some(hmem));
        return None;
    }
    if !bytes.is_empty() {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    }
    let _ = GlobalUnlock(hmem);
    Some(hmem)
}

/// Portapapeles real de Windows.
pub struct Win32Clipboard {
    open_retries: u32,
    open_backoff_ms: u64,
}

impl Win32Clipboard {
    /// Construye un portapapeles con los reintentos por defecto. Pensado para
    /// tests de integración que usan el backend real de forma aislada.
    pub fn for_tests() -> Self {
        let cfg = CaptureConfig::default();
        Self {
            open_retries: cfg.open_retries,
            open_backoff_ms: cfg.open_backoff_ms,
        }
    }

    fn guard(&self) -> Result<ClipboardGuard> {
        ClipboardGuard::open(self.open_retries, self.open_backoff_ms)
    }
}

impl Clipboard for Win32Clipboard {
    fn sequence_number(&self) -> u32 {
        // SAFETY: no requiere abrir el portapapeles.
        unsafe { GetClipboardSequenceNumber() }
    }

    fn read_text(&self) -> Result<Option<String>> {
        let _guard = self.guard()?;
        // SAFETY: portapapeles abierto mientras vive `_guard`.
        unsafe {
            let handle = match GetClipboardData(CF_UNICODETEXT) {
                Ok(h) if !h.is_invalid() => h,
                _ => return Ok(None),
            };
            let hglobal = HGLOBAL(handle.0);
            let ptr = GlobalLock(hglobal) as *const u16;
            if ptr.is_null() {
                return Ok(None);
            }
            // El bloque está terminado en NUL; acotamos además por su tamaño.
            let max_units = GlobalSize(hglobal) / 2;
            let mut len = 0usize;
            while len < max_units && *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(hglobal);
            Ok(Some(text))
        }
    }

    fn snapshot(&self) -> Result<ClipboardSnapshot> {
        let _guard = self.guard()?;
        let wanted = [
            CF_UNICODETEXT,
            register_format("HTML Format"),
            register_format("Rich Text Format"),
        ];
        let mut formats = Vec::new();
        for fmt in wanted {
            if fmt == 0 {
                continue;
            }
            // SAFETY: portapapeles abierto mientras vive `_guard`.
            if let Some(bytes) = unsafe { read_format_bytes(fmt) } {
                formats.push((fmt, bytes));
            }
        }
        Ok(ClipboardSnapshot { formats })
    }

    fn restore(&self, snapshot: &ClipboardSnapshot) -> Result<()> {
        if snapshot.is_empty() {
            return Ok(());
        }
        let _guard = self.guard()?;
        // SAFETY: portapapeles abierto mientras vive `_guard`.
        unsafe {
            EmptyClipboard()?;
            for (fmt, bytes) in &snapshot.formats {
                let Some(hmem) = alloc_global(bytes) else {
                    continue;
                };
                // `SetClipboardData` toma la propiedad del bloque al tener
                // éxito; si falla, lo liberamos nosotros para no filtrar.
                if SetClipboardData(*fmt, Some(HANDLE(hmem.0))).is_err() {
                    let _ = GlobalFree(Some(hmem));
                }
            }
        }
        Ok(())
    }
}

/// Teclado real: envía `Ctrl+C` vía `SendInput`.
pub struct Win32Keyboard;

fn key_event(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Modificadores a soltar antes del `Ctrl+C`. El atajo de lectura incluye
/// `Shift`, así que si el usuario lo tiene pulsado al disparar, el `Ctrl+C` se
/// interpretaría como `Ctrl+Shift+C` y no copiaría nada. Soltamos todos los
/// modificadores (los que no estén pulsados generan keyups inocuos) para enviar
/// un `Ctrl+C` limpio.
const MODIFIERS_TO_RELEASE: &[VIRTUAL_KEY] = &[
    VK_LSHIFT,
    VK_RSHIFT,
    VK_SHIFT,
    VK_LMENU,
    VK_RMENU,
    VK_MENU,
    VK_LWIN,
    VK_RWIN,
    VK_LCONTROL,
    VK_RCONTROL,
    VK_CONTROL,
];

/// Secuencia lógica de eventos para un `Ctrl+C` limpio: primero se sueltan los
/// modificadores (keyup), luego el `Ctrl+C`. Cada par es `(tecla, es_keyup)`.
/// Función pura para poder verificar el orden en tests sin tocar el sistema.
fn copy_key_sequence() -> Vec<(VIRTUAL_KEY, bool)> {
    let mut seq: Vec<(VIRTUAL_KEY, bool)> =
        MODIFIERS_TO_RELEASE.iter().map(|&vk| (vk, true)).collect();
    seq.push((VK_CONTROL, false)); // Ctrl ↓
    seq.push((VK_C, false)); // C ↓
    seq.push((VK_C, true)); // C ↑
    seq.push((VK_CONTROL, true)); // Ctrl ↑
    seq
}

impl KeySender for Win32Keyboard {
    fn send_copy(&self) -> Result<()> {
        let inputs: Vec<INPUT> = copy_key_sequence()
            .into_iter()
            .map(|(vk, up)| {
                let flags = if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                };
                key_event(vk, flags)
            })
            .collect();

        // SAFETY: `inputs` es un slice válido y `cbsize` es el tamaño de `INPUT`.
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err(CaptureError::Other(format!(
                "SendInput envió {sent}/{} eventos",
                inputs.len()
            )));
        }
        Ok(())
    }
}

/// `Sleeper` real basado en `std::thread::sleep`.
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

impl SelectionCapture<Win32Clipboard, Win32Keyboard, ThreadSleeper> {
    /// Construye la captura con los backends reales de Windows.
    pub fn system() -> Self {
        let cfg = CaptureConfig::default();
        let clipboard = Win32Clipboard {
            open_retries: cfg.open_retries,
            open_backoff_ms: cfg.open_backoff_ms,
        };
        SelectionCapture::with_parts(clipboard, Win32Keyboard, ThreadSleeper, cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_suelta_modificadores_antes_del_ctrl_c() {
        let seq = copy_key_sequence();

        let shift_up = seq
            .iter()
            .position(|&(vk, up)| vk == VK_SHIFT && up)
            .expect("debe soltar Shift");
        let ctrl_down = seq
            .iter()
            .position(|&(vk, up)| vk == VK_CONTROL && !up)
            .expect("debe pulsar Ctrl para copiar");
        let c_down = seq
            .iter()
            .position(|&(vk, up)| vk == VK_C && !up)
            .expect("debe pulsar C");

        // El keyup de Shift ocurre antes de pulsar Ctrl y C (si no, sería
        // Ctrl+Shift+C y la copia no ocurriría).
        assert!(shift_up < ctrl_down, "Shift debe soltarse antes de Ctrl");
        assert!(shift_up < c_down, "Shift debe soltarse antes de C");
        // El primer evento es un keyup (soltar modificadores).
        assert!(seq[0].1, "la secuencia empieza soltando modificadores");
        // Cierra devolviendo Ctrl y C a estado soltado.
        assert_eq!(seq.last(), Some(&(VK_CONTROL, true)));
    }
}
