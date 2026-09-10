//! Pipeline de inferencia: 4 sesiones ONNX (duration predictor, text encoder,
//! vector estimator y vocoder). Port de `TextToSpeech` de `helper.rs`, adaptado
//! a la API de `ort` 2.0.0-rc.7 (`run(inputs!{…}?)`, `try_extract_raw_tensor`).

use std::path::Path;

use ndarray::{Array, Array1, Array2, Array3};
use ort::Session;
use rand_distr::{Distribution, Normal};

use super::text::UnicodeProcessor;
use super::voice::{self, Config, Style};
use super::Result;
use crate::chunking::{chunk_text, max_len_for_lang};

/// Pasos de denoising por defecto (igual que el ejemplo oficial).
pub const DEFAULT_TOTAL_STEP: usize = 8;
/// Silencio insertado entre fragmentos, en segundos.
pub const CHUNK_SILENCE_SECS: f32 = 0.3;

/// Motor de inferencia con las cuatro sesiones ONNX cargadas.
pub struct TextToSpeech {
    cfgs: Config,
    text_processor: UnicodeProcessor,
    dp_ort: Session,
    text_enc_ort: Session,
    vector_est_ort: Session,
    vocoder_ort: Session,
    pub sample_rate: i32,
}

impl TextToSpeech {
    /// Carga configuración, indexador y las cuatro sesiones desde `onnx_dir`.
    pub fn load(onnx_dir: &Path) -> Result<Self> {
        let cfgs = voice::load_config(onnx_dir)?;
        let sample_rate = cfgs.ae.sample_rate;

        let dp_ort =
            Session::builder()?.commit_from_file(onnx_dir.join("duration_predictor.onnx"))?;
        let text_enc_ort =
            Session::builder()?.commit_from_file(onnx_dir.join("text_encoder.onnx"))?;
        let vector_est_ort =
            Session::builder()?.commit_from_file(onnx_dir.join("vector_estimator.onnx"))?;
        let vocoder_ort = Session::builder()?.commit_from_file(onnx_dir.join("vocoder.onnx"))?;

        let text_processor = UnicodeProcessor::new(&onnx_dir.join("unicode_indexer.json"))?;

        Ok(Self {
            cfgs,
            text_processor,
            dp_ort,
            text_enc_ort,
            vector_est_ort,
            vocoder_ort,
            sample_rate,
        })
    }

    /// Sintetiza un texto completo, troceándolo y concatenando con silencios.
    /// Devuelve `(muestras f32, duración total en segundos)`.
    pub fn synthesize(
        &self,
        text: &str,
        lang: &str,
        style: &Style,
        total_step: usize,
        speed: f32,
        silence_secs: f32,
    ) -> Result<(Vec<f32>, f32)> {
        let chunks = chunk_text(text, Some(max_len_for_lang(lang)));

        let mut wav_cat: Vec<f32> = Vec::new();
        let mut dur_cat: f32 = 0.0;

        for (i, chunk) in chunks.iter().enumerate() {
            let (wav, duration) = self.infer(
                std::slice::from_ref(chunk),
                &[lang.to_string()],
                style,
                total_step,
                speed,
            )?;

            let dur = duration.first().copied().unwrap_or(0.0);
            let wav_len = (self.sample_rate as f32 * dur) as usize;
            let wav_chunk = &wav[..wav_len.min(wav.len())];

            if i == 0 {
                wav_cat.extend_from_slice(wav_chunk);
                dur_cat = dur;
            } else {
                let silence_len = (silence_secs * self.sample_rate as f32) as usize;
                wav_cat.extend(std::iter::repeat_n(0.0f32, silence_len));
                wav_cat.extend_from_slice(wav_chunk);
                dur_cat += silence_secs + dur;
            }
        }

        Ok((wav_cat, dur_cat))
    }

    /// Sintetiza **un solo fragmento** ya troceado (base del streaming de M5).
    /// Devuelve `(muestras recortadas a la duración, duración en segundos)`.
    pub fn synthesize_chunk(
        &self,
        chunk: &str,
        lang: &str,
        style: &Style,
        total_step: usize,
        speed: f32,
    ) -> Result<(Vec<f32>, f32)> {
        let (wav, duration) = self.infer(
            &[chunk.to_string()],
            &[lang.to_string()],
            style,
            total_step,
            speed,
        )?;
        let dur = duration.first().copied().unwrap_or(0.0);
        let wav_len = (self.sample_rate as f32 * dur) as usize;
        let wav_len = wav_len.min(wav.len());
        Ok((wav[..wav_len].to_vec(), dur))
    }

    /// Inferencia de un lote (aquí siempre `bsz = 1`). Devuelve `(wav, duración)`.
    fn infer(
        &self,
        text_list: &[String],
        lang_list: &[String],
        style: &Style,
        total_step: usize,
        speed: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        let bsz = text_list.len();

        let (text_ids, text_mask) = self.text_processor.call(text_list, lang_list)?;
        let cols = text_ids.first().map(|r| r.len()).unwrap_or(0);
        let mut flat = Vec::with_capacity(bsz * cols);
        for row in &text_ids {
            flat.extend_from_slice(row);
        }
        let text_ids_array: Array2<i64> = Array::from_shape_vec((bsz, cols), flat)?;

        // 1) Duración.
        let dp_out = self.dp_ort.run(ort::inputs! {
            "text_ids" => text_ids_array.view(),
            "style_dp" => style.dp.view(),
            "text_mask" => text_mask.view(),
        }?)?;
        let (_, dur_data) = dp_out["duration"].try_extract_raw_tensor::<f32>()?;
        let mut duration: Vec<f32> = dur_data.to_vec();
        for d in duration.iter_mut() {
            *d /= speed;
        }

        // 2) Codificación de texto.
        let te_out = self.text_enc_ort.run(ort::inputs! {
            "text_ids" => text_ids_array.view(),
            "style_ttl" => style.ttl.view(),
            "text_mask" => text_mask.view(),
        }?)?;
        let (te_shape, te_data) = te_out["text_emb"].try_extract_raw_tensor::<f32>()?;
        let text_emb: Array3<f32> = Array3::from_shape_vec(
            (
                te_shape[0] as usize,
                te_shape[1] as usize,
                te_shape[2] as usize,
            ),
            te_data.to_vec(),
        )?;

        // 3) Latente ruidoso.
        let (mut xt, latent_mask) = sample_noisy_latent(
            &duration,
            self.sample_rate,
            self.cfgs.ae.base_chunk_size,
            self.cfgs.ttl.chunk_compress_factor,
            self.cfgs.ttl.latent_dim,
        );

        // 4) Bucle de denoising.
        let total_step_array: Array1<f32> = Array::from_elem(bsz, total_step as f32);
        for step in 0..total_step {
            let current_step_array: Array1<f32> = Array::from_elem(bsz, step as f32);
            let ve_out = self.vector_est_ort.run(ort::inputs! {
                "noisy_latent" => xt.view(),
                "text_emb" => text_emb.view(),
                "style_ttl" => style.ttl.view(),
                "latent_mask" => latent_mask.view(),
                "text_mask" => text_mask.view(),
                "current_step" => current_step_array.view(),
                "total_step" => total_step_array.view(),
            }?)?;
            let (d_shape, d_data) = ve_out["denoised_latent"].try_extract_raw_tensor::<f32>()?;
            xt = Array3::from_shape_vec(
                (
                    d_shape[0] as usize,
                    d_shape[1] as usize,
                    d_shape[2] as usize,
                ),
                d_data.to_vec(),
            )?;
        }

        // 5) Vocoder → forma de onda.
        let voc_out = self.vocoder_ort.run(ort::inputs! {
            "latent" => xt.view(),
        }?)?;
        let (_, wav_data) = voc_out["wav_tts"].try_extract_raw_tensor::<f32>()?;

        Ok((wav_data.to_vec(), duration))
    }
}

/// Muestrea el latente ruidoso desde una normal y aplica la máscara temporal.
/// Port de `sample_noisy_latent`.
fn sample_noisy_latent(
    duration: &[f32],
    sample_rate: i32,
    base_chunk_size: i32,
    chunk_compress: i32,
    latent_dim: i32,
) -> (Array3<f32>, Array3<f32>) {
    let bsz = duration.len();
    let max_dur = duration.iter().fold(0.0f32, |a, &b| a.max(b));

    let wav_len_max = (max_dur * sample_rate as f32) as usize;
    let wav_lengths: Vec<usize> = duration
        .iter()
        .map(|&d| (d * sample_rate as f32) as usize)
        .collect();

    let chunk_size = (base_chunk_size * chunk_compress) as usize;
    let latent_len = wav_len_max.div_ceil(chunk_size);
    let latent_dim_val = (latent_dim * chunk_compress) as usize;

    let mut noisy_latent = Array3::<f32>::zeros((bsz, latent_dim_val, latent_len));

    let normal = Normal::new(0.0, 1.0).unwrap();
    let mut rng = rand::thread_rng();
    for b in 0..bsz {
        for d in 0..latent_dim_val {
            for t in 0..latent_len {
                noisy_latent[[b, d, t]] = normal.sample(&mut rng);
            }
        }
    }

    let latent_lengths: Vec<usize> = wav_lengths
        .iter()
        .map(|&len| len.div_ceil(chunk_size))
        .collect();
    let latent_mask = super::text::length_to_mask(&latent_lengths, Some(latent_len));

    for b in 0..bsz {
        for d in 0..latent_dim_val {
            for t in 0..latent_len {
                noisy_latent[[b, d, t]] *= latent_mask[[b, 0, t]];
            }
        }
    }

    (noisy_latent, latent_mask)
}
