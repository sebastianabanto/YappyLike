//! Motor TTS local (Supertonic 3). Fachada de **carga perezosa**: las cuatro
//! sesiones ONNX se cargan en la primera síntesis y quedan cacheadas en memoria
//! (§4). Las voces se cargan bajo demanda y se cachean por nombre.
//!
//! El pipeline es un port del ejemplo oficial en Rust (ver [`synth`], [`text`],
//! [`voice`]).

mod synth;
mod text;
mod voice;

pub use synth::{CHUNK_SILENCE_SECS, DEFAULT_TOTAL_STEP};
pub use text::{is_valid_lang, AVAILABLE_LANGS};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use thiserror::Error;

use synth::TextToSpeech;
use voice::Style;

#[derive(Debug, Error)]
pub enum TtsError {
    #[error("E/S en {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("JSON inválido en {path}: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
    #[error("idioma no soportado: {0}")]
    InvalidLang(String),
    #[error("ONNX Runtime: {0}")]
    Ort(#[from] ort::Error),
    #[error("forma de tensor inválida: {0}")]
    Shape(#[from] ndarray::ShapeError),
    #[error("faltan archivos de modelo en {0}")]
    ModelsMissing(String),
    #[error("el motor TTS está envenenado por un pánico anterior")]
    Poisoned,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, TtsError>;

/// Archivos ONNX y auxiliares que el motor necesita bajo `models/onnx`.
pub const REQUIRED_ONNX_FILES: &[&str] = &[
    "duration_predictor.onnx",
    "text_encoder.onnx",
    "vector_estimator.onnx",
    "vocoder.onnx",
    "tts.json",
    "unicode_indexer.json",
];

/// Voz por defecto (§4/§6: voces `M1..F5`).
pub const DEFAULT_VOICE: &str = "M1";

/// Resultado de una síntesis lista para reproducir.
pub struct Synth {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub duration_secs: f32,
}

struct EngineState {
    tts: Option<TextToSpeech>,
    voices: HashMap<String, Arc<Style>>,
}

/// Fachada del motor TTS con carga perezosa y cachés en memoria.
pub struct TtsEngine {
    models_dir: PathBuf,
    state: Mutex<EngineState>,
}

impl TtsEngine {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            state: Mutex::new(EngineState {
                tts: None,
                voices: HashMap::new(),
            }),
        }
    }

    /// Directorio de las sesiones ONNX y assets (`models/onnx`).
    pub fn onnx_dir(&self) -> PathBuf {
        self.models_dir.join("onnx")
    }

    /// Directorio de las voces de estilo (`models/voice_styles`).
    pub fn voices_dir(&self) -> PathBuf {
        self.models_dir.join("voice_styles")
    }

    /// `true` si todos los archivos requeridos existen en disco.
    pub fn models_present(&self) -> bool {
        let onnx = self.onnx_dir();
        REQUIRED_ONNX_FILES.iter().all(|f| onnx.join(f).is_file())
            && self
                .voices_dir()
                .join(format!("{DEFAULT_VOICE}.json"))
                .is_file()
    }

    /// Sintetiza `text` en `lang` con la voz `voice`. Carga el modelo y la voz
    /// de forma perezosa la primera vez. Bloquea hasta terminar la síntesis.
    pub fn synthesize(&self, text: &str, lang: &str, voice: &str, speed: f32) -> Result<Synth> {
        if !self.models_present() {
            return Err(TtsError::ModelsMissing(
                self.models_dir.display().to_string(),
            ));
        }

        let mut state = self.state.lock().map_err(|_| TtsError::Poisoned)?;

        if state.tts.is_none() {
            tracing::info!("cargando modelo TTS (primera síntesis)…");
            let tts = TextToSpeech::load(&self.onnx_dir())?;
            state.tts = Some(tts);
            tracing::info!("modelo TTS cargado y cacheado en memoria");
        }

        let style = self.load_voice_cached(&mut state, voice)?;

        let tts = state.tts.as_ref().expect("cargado justo arriba");
        let (samples, duration_secs) = tts.synthesize(
            text,
            lang,
            &style,
            DEFAULT_TOTAL_STEP,
            speed,
            CHUNK_SILENCE_SECS,
        )?;
        let sample_rate = tts.sample_rate as u32;

        Ok(Synth {
            samples,
            sample_rate,
            duration_secs,
        })
    }

    /// Sintetiza **un fragmento** ya troceado (streaming de M5). Carga el modelo
    /// y la voz de forma perezosa. Serializa con el resto de síntesis (una sola
    /// sesión ONNX), por lo que una lectura nueva espera, a lo sumo, a que
    /// termine el fragmento en curso.
    pub fn synthesize_chunk(
        &self,
        chunk: &str,
        lang: &str,
        voice: &str,
        speed: f32,
    ) -> Result<Synth> {
        self.synthesize_chunk_steps(chunk, lang, voice, speed, DEFAULT_TOTAL_STEP)
    }

    /// Como [`Self::synthesize_chunk`] pero con un número de pasos de denoising
    /// explícito. Es la palanca **calidad ↔ velocidad** de Supertonic-3: menos
    /// pasos = más rápido y algo más áspero; más pasos = más limpio y lento
    /// (`DEFAULT_TOTAL_STEP` = 8, el del ejemplo oficial).
    pub fn synthesize_chunk_steps(
        &self,
        chunk: &str,
        lang: &str,
        voice: &str,
        speed: f32,
        total_step: usize,
    ) -> Result<Synth> {
        if !self.models_present() {
            return Err(TtsError::ModelsMissing(
                self.models_dir.display().to_string(),
            ));
        }

        let mut state = self.state.lock().map_err(|_| TtsError::Poisoned)?;

        if state.tts.is_none() {
            tracing::info!("cargando modelo TTS (primera síntesis)…");
            let tts = TextToSpeech::load(&self.onnx_dir())?;
            state.tts = Some(tts);
            tracing::info!("modelo TTS cargado y cacheado en memoria");
        }

        let style = self.load_voice_cached(&mut state, voice)?;
        let tts = state.tts.as_ref().expect("cargado justo arriba");
        let (samples, duration_secs) =
            tts.synthesize_chunk(chunk, lang, &style, total_step, speed)?;
        let sample_rate = tts.sample_rate as u32;

        Ok(Synth {
            samples,
            sample_rate,
            duration_secs,
        })
    }

    /// Devuelve la voz `name` desde la caché, cargándola si hace falta.
    fn load_voice_cached(&self, state: &mut EngineState, name: &str) -> Result<Arc<Style>> {
        if let Some(style) = state.voices.get(name) {
            return Ok(Arc::clone(style));
        }
        let path = self.voices_dir().join(format!("{name}.json"));
        let style = Arc::new(voice::load_voice_style(&path)?);
        state.voices.insert(name.to_string(), Arc::clone(&style));
        Ok(style)
    }
}

/// Ruta del directorio de modelos: `<dir_exe>\models` en modo portable (M8), si
/// no `%APPDATA%\YappyLike\models`.
pub fn default_models_dir() -> Result<PathBuf> {
    if let Some(root) = crate::settings::portable_root() {
        return Ok(root.join("models"));
    }
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| TtsError::Other("no se pudo determinar %APPDATA%".to_string()))?;
    Ok(Path::new(&appdata).join("YappyLike").join("models"))
}
