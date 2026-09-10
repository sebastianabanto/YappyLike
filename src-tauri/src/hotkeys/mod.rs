//! Atajos globales (§6, M3): registro, enrutado y control de re-entrada.
//!
//! Defaults: `Ctrl+Shift+Space` (leer), `Ctrl+Shift+S` (stop),
//! `Ctrl+Shift+P` (pausa/reanudar), `Ctrl+Shift+O` (settings). Funcionan sin
//! foco gracias a `tauri-plugin-global-shortcut`.
//!
//! Robustez pedida por el prompt:
//! - Si el registro de un atajo falla (otra app ya lo tiene), se avisa con una
//!   notificación nativa y **el resto sigue funcionando**.
//! - La acción de lectura tiene **debounce** y un **flag de captura en curso**
//!   para evitar re-entrada (dos pulsaciones no lanzan dos capturas solapadas).

mod hotkey;

pub use hotkey::Hotkey;

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use thiserror::Error;

use crate::capture::win32::{ThreadSleeper, Win32Clipboard, Win32Keyboard};
use crate::capture::SelectionCapture;
use crate::speech::SpeechService;

/// Ventana mínima entre dos lecturas aceptadas (debounce anti-rebote).
const READ_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("modificador o tecla desconocida: '{0}'")]
    UnknownKey(String),
    #[error("la combinación '{0}' no tiene tecla principal")]
    NoKey(String),
    #[error("la combinación '{0}' tiene más de una tecla principal")]
    MultipleKeys(String),
}

pub type Result<T> = std::result::Result<T, HotkeyError>;

/// Acciones disparables por atajo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Read,
    ReadClipboard,
    Stop,
    PauseResume,
    Settings,
}

impl HotkeyAction {
    /// Etiqueta legible para logs y notificaciones.
    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::Read => "Leer selección",
            HotkeyAction::ReadClipboard => "Leer portapapeles",
            HotkeyAction::Stop => "Detener",
            HotkeyAction::PauseResume => "Pausar / Reanudar",
            HotkeyAction::Settings => "Ajustes",
        }
    }
}

/// Combinaciones asignadas a cada acción.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyConfig {
    pub read: Hotkey,
    pub read_clipboard: Hotkey,
    pub stop: Hotkey,
    pub pause_resume: Hotkey,
    pub settings: Hotkey,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            read: Hotkey::ctrl_shift("Space"),
            read_clipboard: Hotkey::ctrl_shift("KeyC"),
            stop: Hotkey::ctrl_shift("KeyS"),
            pause_resume: Hotkey::ctrl_shift("KeyP"),
            settings: Hotkey::ctrl_shift("KeyO"),
        }
    }
}

impl HotkeyConfig {
    /// Pares `(acción, combinación)` en orden estable.
    pub fn entries(&self) -> [(HotkeyAction, &Hotkey); 5] {
        [
            (HotkeyAction::Read, &self.read),
            (HotkeyAction::ReadClipboard, &self.read_clipboard),
            (HotkeyAction::Stop, &self.stop),
            (HotkeyAction::PauseResume, &self.pause_resume),
            (HotkeyAction::Settings, &self.settings),
        ]
    }
}

/// Convierte un [`Hotkey`] al `Shortcut` del plugin.
fn to_shortcut(hk: &Hotkey) -> Result<Shortcut> {
    let mut mods = Modifiers::empty();
    if hk.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if hk.shift {
        mods |= Modifiers::SHIFT;
    }
    if hk.alt {
        mods |= Modifiers::ALT;
    }
    if hk.meta {
        mods |= Modifiers::SUPER;
    }
    let code = Code::from_str(&hk.key).map_err(|_| HotkeyError::UnknownKey(hk.key.clone()))?;
    let mods = if mods.is_empty() { None } else { Some(mods) };
    Ok(Shortcut::new(mods, code))
}

/// Backend real de captura (Win32) usado por la acción de lectura.
type SystemCapture = SelectionCapture<Win32Clipboard, Win32Keyboard, ThreadSleeper>;

/// Estado compartido entre los manejadores de atajo.
struct HotkeyRuntime {
    /// Verdadero mientras una captura está en curso (anti-reentrada).
    capturing: AtomicBool,
    /// Momento de la última lectura aceptada (debounce).
    last_read: Mutex<Option<Instant>>,
    capture: SystemCapture,
    speech: Arc<SpeechService>,
}

impl HotkeyRuntime {
    fn new(speech: Arc<SpeechService>) -> Self {
        Self {
            capturing: AtomicBool::new(false),
            last_read: Mutex::new(None),
            capture: SelectionCapture::system(),
            speech,
        }
    }

    /// `true` si la lectura debe ignorarse por caer dentro del debounce.
    fn debounced(&self) -> bool {
        let mut last = match self.last_read.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let now = Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev) < READ_DEBOUNCE {
                return true;
            }
        }
        *last = Some(now);
        false
    }
}

/// Captura la selección y la lee en voz alta (streaming). Común a los hotkeys y
/// al menú de bandeja. `speak` no bloquea: dispara el pipeline y vuelve.
fn capture_and_speak(capture: &SystemCapture, speech: &SpeechService) {
    match capture.capture_selected_text() {
        Some(t) => {
            let chars = t.chars().count();
            tracing::info!("lectura: {chars} caracteres capturados");
            // Una nueva lectura reemplaza la anterior (M5).
            speech.speak(&t);
        }
        None => {
            // TODO(M6): toast "Select some text first." (~2 s).
            tracing::info!("lectura: sin selección");
        }
    }
}

/// Lee el texto **ya presente** en el portapapeles (sin enviar `Ctrl+C` ni
/// alterarlo) y lo reproduce. Cubre apps donde la selección no se puede capturar
/// pero copiar sí funciona. Común a los hotkeys y al menú de bandeja.
fn clipboard_and_speak(capture: &SystemCapture, speech: &SpeechService) {
    match capture.clipboard_text().filter(|t| !t.trim().is_empty()) {
        Some(t) => {
            let chars = t.chars().count();
            tracing::info!("lectura de portapapeles: {chars} caracteres");
            // Una nueva lectura reemplaza la anterior (M5).
            speech.speak(&t);
        }
        None => {
            // Mismo criterio que "sin selección": no interrumpe, solo registra.
            tracing::info!("lectura de portapapeles: sin texto");
        }
    }
}

/// Captura + lectura desde el menú de bandeja, en un hilo aparte (la captura
/// bloquea hasta ~400 ms y no debe congelar el hilo de UI).
pub fn read_selection_async(speech: Arc<SpeechService>) {
    std::thread::spawn(move || {
        let capture: SystemCapture = SelectionCapture::system();
        capture_and_speak(&capture, &speech);
    });
}

/// Lectura del portapapeles desde el menú de bandeja, en un hilo aparte.
pub fn read_clipboard_async(speech: Arc<SpeechService>) {
    std::thread::spawn(move || {
        let capture: SystemCapture = SelectionCapture::system();
        clipboard_and_speak(&capture, &speech);
    });
}

/// Ejecuta la captura de la lectura en el hilo actual (invocado desde un hilo
/// aparte para no bloquear el bucle de eventos durante el poll de ~400 ms).
fn run_read(runtime: &HotkeyRuntime) {
    if runtime.debounced() {
        tracing::debug!("lectura ignorada por debounce");
        return;
    }
    // Anti-reentrada: si ya hay una captura activa, no lanzamos otra encima.
    if runtime.capturing.swap(true, Ordering::SeqCst) {
        tracing::debug!("captura ya en curso; se ignora la nueva pulsación");
        return;
    }

    capture_and_speak(&runtime.capture, &runtime.speech);
    runtime.capturing.store(false, Ordering::SeqCst);
}

/// Como [`run_read`] pero leyendo el portapapeles. Comparte el debounce y el
/// flag anti-reentrada (leer selección y leer portapapeles son excluyentes).
fn run_read_clipboard(runtime: &HotkeyRuntime) {
    if runtime.debounced() {
        tracing::debug!("lectura de portapapeles ignorada por debounce");
        return;
    }
    if runtime.capturing.swap(true, Ordering::SeqCst) {
        tracing::debug!("captura ya en curso; se ignora la pulsación de portapapeles");
        return;
    }

    clipboard_and_speak(&runtime.capture, &runtime.speech);
    runtime.capturing.store(false, Ordering::SeqCst);
}

/// Enruta una pulsación ya filtrada (solo `Pressed`).
fn dispatch(app: &AppHandle, action: HotkeyAction, runtime: &Arc<HotkeyRuntime>) {
    match action {
        HotkeyAction::Read => {
            // La captura bloquea hasta ~400 ms: fuera del hilo de eventos.
            let rt = Arc::clone(runtime);
            std::thread::spawn(move || run_read(&rt));
        }
        HotkeyAction::ReadClipboard => {
            // Leer el portapapeles es rápido, pero lo mantenemos fuera del hilo
            // de eventos por consistencia con la captura de selección.
            let rt = Arc::clone(runtime);
            std::thread::spawn(move || run_read_clipboard(&rt));
        }
        HotkeyAction::Stop => {
            tracing::info!("hotkey: Detener");
            runtime.speech.stop();
        }
        HotkeyAction::PauseResume => {
            tracing::info!("hotkey: Pausar/Reanudar");
            runtime.speech.toggle_pause();
        }
        HotkeyAction::Settings => {
            tracing::info!("hotkey: Ajustes");
            if let Err(e) = crate::commands::open_settings_window(app) {
                tracing::error!("no se pudo abrir la ventana de Settings: {e}");
            }
        }
    }
}

/// Muestra una notificación nativa; si falla, solo lo registra.
fn notify(app: &AppHandle, title: &str, body: String) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("no se pudo mostrar la notificación: {e}");
    }
}

/// Resultado del registro de un atajo (para reportar conflictos a la UI).
#[derive(Debug, Clone, Serialize)]
pub struct RegisterOutcome {
    /// Acción afectada (etiqueta legible).
    pub action: String,
    /// Combinación en forma de texto.
    pub hotkey: String,
    /// `None` si se registró bien; mensaje de error si hubo conflicto/fallo.
    pub error: Option<String>,
}

/// Registra todos los atajos sobre un `runtime`. El fallo de uno **no** aborta
/// el resto. Devuelve el resultado por atajo (para detección de conflictos).
fn register_all(
    app: &AppHandle,
    config: &HotkeyConfig,
    runtime: &Arc<HotkeyRuntime>,
    notify_enabled: bool,
) -> Vec<RegisterOutcome> {
    let shortcuts = app.global_shortcut();
    let mut outcomes = Vec::with_capacity(5);

    for (action, hk) in config.entries() {
        let shortcut = match to_shortcut(hk) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("combinación inválida para {}: {e}", action.label());
                if notify_enabled {
                    notify(
                        app,
                        "Atajo inválido",
                        format!("La combinación de «{}» no es válida: {e}", action.label()),
                    );
                }
                outcomes.push(RegisterOutcome {
                    action: action.label().to_string(),
                    hotkey: hk.to_string(),
                    error: Some(e.to_string()),
                });
                continue;
            }
        };

        let rt = Arc::clone(runtime);
        let result = shortcuts.on_shortcut(shortcut, move |app, _sc, event| {
            // El plugin dispara en pulsar y soltar; solo nos interesa pulsar.
            if event.state == ShortcutState::Pressed {
                dispatch(app, action, &rt);
            }
        });

        let error = match result {
            Ok(()) => {
                tracing::info!("atajo registrado: {hk} → {}", action.label());
                None
            }
            Err(e) => {
                tracing::warn!("no se pudo registrar {hk} para {}: {e}", action.label());
                if notify_enabled {
                    notify(
                        app,
                        "Atajo no disponible",
                        format!(
                            "No se pudo registrar {hk} para «{}» (¿lo usa otra app?). \
                             El resto de atajos sigue funcionando.",
                            action.label()
                        ),
                    );
                }
                Some(e.to_string())
            }
        };
        outcomes.push(RegisterOutcome {
            action: action.label().to_string(),
            hotkey: hk.to_string(),
            error,
        });
    }

    outcomes
}

/// Registra los atajos al arrancar (los `runtime`/closures quedan vivos dentro
/// del plugin mientras la app viva).
pub fn setup(
    app: &AppHandle,
    config: &HotkeyConfig,
    speech: Arc<SpeechService>,
    notify_enabled: bool,
) -> Result<()> {
    let runtime = Arc::new(HotkeyRuntime::new(speech));
    register_all(app, config, &runtime, notify_enabled);
    Ok(())
}

/// Re-registra los atajos con una configuración nueva (al guardar Settings):
/// desregistra los actuales y registra los nuevos. Devuelve los conflictos.
pub fn reload(
    app: &AppHandle,
    config: &HotkeyConfig,
    speech: Arc<SpeechService>,
    notify_enabled: bool,
) -> Vec<RegisterOutcome> {
    if let Err(e) = app.global_shortcut().unregister_all() {
        tracing::warn!("no se pudieron desregistrar los atajos previos: {e}");
    }
    let runtime = Arc::new(HotkeyRuntime::new(speech));
    register_all(app, config, &runtime, notify_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_coinciden_con_su_forma_de_texto() {
        let cfg = HotkeyConfig::default();
        assert_eq!(cfg.read, "Ctrl+Shift+Space".parse().unwrap());
        assert_eq!(cfg.read_clipboard, "Ctrl+Shift+C".parse().unwrap());
        assert_eq!(cfg.stop, "Ctrl+Shift+S".parse().unwrap());
        assert_eq!(cfg.pause_resume, "Ctrl+Shift+P".parse().unwrap());
        assert_eq!(cfg.settings, "Ctrl+Shift+O".parse().unwrap());
    }

    #[test]
    fn defaults_se_muestran_como_se_espera() {
        let cfg = HotkeyConfig::default();
        assert_eq!(cfg.read.to_string(), "Ctrl+Shift+Space");
        assert_eq!(cfg.read_clipboard.to_string(), "Ctrl+Shift+C");
        assert_eq!(cfg.stop.to_string(), "Ctrl+Shift+S");
        assert_eq!(cfg.pause_resume.to_string(), "Ctrl+Shift+P");
        assert_eq!(cfg.settings.to_string(), "Ctrl+Shift+O");
    }

    #[test]
    fn entries_van_en_orden_estable() {
        let cfg = HotkeyConfig::default();
        let acciones: Vec<_> = cfg.entries().iter().map(|(a, _)| *a).collect();
        assert_eq!(
            acciones,
            vec![
                HotkeyAction::Read,
                HotkeyAction::ReadClipboard,
                HotkeyAction::Stop,
                HotkeyAction::PauseResume,
                HotkeyAction::Settings,
            ]
        );
    }
}
