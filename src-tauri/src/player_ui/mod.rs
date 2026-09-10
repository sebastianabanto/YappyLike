//! Mini player flotante (§6, M7).
//!
//! Ventana diminuta, always-on-top y **no activable** que aparece al empezar a
//! leer y se destruye al terminar. Muestra el estado (`Spanish · M1 · 1.05x`) y
//! los controles de pausa/stop/cerrar.
//!
//! **Requisito no negociable:** no debe robar el foco. Si lo hiciera, rompería
//! la selección de texto del usuario y el flujo entero dejaría de funcionar. En
//! Windows se consigue con `WS_EX_NOACTIVATE` (la ventana nunca se activa, ni al
//! hacer clic en sus botones) más `WS_EX_TOOLWINDOW` (fuera de la barra de
//! tareas y del Alt-Tab). `focused(false)` de Tauri solo evita el foco *inicial*;
//! el estilo extendido es lo que lo garantiza durante toda la vida de la ventana.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Etiqueta estable de la ventana del mini player.
pub const PLAYER_LABEL: &str = "player";

/// Estado que consume la UI del mini player (comando inicial + eventos).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerState {
    /// Línea de estado, p. ej. `Spanish · M1 · 1.05x`.
    pub status: String,
    /// `true` si la reproducción está pausada.
    pub paused: bool,
}

/// Nombre legible del idioma para la línea de estado (ejemplo del prompt en
/// inglés: `Spanish · M1 · 1.05x`). Códigos desconocidos se muestran en mayúsculas.
fn lang_display(lang: &str) -> String {
    match lang {
        "es" => "Spanish".to_string(),
        "en" => "English".to_string(),
        "fr" => "French".to_string(),
        "de" => "German".to_string(),
        "na" | "auto" | "" => "Auto".to_string(),
        other => other.to_uppercase(),
    }
}

/// Construye la línea de estado `Idioma · Voz · Velocidad`.
pub fn status_line(lang: &str, voice: &str, speed: f32) -> String {
    format!("{} · {} · {:.2}x", lang_display(lang), voice, speed)
}

/// Asegura que el mini player está visible y actualizado. Si ya existe, emite un
/// evento de actualización; si no, lo crea. Las operaciones sobre ventanas se
/// hacen en el hilo principal (Tauri lo exige para crear/cerrar de forma segura).
pub fn ensure_shown(app: &AppHandle, state: PlayerState) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if app.get_webview_window(PLAYER_LABEL).is_some() {
            let _ = app.emit_to(PLAYER_LABEL, "player://update", &state);
        } else if let Err(e) = build_window(&app) {
            tracing::error!("no se pudo crear el mini player: {e}");
        }
        // Si acabamos de crearla, la UI pedirá el estado con `player_state` al
        // cargar; no hace falta emitir aquí (evita una carrera con el load).
    });
}

/// Emite una actualización de estado a la ventana (si está abierta). No crea la
/// ventana; es seguro llamarlo desde cualquier hilo.
pub fn update(app: &AppHandle, state: PlayerState) {
    let _ = app.emit_to(PLAYER_LABEL, "player://update", &state);
}

/// Cierra (destruye) el mini player si está abierto. Idempotente.
pub fn hide(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(win) = app.get_webview_window(PLAYER_LABEL) {
            let _ = win.close();
        }
    });
}

/// Crea la ventana del mini player (oculta), la hace no activable, la coloca en
/// la esquina inferior derecha y la muestra sin robar el foco.
fn build_window(app: &AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(
        app,
        PLAYER_LABEL,
        WebviewUrl::App("mini_player.html".into()),
    )
    .title("YappyLike")
    .inner_size(272.0, 100.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()?;

    make_non_activating(&win);
    position_bottom_right(app, &win);
    // Mostrar tras posicionar y aplicar el estilo: con WS_EX_NOACTIVATE ya
    // puesto, mostrarla no la activa.
    let _ = win.show();
    Ok(())
}

/// Coloca la ventana en la esquina inferior derecha del monitor, con un margen y
/// un hueco aproximado para la barra de tareas. Best-effort: si algo falla, la
/// ventana se queda donde la puso el gestor (el usuario puede moverla).
fn position_bottom_right(app: &AppHandle, win: &WebviewWindow) {
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let Ok(outer) = win.outer_size() else {
        return;
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let origin = monitor.position();
    let margin = (16.0 * scale).round() as i32;
    // Hueco aproximado de la barra de tareas (≈48 px a escala 100%).
    let taskbar = (48.0 * scale).round() as i32;
    let x = origin.x + size.width as i32 - outer.width as i32 - margin;
    let y = origin.y + size.height as i32 - outer.height as i32 - margin - taskbar;
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Aplica `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` al HWND de la ventana para que
/// nunca robe el foco ni aparezca en la barra de tareas / Alt-Tab.
#[cfg(windows)]
fn make_non_activating(win: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let hwnd = match win.hwnd() {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("no se pudo obtener el HWND del mini player: {e}");
            return;
        }
    };
    // SAFETY: `hwnd` es un handle válido recién creado por Tauri; Get/SetWindowLongPtr
    // sobre GWL_EXSTYLE es una operación estándar y admitida entre hilos.
    unsafe {
        let hwnd = HWND(hwnd.0);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new = ex | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
    }
}

/// Stub no-Windows (la app es Windows-only; permite compilar/testear en otros SO).
#[cfg(not(windows))]
fn make_non_activating(_win: &WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linea_de_estado_formato_ejemplo() {
        assert_eq!(status_line("es", "M1", 1.05), "Spanish · M1 · 1.05x");
        assert_eq!(status_line("en", "F3", 1.2), "English · F3 · 1.20x");
    }

    #[test]
    fn idioma_agnostico_y_desconocido() {
        assert_eq!(status_line("na", "M1", 1.0), "Auto · M1 · 1.00x");
        assert_eq!(status_line("auto", "M1", 1.0), "Auto · M1 · 1.00x");
        assert_eq!(status_line("it", "M1", 1.0), "IT · M1 · 1.00x");
    }
}
