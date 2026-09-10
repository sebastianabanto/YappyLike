//! Gestión de los assets del modelo (§3): manifiesto con tamaño y SHA-256,
//! descarga por HTTPS con verificación de hash **al vuelo**, reintento único y
//! cancelación. Los archivos se descargan **una sola vez**: si ya están en
//! disco con el tamaño correcto, no se vuelven a bajar.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("error de descarga de {url}: {source}")]
    Http {
        url: String,
        source: Box<ureq::Error>,
    },
    #[error("el hash de {file} no coincide tras reintentar")]
    HashMismatch { file: String },
    #[error("descarga cancelada por el usuario")]
    Cancelled,
    #[error("manifiesto inválido: {0}")]
    Manifest(String),
}

pub type Result<T> = std::result::Result<T, ModelError>;

/// Manifiesto de assets, embebido en el binario.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub base_url: String,
    pub approx_total_mb: u64,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    /// Ruta relativa dentro de `models/` y sufijo de la URL de descarga.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

impl Manifest {
    /// Carga el manifiesto embebido (`assets_manifest.json`).
    pub fn embedded() -> Result<Self> {
        serde_json::from_str(include_str!("../../assets_manifest.json"))
            .map_err(|e| ModelError::Manifest(e.to_string()))
    }

    /// Suma de tamaños de todos los assets, en bytes.
    pub fn total_bytes(&self) -> u64 {
        self.assets.iter().map(|a| a.size).sum()
    }
}

/// Progreso de descarga informado por [`download_missing`].
#[derive(Debug, Clone)]
pub struct Progress {
    /// Índice (0-based) del archivo en curso dentro de los que faltan.
    pub file_index: usize,
    /// Cantidad de archivos que faltan por descargar.
    pub file_count: usize,
    /// Ruta del archivo en curso.
    pub current_file: String,
    /// Bytes descargados acumulados sobre el total pendiente.
    pub downloaded: u64,
    /// Total de bytes a descargar (solo los que faltan).
    pub total: u64,
}

/// `true` si el asset ya está en disco con el tamaño esperado.
fn is_present(models_dir: &Path, asset: &Asset) -> bool {
    let path = models_dir.join(&asset.path);
    fs::metadata(&path)
        .map(|m| m.len() == asset.size)
        .unwrap_or(false)
}

/// `true` si **todos** los assets están presentes (tamaño correcto).
pub fn all_present(models_dir: &Path, manifest: &Manifest) -> bool {
    manifest.assets.iter().all(|a| is_present(models_dir, a))
}

/// Assets que faltan (o tienen tamaño incorrecto) por descargar.
pub fn missing<'a>(models_dir: &Path, manifest: &'a Manifest) -> Vec<&'a Asset> {
    manifest
        .assets
        .iter()
        .filter(|a| !is_present(models_dir, a))
        .collect()
}

/// Descarga los assets que falten, verificando el SHA-256 de cada uno. Reintenta
/// una vez ante un hash incorrecto. Llama a `on_progress` con frecuencia y
/// aborta pronto si `cancel` se activa.
pub fn download_missing<F: FnMut(&Progress)>(
    models_dir: &Path,
    manifest: &Manifest,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<()> {
    let pending = missing(models_dir, manifest);
    let total: u64 = pending.iter().map(|a| a.size).sum();
    let file_count = pending.len();
    let mut downloaded_total: u64 = 0;

    for (file_index, asset) in pending.iter().enumerate() {
        let mut attempt = 0;
        loop {
            let base_downloaded = downloaded_total;
            let result = download_one(manifest, models_dir, asset, cancel, &mut |file_done| {
                let p = Progress {
                    file_index,
                    file_count,
                    current_file: asset.path.clone(),
                    downloaded: base_downloaded + file_done,
                    total,
                };
                on_progress(&p);
            });

            match result {
                Ok(()) => {
                    downloaded_total += asset.size;
                    break;
                }
                Err(ModelError::HashMismatch { .. }) if attempt < 1 => {
                    attempt += 1;
                    tracing::warn!("hash incorrecto en {}, reintentando", asset.path);
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(())
}

/// Descarga un asset a `<dest>.part`, verificando el hash mientras se escribe, y
/// lo mueve a su sitio si coincide.
fn download_one<F: FnMut(u64)>(
    manifest: &Manifest,
    models_dir: &Path,
    asset: &Asset,
    cancel: &AtomicBool,
    on_file_progress: &mut F,
) -> Result<()> {
    let url = format!("{}/{}", manifest.base_url, asset.path);
    let dest = models_dir.join(&asset.path);
    let part = PathBuf::from(format!("{}.part", dest.display()));

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let response = ureq::get(&url).call().map_err(|e| ModelError::Http {
        url: url.clone(),
        source: Box::new(e),
    })?;

    let mut reader = response.into_reader();
    let mut out = BufWriter::new(File::create(&part)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = fs::remove_file(&part);
            return Err(ModelError::Cancelled);
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        on_file_progress(done);
    }
    out.flush()?;
    drop(out);

    let got = to_hex(&hasher.finalize());
    if got != asset.sha256 {
        let _ = fs::remove_file(&part);
        return Err(ModelError::HashMismatch {
            file: asset.path.clone(),
        });
    }

    fs::rename(&part, &dest)?;
    Ok(())
}

/// Verifica el SHA-256 de un archivo ya en disco contra el esperado.
pub fn verify_file_hash(path: &Path, expected_sha256: &str) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()) == expected_sha256)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifiesto_embebido_es_valido() {
        let m = Manifest::embedded().expect("manifiesto");
        assert_eq!(m.version, 1);
        assert!(m.base_url.starts_with("https://"));
        // 6 archivos onnx/aux + 10 voces = 16 assets.
        assert_eq!(m.assets.len(), 16);
        // Cada hash es un SHA-256 en hex (64 chars).
        for a in &m.assets {
            assert_eq!(a.sha256.len(), 64, "hash de {}", a.path);
            assert!(a.size > 0);
        }
        assert!(m.total_bytes() > 350_000_000);
    }

    #[test]
    fn to_hex_formatea_con_ceros() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff, 0xa0]), "000fffa0");
    }

    #[test]
    fn verifica_hash_de_archivo_temporal() {
        let dir = std::env::temp_dir().join(format!("yl_modeltest_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let f = dir.join("hola.txt");
        fs::write(&f, b"hola").unwrap();
        // SHA-256 de "hola".
        let expected = "b221d9dbb083a7f33428d7c2a3c3198ae925614d70210e28716ccaa7cd4ddb79";
        assert!(verify_file_hash(&f, expected).unwrap());
        assert!(!verify_file_hash(&f, &"0".repeat(64)).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }
}
