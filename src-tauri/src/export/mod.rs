//! Exportación de la lectura a un archivo **WAV** (feature #2).
//!
//! A diferencia de la reproducción por streaming ([`crate::speech`]), aquí se
//! sintetiza **todo** el texto, se **concatena** en un solo buffer (con el mismo
//! silencio entre fragmentos que la lectura) y se escribe un WAV PCM 16-bit mono
//! **sin dependencias externas**.
//!
//! El destino es una **carpeta configurable** (Ajustes → Audio); si está vacía,
//! se usa la de por defecto: `Documentos\YappyLike` (o `<portable>\exports` en
//! modo portable). El nombre lleva marca de tiempo para no pisar exportaciones
//! previas.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::capture::SelectionCapture;
use crate::commands::AppState;

/// Carpeta de exportación por defecto: `<portable>\exports` en modo portable, si
/// no `%USERPROFILE%\Documents\YappyLike`.
pub fn default_export_dir() -> PathBuf {
    if let Some(root) = crate::settings::portable_root() {
        return root.join("exports");
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile).join("Documents").join("YappyLike");
    }
    PathBuf::from(".").join("YappyLike")
}

/// Resuelve la carpeta de destino: la configurada por el usuario si no está en
/// blanco, o la de por defecto.
pub fn resolve_export_dir(configured: Option<&str>) -> PathBuf {
    match configured {
        Some(s) if !s.trim().is_empty() => PathBuf::from(s.trim()),
        _ => default_export_dir(),
    }
}

/// Construye la ruta de salida `<dir>\YappyLike-<AAAAMMDD-HHMMSS>.wav`.
pub fn build_output_path(dir: &Path) -> PathBuf {
    dir.join(format!("YappyLike-{}.wav", timestamp_for_filename()))
}

/// Escribe `samples` (f32 en `[-1, 1]`) como WAV **PCM 16-bit mono**.
pub fn write_wav_pcm16(path: &Path, samples: &[f32], sample_rate: u32) -> io::Result<()> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2;

    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits per sample
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        let val = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_all(&val.to_le_bytes())?;
    }
    w.flush()
}

/// Exporta a WAV el texto **del portapapeles** en un hilo aparte (la síntesis
/// del texto completo puede tardar varios segundos). Se lee el portapapeles en
/// vez de la selección porque el disparador es el menú de bandeja: al hacer clic
/// ahí el foco ya no está en el texto seleccionado, así que un `Ctrl+C` no
/// capturaría nada. Notifica el resultado si las notificaciones están activas.
pub fn export_clipboard_async(app: AppHandle, state: Arc<AppState>) {
    std::thread::spawn(move || {
        let capture = SelectionCapture::system();
        let text = match capture.clipboard_text().filter(|t| !t.trim().is_empty()) {
            Some(t) => t,
            None => {
                tracing::info!("exportar: portapapeles sin texto");
                notify(
                    &app,
                    &state,
                    "YappyLike",
                    "No hay texto en el portapapeles para exportar. Copia algo primero (Ctrl+C).",
                );
                return;
            }
        };

        let cfg = state.config_snapshot();
        let dir = resolve_export_dir(cfg.export.dir.as_deref());
        tracing::info!(
            "exportar: {} caracteres → {}",
            text.chars().count(),
            dir.display()
        );

        match state.speech.export_to_wav(&text, &dir) {
            Ok(path) => {
                tracing::info!("exportar: audio guardado en {}", path.display());
                notify(&app, &state, "Audio guardado", &path.display().to_string());
            }
            Err(e) => {
                tracing::error!("exportar: fallo: {e}");
                notify(&app, &state, "Error al exportar", &e);
            }
        }
    });
}

/// Muestra una notificación si el usuario las tiene activas.
fn notify(app: &AppHandle, state: &Arc<AppState>, title: &str, body: &str) {
    if !state.config_snapshot().general.show_notifications {
        return;
    }
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("no se pudo mostrar la notificación de exportación: {e}");
    }
}

/// Marca de tiempo UTC `AAAAMMDD-HHMMSS` para nombres de archivo (sin
/// dependencias: sortable e inequívoca).
fn timestamp_for_filename() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// Convierte días desde 1970-01-01 en `(año, mes, día)` (algoritmo de Howard
/// Hinnant, calendario gregoriano proléptico).
fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch_y_fechas_conocidas() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-01-01 = 10957 días tras epoch.
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        // 2021-01-01 = 18628 días tras epoch.
        assert_eq!(civil_from_days(18_628), (2021, 1, 1));
    }

    #[test]
    fn write_wav_cabecera_y_datos_correctos() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("yappylike_export_test_{}.wav", std::process::id()));
        // 3 muestras a 24 kHz: extremos y silencio.
        let samples = [1.0f32, -1.0, 0.0];
        write_wav_pcm16(&path, &samples, 24_000).expect("escribir wav");

        let bytes = std::fs::read(&path).expect("leer wav");
        // 44 bytes de cabecera + 2 por muestra.
        assert_eq!(bytes.len(), 44 + samples.len() * 2);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[36..40], b"data");
        // sample_rate en offset 24 (LE).
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            24_000
        );
        // Primera muestra = 1.0 → 32767 (LE).
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), 32767);
        // Segunda = -1.0 → -32767.
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), -32767);
        // Tercera = 0.0 → 0.
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resolve_export_dir_usa_default_si_vacio() {
        assert_eq!(resolve_export_dir(None), default_export_dir());
        assert_eq!(resolve_export_dir(Some("   ")), default_export_dir());
        assert_eq!(
            resolve_export_dir(Some("C:/tmp/x")),
            PathBuf::from("C:/tmp/x")
        );
    }
}
