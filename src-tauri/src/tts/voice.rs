//! Configuración del modelo (`tts.json`) y voces de estilo (JSON `M1..F5`).
//!
//! Port de las estructuras y cargadores de `helper.rs`. Solo se deserializan los
//! campos que el pipeline necesita; `serde` ignora el resto del JSON.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use ndarray::Array3;
use serde::Deserialize;

use super::{Result, TtsError};

/// Configuración global del modelo (subconjunto de `tts.json`).
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub ae: AeConfig,
    pub ttl: TtlConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AeConfig {
    pub sample_rate: i32,
    pub base_chunk_size: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtlConfig {
    pub chunk_compress_factor: i32,
    pub latent_dim: i32,
}

/// Carga `tts.json` desde el directorio de modelos ONNX.
pub fn load_config(onnx_dir: &Path) -> Result<Config> {
    let path = onnx_dir.join("tts.json");
    read_json(&path)
}

// --- Voces de estilo --- //

#[derive(Debug, Clone, Deserialize)]
struct VoiceStyleData {
    style_ttl: StyleComponent,
    style_dp: StyleComponent,
}

#[derive(Debug, Clone, Deserialize)]
struct StyleComponent {
    data: Vec<Vec<Vec<f32>>>,
    dims: Vec<usize>,
}

/// Tensores de estilo de una voz, con `bsz = 1`.
pub struct Style {
    pub ttl: Array3<f32>,
    pub dp: Array3<f32>,
}

/// Carga una voz de estilo (un único archivo) como tensores `[1, d1, d2]`.
pub fn load_voice_style(path: &Path) -> Result<Style> {
    let data: VoiceStyleData = read_json(path)?;

    let ttl = component_to_array(&data.style_ttl, path)?;
    let dp = component_to_array(&data.style_dp, path)?;

    Ok(Style { ttl, dp })
}

/// Aplana un `StyleComponent` a un `Array3<f32>` de forma `[1, dims[1], dims[2]]`.
fn component_to_array(c: &StyleComponent, path: &Path) -> Result<Array3<f32>> {
    if c.dims.len() != 3 {
        return Err(TtsError::Other(format!(
            "voz {} con dims inesperadas: {:?}",
            path.display(),
            c.dims
        )));
    }
    let (d1, d2) = (c.dims[1], c.dims[2]);
    let mut flat = Vec::with_capacity(d1 * d2);
    for batch in &c.data {
        for row in batch {
            flat.extend_from_slice(row);
        }
    }
    Array3::from_shape_vec((1, d1, d2), flat).map_err(TtsError::from)
}

/// Lee y deserializa un JSON, adjuntando la ruta a los errores.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).map_err(|source| TtsError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(|source| TtsError::Json {
        path: path.display().to_string(),
        source,
    })
}
