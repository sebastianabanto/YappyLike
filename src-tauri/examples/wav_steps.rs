//! Genera dos WAV de la MISMA frase con distinto número de pasos de denoising
//! (8 vs 16) para comparar la calidad a oído.
//!
//! Uso (desde `src-tauri/`):
//!   cargo run --release --example wav_steps
//! Escribe `voz_08pasos.wav` y `voz_16pasos.wav` en el directorio actual.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use yappylike_lib::tts::{default_models_dir, TtsEngine};

const LANG: &str = "es";
const VOICE: &str = "M1";
const SPEED: f32 = 1.05;
// Frase con entonación (coma, dos puntos y pregunta) para que se noten los matices.
const SENTENCE: &str =
    "Fíjate en la entonación y en los matices de la voz: ¿se nota la diferencia entre ocho y dieciséis pasos?";
const STEPS: &[usize] = &[8, 16];

fn main() {
    let models_dir = default_models_dir().expect("APPDATA");
    let engine = TtsEngine::new(models_dir);
    if !engine.models_present() {
        eprintln!("Faltan modelos. Descárgalos primero (arranca la app).");
        std::process::exit(1);
    }

    // Calentamiento (carga del modelo, fuera de interés).
    let _ = engine
        .synthesize_chunk("Calentando.", LANG, VOICE, SPEED)
        .expect("calentamiento");

    println!("Frase: \"{SENTENCE}\"\n");
    for &steps in STEPS {
        let synth = engine
            .synthesize_chunk_steps(SENTENCE, LANG, VOICE, SPEED, steps)
            .expect("síntesis");
        let out = PathBuf::from(format!("voz_{steps:02}pasos.wav"));
        write_wav(&out, &synth.samples, synth.sample_rate).expect("escribir wav");
        println!(
            "  {} pasos → {} ({:.2} s de audio, {} Hz)",
            steps,
            out.display(),
            synth.duration_secs,
            synth.sample_rate
        );
    }
    println!("\nEscúchalos y compara: entonación, aspereza y suavidad.");
}

/// Escritor WAV mínimo (PCM 16-bit mono), sin dependencias.
fn write_wav(path: &PathBuf, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
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
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&16u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        let val = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_all(&val.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}
