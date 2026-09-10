//! YappyLike — biblioteca principal.
//!
//! Aquí vive únicamente el *wiring* de arranque (plugins, estado, tray, atajos,
//! logging). La lógica de negocio se reparte en los módulos (`capture`, `tts`,
//! `audio`, `model`, `speech`, `hotkeys`, `tray`, `logging`).

pub mod audio;
pub mod capture;
mod chunking;
mod commands;
mod export;
mod hotkeys;
mod logging;
pub mod model;
mod player_ui;
mod replacements;
mod settings;
mod speech;
mod tray;
pub mod tts;

use std::sync::Arc;

use tauri::Manager;

use commands::AppState;
use speech::SpeechService;
use tts::TtsEngine;

/// Punto de entrada de la aplicación residente.
///
/// Arranca **sin ninguna ventana** (solo tray). En el primer arranque, si faltan
/// los modelos, abre la ventana Welcome para descargarlos.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // El guard debe vivir toda la ejecución para hacer flush de los logs.
    let _log_guard = match logging::init() {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!("YappyLike: no se pudo iniciar el logging: {e}");
            None
        }
    };

    tauri::Builder::default()
        // Debe ser el primer plugin registrado (recomendación oficial).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tracing::warn!("Segunda instancia detectada; se ignora y termina.");
            if let Some(tray) = app.tray_by_id(tray::TRAY_ID) {
                let _ = tray.set_tooltip(Some("YappyLike ya está en ejecución"));
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        // La X de la ventana de Ajustes la **oculta** (se minimiza a la bandeja)
        // en vez de cerrarla; así el usuario la recupera desde el tray y no se
        // pierde estado. La app solo se cierra desde el menú Exit de la bandeja.
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::download_info,
            commands::start_download,
            commands::cancel_download,
            commands::player_pause_resume,
            commands::player_stop,
            commands::player_skip,
            commands::set_volume,
            commands::list_output_devices,
            commands::set_output_device,
            commands::player_state,
            commands::get_settings,
            commands::save_settings,
            commands::preview_voice,
            commands::list_voices,
            commands::engine_status,
            commands::app_version,
        ])
        .setup(|app| {
            let handle = app.handle();

            // Configuración persistente (§6): se carga tolerante a defaults.
            let config = settings::Config::load();

            // Estado compartido: modelos, manifiesto y servicio de voz.
            let models_dir = tts::default_models_dir()
                .map_err(|e| format!("no se pudo determinar el directorio de modelos: {e}"))?;
            let manifest = model::Manifest::embedded()
                .map_err(|e| format!("manifiesto de modelos inválido: {e}"))?;
            let engine = Arc::new(TtsEngine::new(models_dir.clone()));
            let speech = Arc::new(SpeechService::new(engine));
            // Handle para gestionar el mini player (M7).
            speech.set_app_handle(handle.clone());
            // Aplica voz/idioma/velocidad/volumen/dispositivo de la config.
            speech.apply_settings(config.to_speech_settings());

            let notify_enabled = config.general.show_notifications;
            // Atajos de la config (si el usuario dejó una combinación inválida a
            // mano, se recurre a los defaults sin abortar).
            let hotkey_config = config.to_hotkey_config().unwrap_or_else(|e| {
                tracing::warn!("atajos de la config inválidos ({e}); usando defaults");
                hotkeys::HotkeyConfig::default()
            });

            // Arranque con Windows según la config.
            if let Err(e) = settings::set_start_with_windows(config.general.start_with_windows) {
                tracing::warn!("no se pudo ajustar el arranque con Windows: {e}");
            }

            let state = Arc::new(AppState::new(models_dir, manifest, speech, config));
            app.manage(Arc::clone(&state));

            tray::setup(handle)?;
            hotkeys::setup(
                handle,
                &hotkey_config,
                Arc::clone(&state.speech),
                notify_enabled,
            )?;

            // Primer arranque: si faltan modelos, abre la ventana de bienvenida.
            if state.needs_download() {
                tracing::info!("faltan modelos; abriendo ventana de bienvenida");
                if let Err(e) = commands::open_welcome_window(handle) {
                    tracing::error!("no se pudo abrir la ventana Welcome: {e}");
                }
            } else {
                tracing::info!("modelos presentes; listo para leer");
            }

            tracing::info!("YappyLike listo (tray-only).");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error irrecuperable al construir la aplicación Tauri")
        .run(|_app, event| {
            // La app es **residente en la bandeja**: cerrar su última ventana
            // (p. ej. el mini player al terminar de leer, o Welcome) NO debe
            // cerrar la app. `ExitRequested` con `code: None` es justo ese caso
            // ("última ventana cerrada") ⇒ se impide salir. La salida explícita
            // del menú Exit usa `app.exit(code)` (lleva `code: Some`) y sí sale.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
