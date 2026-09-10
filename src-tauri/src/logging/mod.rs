//! Inicialización de logging con `tracing`.
//!
//! Escribe a `%APPDATA%\YappyLike\logs\` con rotación **diaria** y un tope
//! total de 10 MB (§8 del prompt): al arrancar se podan los archivos más
//! antiguos hasta quedar por debajo del límite.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Tope total del directorio de logs, en bytes (10 MB).
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Prefijo de los archivos de log rotados diariamente.
const LOG_PREFIX: &str = "yappylike.log";

#[derive(Debug, Error)]
pub enum LogError {
    #[error("no se pudo determinar %APPDATA%")]
    NoAppData,
    #[error("error de E/S en el directorio de logs: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LogError>;

/// Devuelve `<dir_exe>\logs` en modo portable (M8), si no
/// `%APPDATA%\YappyLike\logs`, creándolo si no existe.
pub fn logs_dir() -> Result<PathBuf> {
    let dir = if let Some(root) = crate::settings::portable_root() {
        root.join("logs")
    } else {
        let appdata = std::env::var_os("APPDATA").ok_or(LogError::NoAppData)?;
        PathBuf::from(appdata).join("YappyLike").join("logs")
    };
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Inicializa el subscriber global de `tracing`.
///
/// Devuelve un [`WorkerGuard`] que **debe mantenerse vivo** durante toda la
/// vida del proceso; al soltarse, hace flush de los logs pendientes.
pub fn init() -> Result<WorkerGuard> {
    let dir = logs_dir()?;

    // Poda antes de empezar a escribir para respetar el tope de 10 MB.
    if let Err(e) = prune_to_cap(&dir, MAX_LOG_BYTES) {
        // No es fatal: seguimos, pero lo dejamos anotado en stderr.
        eprintln!("YappyLike: no se pudo podar logs antiguos: {e}");
    }

    let file_appender = tracing_appender::rolling::daily(&dir, LOG_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Nivel por defecto `info`; sobreescribible con la env var YAPPYLIKE_LOG.
    let filter =
        EnvFilter::try_from_env("YAPPYLIKE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(non_blocking)
        .init();

    tracing::info!("logging inicializado en {}", dir.display());
    Ok(guard)
}

/// Elimina los archivos de log más antiguos hasta que el tamaño total del
/// directorio quede por debajo de `cap_bytes`.
fn prune_to_cap(dir: &Path, cap_bytes: u64) -> std::io::Result<()> {
    let mut entries: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = match entry.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        // Solo tocamos nuestros propios logs.
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(LOG_PREFIX) {
            continue;
        }
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        entries.push((entry.path(), meta.len(), modified));
    }

    let mut total: u64 = entries.iter().map(|(_, len, _)| *len).sum();
    if total <= cap_bytes {
        return Ok(());
    }

    // Más antiguos primero.
    entries.sort_by_key(|(_, _, modified)| *modified);

    for (path, len, _) in entries {
        if total <= cap_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(dir: &Path, name: &str, bytes: usize) {
        let mut f = fs::File::create(dir.join(name)).unwrap();
        f.write_all(&vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn prune_deja_directorio_bajo_el_tope() {
        let tmp = std::env::temp_dir().join(format!("yl_logtest_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // 3 archivos de 1 KB con nuestro prefijo, tope de 2 KB.
        write_file(&tmp, "yappylike.log.2026-01-01", 1024);
        write_file(&tmp, "yappylike.log.2026-01-02", 1024);
        write_file(&tmp, "yappylike.log.2026-01-03", 1024);

        prune_to_cap(&tmp, 2048).unwrap();

        let total: u64 = fs::read_dir(&tmp)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.metadata().unwrap().len())
            .sum();
        assert!(total <= 2048, "total={total} debería ser <= 2048");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn prune_no_toca_archivos_ajenos() {
        let tmp = std::env::temp_dir().join(format!("yl_logtest2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        write_file(&tmp, "otro.txt", 4096);
        write_file(&tmp, "yappylike.log.2026-01-01", 4096);

        prune_to_cap(&tmp, 0).unwrap();

        // El archivo ajeno sigue ahí; el nuestro no.
        assert!(tmp.join("otro.txt").exists());
        assert!(!tmp.join("yappylike.log.2026-01-01").exists());

        let _ = fs::remove_dir_all(&tmp);
    }
}
