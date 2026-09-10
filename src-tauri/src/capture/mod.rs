//! `SelectionCapture` — captura el texto seleccionado en cualquier app.
//!
//! Flujo (§6, M2): respaldar el portapapeles → registrar su *sequence number*
//! → enviar `Ctrl+C` con `SendInput` → hacer *poll* hasta que el número cambie
//! → leer el texto → **restaurar** el portapapeles original *solo si el usuario
//! no copió otra cosa mientras tanto*.
//!
//! La lógica vive aquí de forma **agnóstica al sistema operativo**: las tres
//! operaciones de plataforma (portapapeles, teclado, dormir) se abstraen en
//! traits para poder mockearlas en tests. La implementación real de Win32 está
//! en [`win32`] y solo se compila en Windows.

#[cfg(windows)]
pub mod win32;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("el portapapeles quedó ocupado por otra app tras {0} intentos")]
    ClipboardBusy(u32),
    #[cfg(windows)]
    #[error("error de Win32: {0}")]
    Win32(#[from] windows::core::Error),
    #[error("fallo de captura: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

/// Copia (best-effort) de los formatos del portapapeles que sabemos preservar.
///
/// Cada entrada es `(formato, bytes crudos)`. Formatos no soportados (imágenes,
/// listas de archivos) **no** se capturan y por tanto no se restauran; ese es
/// un límite documentado del backup.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub formats: Vec<(u32, Vec<u8>)>,
}

impl ClipboardSnapshot {
    /// `true` si no se pudo respaldar ningún formato conocido.
    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
    }
}

/// Operaciones de portapapeles necesarias para el flujo de captura.
pub trait Clipboard {
    /// *Sequence number* del portapapeles: cambia cada vez que **cualquier**
    /// app escribe en él. Leer no lo altera.
    fn sequence_number(&self) -> u32;
    /// Lee el texto Unicode actual (`None` si no hay texto).
    fn read_text(&self) -> Result<Option<String>>;
    /// Respalda los formatos soportados del contenido actual.
    fn snapshot(&self) -> Result<ClipboardSnapshot>;
    /// Reinstala un respaldo previamente tomado.
    fn restore(&self, snapshot: &ClipboardSnapshot) -> Result<()>;
}

/// Envío de la combinación de copiado al foco actual.
pub trait KeySender {
    /// Emula `Ctrl+C`.
    fn send_copy(&self) -> Result<()>;
}

/// Abstracción de `sleep` para poder saltarla en tests.
pub trait Sleeper {
    fn sleep_ms(&self, ms: u64);
}

/// Parámetros del flujo de captura.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Intervalo entre sondeos del *sequence number*.
    pub poll_interval_ms: u64,
    /// Tiempo máximo esperando a que el portapapeles cambie tras `Ctrl+C`.
    pub poll_timeout_ms: u64,
    /// Reintentos de `OpenClipboard` si otra app lo tiene abierto.
    pub open_retries: u32,
    /// Espera entre reintentos de apertura.
    pub open_backoff_ms: u64,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        // §6: poll ~20 ms hasta 400 ms; hasta 5 reintentos con backoff corto.
        Self {
            poll_interval_ms: 20,
            poll_timeout_ms: 400,
            open_retries: 5,
            open_backoff_ms: 15,
        }
    }
}

/// Captura de la selección activa, parametrizada por sus tres dependencias de
/// plataforma para permitir tests con mocks.
pub struct SelectionCapture<C: Clipboard, K: KeySender, S: Sleeper> {
    clipboard: C,
    keys: K,
    sleeper: S,
    cfg: CaptureConfig,
}

impl<C: Clipboard, K: KeySender, S: Sleeper> SelectionCapture<C, K, S> {
    /// Construye la captura a partir de sus piezas.
    pub fn with_parts(clipboard: C, keys: K, sleeper: S, cfg: CaptureConfig) -> Self {
        Self {
            clipboard,
            keys,
            sleeper,
            cfg,
        }
    }

    /// Captura el texto seleccionado. **Nunca** hace panic ni bloquea de forma
    /// indefinida; ante cualquier error devuelve `None` y lo registra.
    pub fn capture_selected_text(&self) -> Option<String> {
        match self.try_capture() {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!("captura de selección fallida: {e}");
                None
            }
        }
    }

    /// Reinstala un respaldo del portapapeles. Expuesto para completar la
    /// abstracción pedida (§6); el flujo normal ya restaura internamente.
    pub fn restore_clipboard(&self, snapshot: &ClipboardSnapshot) -> Result<()> {
        self.clipboard.restore(snapshot)
    }

    /// Lee el texto que hay ahora mismo en el portapapeles (utilidad para
    /// diagnósticos y para el probe de aceptación).
    pub fn clipboard_text(&self) -> Option<String> {
        self.clipboard.read_text().ok().flatten()
    }

    /// Núcleo del flujo, separado para poder propagar errores en tests.
    fn try_capture(&self) -> Result<Option<String>> {
        let seq_before = self.clipboard.sequence_number();

        // 1) Respaldo best-effort. Si el portapapeles está ocupado, seguimos
        //    adelante sin poder restaurar (mejor leer que abortar).
        let snapshot = match self.clipboard.snapshot() {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("no se pudo respaldar el portapapeles: {e}");
                None
            }
        };

        // 2) Ctrl+C al foco actual.
        self.keys.send_copy()?;

        // 3) Poll hasta que el sequence number cambie o venza el timeout.
        let mut waited = 0u64;
        let mut changed = false;
        while waited < self.cfg.poll_timeout_ms {
            self.sleeper.sleep_ms(self.cfg.poll_interval_ms);
            waited += self.cfg.poll_interval_ms;
            if self.clipboard.sequence_number() != seq_before {
                changed = true;
                break;
            }
        }

        // Sin cambio ⇒ no había selección. El portapapeles no se tocó, así que
        // no hay nada que restaurar.
        if !changed {
            tracing::debug!("Ctrl+C no alteró el portapapeles: sin selección");
            return Ok(None);
        }

        // Número justo después de nuestro copiado; leer texto no lo altera.
        let seq_after_copy = self.clipboard.sequence_number();
        let text = self.clipboard.read_text()?;

        // 4) Restaurar el original **solo** si nadie copió algo nuevo después
        //    de nosotros, y solo si teníamos un respaldo con contenido.
        if let Some(snap) = &snapshot {
            if !snap.is_empty() {
                if self.clipboard.sequence_number() == seq_after_copy {
                    if let Err(e) = self.clipboard.restore(snap) {
                        tracing::warn!("no se pudo restaurar el portapapeles: {e}");
                    }
                } else {
                    tracing::debug!("el usuario copió algo nuevo; no se restaura");
                }
            }
        }

        // Trata el texto en blanco como "sin selección".
        Ok(text.filter(|t| !t.trim().is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Estado compartido que simula el portapapeles del sistema.
    #[derive(Default)]
    struct FakeState {
        seq: u32,
        text: Option<String>,
    }

    /// Teclado falso: `send_copy` simula el efecto de `Ctrl+C` sobre el estado
    /// (sube el sequence number y coloca la "selección" como contenido). Si no
    /// hay selección, no altera nada (como un `Ctrl+C` sin texto marcado).
    struct FakeKeyboard {
        state: Rc<RefCell<FakeState>>,
        selection: Option<String>,
    }

    impl KeySender for FakeKeyboard {
        fn send_copy(&self) -> Result<()> {
            if let Some(sel) = &self.selection {
                let mut st = self.state.borrow_mut();
                st.seq += 1;
                st.text = Some(sel.clone());
            }
            Ok(())
        }
    }

    struct FakeClipboard {
        state: Rc<RefCell<FakeState>>,
        snapshot_fails: bool,
        /// Simula que otra app copia algo entre nuestro copiado y la lectura.
        bump_seq_on_read: bool,
        restore_log: Rc<RefCell<Vec<ClipboardSnapshot>>>,
    }

    impl Clipboard for FakeClipboard {
        fn sequence_number(&self) -> u32 {
            self.state.borrow().seq
        }
        fn read_text(&self) -> Result<Option<String>> {
            if self.bump_seq_on_read {
                self.state.borrow_mut().seq += 1;
            }
            Ok(self.state.borrow().text.clone())
        }
        fn snapshot(&self) -> Result<ClipboardSnapshot> {
            if self.snapshot_fails {
                return Err(CaptureError::ClipboardBusy(5));
            }
            Ok(ClipboardSnapshot {
                formats: vec![(13, b"original".to_vec())],
            })
        }
        fn restore(&self, snapshot: &ClipboardSnapshot) -> Result<()> {
            self.restore_log.borrow_mut().push(snapshot.clone());
            Ok(())
        }
    }

    struct NoSleep;
    impl Sleeper for NoSleep {
        fn sleep_ms(&self, _ms: u64) {}
    }

    /// Registro de restauraciones observadas por el mock.
    type RestoreLog = Rc<RefCell<Vec<ClipboardSnapshot>>>;
    type Harness = (
        SelectionCapture<FakeClipboard, FakeKeyboard, NoSleep>,
        RestoreLog,
    );

    fn build(selection: Option<&str>, snapshot_fails: bool, bump_seq_on_read: bool) -> Harness {
        let state = Rc::new(RefCell::new(FakeState::default()));
        let restore_log = Rc::new(RefCell::new(Vec::new()));
        let clip = FakeClipboard {
            state: state.clone(),
            snapshot_fails,
            bump_seq_on_read,
            restore_log: restore_log.clone(),
        };
        let kb = FakeKeyboard {
            state,
            selection: selection.map(str::to_string),
        };
        let cap = SelectionCapture::with_parts(clip, kb, NoSleep, CaptureConfig::default());
        (cap, restore_log)
    }

    #[test]
    fn captura_texto_y_restaura_original() {
        let (cap, restore_log) = build(Some("hola ñandú €"), false, false);
        let text = cap.capture_selected_text();
        assert_eq!(text.as_deref(), Some("hola ñandú €"));
        // Se restauró exactamente el respaldo original.
        assert_eq!(restore_log.borrow().len(), 1);
        assert_eq!(restore_log.borrow()[0].formats[0].1, b"original");
    }

    #[test]
    fn sin_seleccion_devuelve_none_y_no_restaura() {
        let (cap, restore_log) = build(None, false, false);
        assert_eq!(cap.capture_selected_text(), None);
        // El portapapeles nunca cambió ⇒ no se toca.
        assert!(restore_log.borrow().is_empty());
    }

    #[test]
    fn si_el_usuario_copia_algo_nuevo_no_se_restaura() {
        // `bump_seq_on_read` simula una copia ajena entre copiar y restaurar.
        let (cap, restore_log) = build(Some("seleccion"), false, true);
        let text = cap.capture_selected_text();
        assert_eq!(text.as_deref(), Some("seleccion"));
        assert!(
            restore_log.borrow().is_empty(),
            "no debe pisar lo que el usuario copió después"
        );
    }

    #[test]
    fn respaldo_fallido_no_impide_leer_la_seleccion() {
        // El portapapeles estaba ocupado al respaldar; aun así devolvemos texto.
        let (cap, restore_log) = build(Some("aun asi"), true, false);
        assert_eq!(cap.capture_selected_text().as_deref(), Some("aun asi"));
        assert!(restore_log.borrow().is_empty());
    }

    #[test]
    fn texto_en_blanco_se_trata_como_sin_seleccion() {
        let (cap, _restore_log) = build(Some("   \r\n  "), false, false);
        assert_eq!(cap.capture_selected_text(), None);
    }
}
