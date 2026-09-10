//! System tray: icono, menú y enrutado de eventos.
//!
//! YappyLike vive **solo** en la bandeja (sin ventana al arrancar). El menú
//! expone las acciones principales; en M1 las de lectura/pausa/stop/settings
//! quedan cableadas y registran su invocación — la lógica real llega en los
//! hitos M2–M7.

use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use thiserror::Error;

use crate::commands::AppState;

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("error de Tauri al construir el tray: {0}")]
    Tauri(#[from] tauri::Error),
}

pub type Result<T> = std::result::Result<T, TrayError>;

/// Identificador estable del icono de bandeja.
pub const TRAY_ID: &str = "main";

/// Construye el icono de bandeja con su menú y registra los manejadores.
pub fn setup(app: &AppHandle) -> Result<()> {
    let read = MenuItem::with_id(app, "read", "Read selection", true, None::<&str>)?;
    let read_clipboard =
        MenuItem::with_id(app, "read_clipboard", "Read clipboard", true, None::<&str>)?;
    let export = MenuItem::with_id(
        app,
        "export_wav",
        "Save clipboard as audio…",
        true,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(app, "pause", "Pause / Resume", true, None::<&str>)?;
    let skip = MenuItem::with_id(app, "skip", "Skip", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &read,
            &read_clipboard,
            &export,
            &pause,
            &skip,
            &stop,
            &settings,
            &sep,
            &exit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("YappyLike")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event.id.as_ref()));

    // Reutiliza el icono embebido de la app (formato correcto garantizado).
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;

    tracing::info!("tray construido (id={TRAY_ID})");
    Ok(())
}

/// Enruta las selecciones del menú de bandeja.
fn on_menu_event(app: &AppHandle, id: &str) {
    // Acciones de reproducción: necesitan el servicio de voz del estado global.
    let speech = || app.state::<Arc<AppState>>().speech.clone();

    match id {
        "read" => {
            tracing::info!("menú: Read selection");
            crate::hotkeys::read_selection_async(speech());
        }
        "read_clipboard" => {
            tracing::info!("menú: Read clipboard");
            crate::hotkeys::read_clipboard_async(speech());
        }
        "export_wav" => {
            tracing::info!("menú: Save clipboard as audio");
            let state = Arc::clone(&app.state::<Arc<AppState>>());
            crate::export::export_clipboard_async(app.clone(), state);
        }
        "pause" => {
            tracing::info!("menú: Pause/Resume");
            speech().toggle_pause();
        }
        "skip" => {
            tracing::info!("menú: Skip");
            speech().skip();
        }
        "stop" => {
            tracing::info!("menú: Stop");
            speech().stop();
        }
        "settings" => {
            tracing::info!("menú: Settings");
            if let Err(e) = crate::commands::open_settings_window(app) {
                tracing::error!("no se pudo abrir la ventana de Settings: {e}");
            }
        }
        "exit" => {
            tracing::info!("menú: Exit — cerrando aplicación");
            app.exit(0);
        }
        other => {
            tracing::warn!("menú: id desconocido '{other}'");
        }
    }
}
