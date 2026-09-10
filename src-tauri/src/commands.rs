//! Estado compartido de la app y comandos Tauri para el flujo de primer
//! arranque (descarga del modelo). La ventana Welcome (frontend) invoca estos
//! comandos y escucha los eventos `download://*`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::hotkeys::RegisterOutcome;
use crate::model::{self, Manifest};
use crate::settings::Config;
use crate::speech::SpeechService;

/// Estado global compartido entre comandos, atajos y ventanas.
pub struct AppState {
    pub models_dir: PathBuf,
    pub manifest: Manifest,
    pub speech: Arc<SpeechService>,
    /// Configuración persistente (fuente de verdad en memoria).
    pub config: Mutex<Config>,
    download_cancel: AtomicBool,
    download_active: AtomicBool,
}

impl AppState {
    pub fn new(
        models_dir: PathBuf,
        manifest: Manifest,
        speech: Arc<SpeechService>,
        config: Config,
    ) -> Self {
        Self {
            models_dir,
            manifest,
            speech,
            config: Mutex::new(config),
            download_cancel: AtomicBool::new(false),
            download_active: AtomicBool::new(false),
        }
    }

    /// `true` si faltan assets por descargar.
    pub fn needs_download(&self) -> bool {
        !model::all_present(&self.models_dir, &self.manifest)
    }

    /// Copia de la config actual.
    pub fn config_snapshot(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

/// Info que la ventana Welcome pide al cargar.
#[derive(Serialize)]
pub struct DownloadInfo {
    pub needed: bool,
    pub target_dir: String,
    pub approx_mb: u64,
}

/// Payload del evento de progreso.
#[derive(Clone, Serialize)]
struct ProgressPayload {
    downloaded: u64,
    total: u64,
    percent: f64,
    file: String,
    file_index: usize,
    file_count: usize,
}

#[derive(Clone, Serialize)]
struct ErrorPayload {
    message: String,
}

#[tauri::command]
pub fn download_info(state: State<'_, Arc<AppState>>) -> DownloadInfo {
    DownloadInfo {
        needed: state.needs_download(),
        target_dir: state.models_dir.display().to_string(),
        approx_mb: state.manifest.approx_total_mb,
    }
}

/// Cancela la descarga en curso (si la hay).
#[tauri::command]
pub fn cancel_download(state: State<'_, Arc<AppState>>) {
    state.download_cancel.store(true, Ordering::Relaxed);
}

/// Inicia la descarga de los assets que falten en un hilo aparte y emite
/// eventos `download://progress` | `download://done` | `download://error`.
#[tauri::command]
pub fn start_download(app: AppHandle, state: State<'_, Arc<AppState>>) {
    // Evita dos descargas simultáneas.
    if state.download_active.swap(true, Ordering::SeqCst) {
        return;
    }
    state.download_cancel.store(false, Ordering::Relaxed);

    let state = Arc::clone(&state);
    std::thread::spawn(move || {
        let mut last_emit = Instant::now();
        let result = model::download_missing(
            &state.models_dir,
            &state.manifest,
            &state.download_cancel,
            |p| {
                // Throttle: emite como mucho ~cada 100 ms, y siempre al final.
                let done = p.downloaded >= p.total;
                if done || last_emit.elapsed().as_millis() >= 100 {
                    last_emit = Instant::now();
                    let percent = if p.total > 0 {
                        (p.downloaded as f64 / p.total as f64) * 100.0
                    } else {
                        100.0
                    };
                    let _ = app.emit(
                        "download://progress",
                        ProgressPayload {
                            downloaded: p.downloaded,
                            total: p.total,
                            percent,
                            file: p.current_file.clone(),
                            file_index: p.file_index,
                            file_count: p.file_count,
                        },
                    );
                }
            },
        );

        state.download_active.store(false, Ordering::SeqCst);

        match result {
            Ok(()) => {
                tracing::info!("descarga de modelos completa");
                let _ = app.emit("download://done", ());
            }
            Err(model::ModelError::Cancelled) => {
                tracing::warn!("descarga cancelada por el usuario");
                let _ = app.emit(
                    "download://error",
                    ErrorPayload {
                        message: "Descarga cancelada.".to_string(),
                    },
                );
            }
            Err(e) => {
                tracing::error!("descarga fallida: {e}");
                let _ = app.emit(
                    "download://error",
                    ErrorPayload {
                        message: e.to_string(),
                    },
                );
            }
        }
    });
}

// --- Control de reproducción y audio (M5); UI en M6/M7) --- //

/// Pausa o reanuda la lectura en curso.
#[tauri::command]
pub fn player_pause_resume(state: State<'_, Arc<AppState>>) {
    state.speech.toggle_pause();
}

/// Detiene la lectura al instante.
#[tauri::command]
pub fn player_stop(state: State<'_, Arc<AppState>>) {
    state.speech.stop();
}

/// Salta al siguiente fragmento.
#[tauri::command]
pub fn player_skip(state: State<'_, Arc<AppState>>) {
    state.speech.skip();
}

/// Ajusta el volumen (0.0–2.0).
#[tauri::command]
pub fn set_volume(state: State<'_, Arc<AppState>>, volume: f32) {
    state.speech.set_volume(volume);
}

/// Lista los dispositivos de salida disponibles.
#[tauri::command]
pub fn list_output_devices(state: State<'_, Arc<AppState>>) -> Vec<String> {
    state.speech.list_output_devices()
}

/// Selecciona el dispositivo de salida (`null` = el predeterminado del sistema).
#[tauri::command]
pub fn set_output_device(state: State<'_, Arc<AppState>>, device: Option<String>) {
    state.speech.set_output_device(device);
}

/// Estado actual para poblar el mini player al abrirse (M7).
#[tauri::command]
pub fn player_state(state: State<'_, Arc<AppState>>) -> crate::player_ui::PlayerState {
    state.speech.player_state()
}

// --- Settings (M6) --- //

/// Voces de estilo disponibles (§6: `M1`..`M5`, `F1`..`F5`).
const VOICES: &[&str] = &["M1", "M2", "M3", "M4", "M5", "F1", "F2", "F3", "F4", "F5"];

/// Estado del motor para la sección Engine.
#[derive(Serialize)]
pub struct EngineStatus {
    pub backend: &'static str,
    pub model_dir: String,
    pub ready: bool,
}

/// Resultado de guardar la configuración.
#[derive(Serialize)]
pub struct SaveResult {
    /// `true` si se escribió el archivo de config.
    pub saved: bool,
    /// Atajos que no se pudieron registrar (conflicto con otra app).
    pub hotkey_conflicts: Vec<RegisterOutcome>,
    /// Mensaje de error de validación (atajo inválido o duplicado); si está
    /// presente, **no** se guardó nada.
    pub error: Option<String>,
}

/// Devuelve la configuración actual para poblar la ventana de Settings.
#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Config {
    state.config_snapshot()
}

/// Lista de voces disponibles.
#[tauri::command]
pub fn list_voices() -> Vec<String> {
    VOICES.iter().map(|v| v.to_string()).collect()
}

/// Versión de la aplicación (de `Cargo.toml`, alineada con `tauri.conf.json`).
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Estado del motor (Backend fijo CPU, ubicación del modelo y si está listo).
#[tauri::command]
pub fn engine_status(state: State<'_, Arc<AppState>>) -> EngineStatus {
    EngineStatus {
        backend: "CPU",
        model_dir: state.models_dir.display().to_string(),
        ready: !state.needs_download(),
    }
}

/// Reproduce una muestra con la voz/idioma/velocidad indicados (botón Preview).
#[tauri::command]
pub fn preview_voice(state: State<'_, Arc<AppState>>, voice: String, lang: String, speed: f32) {
    let sample = "Hola, esta es una prueba de la voz seleccionada.";
    state.speech.preview(&voice, &lang, speed, sample);
}

/// Guarda la configuración: valida atajos, persiste, aplica voz/audio,
/// re-registra atajos (reportando conflictos) y ajusta el arranque con Windows.
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    config: Config,
) -> SaveResult {
    // 1. Validar que las combinaciones de atajo son parseables.
    let hk = match config.to_hotkey_config() {
        Ok(h) => h,
        Err(e) => {
            return SaveResult {
                saved: false,
                hotkey_conflicts: Vec::new(),
                error: Some(e.to_string()),
            }
        }
    };

    // 2. Rechazar atajos duplicados entre acciones.
    if let Some(dup) = first_duplicate_hotkey(&config) {
        return SaveResult {
            saved: false,
            hotkey_conflicts: Vec::new(),
            error: Some(format!(
                "El atajo «{dup}» está asignado a más de una acción."
            )),
        };
    }

    // 3. Persistir a disco.
    let saved = match config.save() {
        Ok(()) => true,
        Err(e) => {
            tracing::error!("no se pudo guardar la config: {e}");
            false
        }
    };

    // 4. Aplicar voz/idioma/velocidad/volumen/dispositivo.
    state.speech.apply_settings(config.to_speech_settings());

    // 5. Re-registrar atajos (conflictos con otras apps se reportan).
    let outcomes = crate::hotkeys::reload(
        &app,
        &hk,
        Arc::clone(&state.speech),
        config.general.show_notifications,
    );

    // 6. Arranque con Windows.
    if let Err(e) = crate::settings::set_start_with_windows(config.general.start_with_windows) {
        tracing::warn!("no se pudo ajustar el arranque con Windows: {e}");
    }

    // 7. Actualizar el estado en memoria.
    *state.config.lock().unwrap_or_else(|p| p.into_inner()) = config;

    SaveResult {
        saved,
        hotkey_conflicts: outcomes.into_iter().filter(|o| o.error.is_some()).collect(),
        error: None,
    }
}

/// Devuelve la primera combinación repetida entre las 4 acciones, si la hay.
fn first_duplicate_hotkey(config: &Config) -> Option<String> {
    let hk = &config.hotkeys;
    let all = [
        &hk.read,
        &hk.read_clipboard,
        &hk.stop,
        &hk.pause_resume,
        &hk.settings,
    ];
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i].eq_ignore_ascii_case(all[j]) {
                return Some(all[i].clone());
            }
        }
    }
    None
}

/// Abre (o enfoca) la ventana de Settings (perezosa; se destruye al cerrar).
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window("settings") {
        // Pudo quedar oculta (la X la minimiza a la bandeja): mostrarla y enfocar.
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("YappyLike — Ajustes")
        .inner_size(560.0, 620.0)
        .min_inner_size(480.0, 480.0)
        .center()
        .build()?;
    Ok(())
}

/// Abre (o enfoca) la ventana de bienvenida de primer arranque.
pub fn open_welcome_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window("welcome") {
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "welcome", WebviewUrl::App("welcome.html".into()))
        .title("Bienvenido a YappyLike")
        .inner_size(500.0, 360.0)
        .resizable(false)
        .center()
        .build()?;
    Ok(())
}
