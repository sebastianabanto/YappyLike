//! Probe de M4: sintetiza una frase con el motor TTS portado y vuelca un WAV,
//! informando duración, RMS y pico para verificar que el audio no es silencio.
//!
//! Uso (desde `src-tauri/`): `cargo run --example tts_probe`
//! Requiere los modelos ya descargados en `%APPDATA%\YappyLike\models`.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use yappylike_lib::audio::AudioPlayer;
use yappylike_lib::tts::{default_models_dir, TtsEngine};

fn main() {
    let models_dir = default_models_dir().expect("APPDATA");
    println!("Modelos en: {}", models_dir.display());

    let engine = TtsEngine::new(models_dir);
    if !engine.models_present() {
        eprintln!("Faltan modelos. Descárgalos primero.");
        std::process::exit(1);
    }

    let text = "Este es un texto de prueba en español.";
    let lang = "es";

    let t0 = Instant::now();
    let synth = engine.synthesize(text, lang, "M1", 1.05).expect("síntesis");
    let load_and_synth = t0.elapsed();

    // Segunda síntesis: modelo ya cacheado (mide el objetivo de <1s de §9).
    let t1 = Instant::now();
    let synth2 = engine
        .synthesize("Otra frase distinta para probar.", lang, "M1", 1.05)
        .expect("segunda síntesis");
    let warm = t1.elapsed();

    let (rms, peak) = rms_peak(&synth.samples);
    println!("--- Síntesis 1 ---");
    println!("  muestras: {}", synth.samples.len());
    println!("  sample_rate: {}", synth.sample_rate);
    println!("  duración: {:.2} s", synth.duration_secs);
    println!("  RMS: {rms:.4}  pico: {peak:.4}");
    println!(
        "  tiempo (carga+síntesis): {:.2} s",
        load_and_synth.as_secs_f64()
    );
    println!("--- Síntesis 2 (modelo caliente) ---");
    println!(
        "  duración: {:.2} s  tiempo: {:.2} s",
        synth2.duration_secs,
        warm.as_secs_f64()
    );

    let out = PathBuf::from("tts_probe_out.wav");
    write_wav(&out, &synth.samples, synth.sample_rate).expect("escribir wav");
    println!("WAV (es) escrito en: {}", out.display());

    // También en 'na' (agnóstico, el default de la app §4) para comparar.
    let synth_na = engine
        .synthesize(text, "na", "M1", 1.05)
        .expect("síntesis na");
    let out_na = PathBuf::from("tts_probe_na.wav");
    write_wav(&out_na, &synth_na.samples, synth_na.sample_rate).expect("escribir wav na");
    let (rms_na, _) = rms_peak(&synth_na.samples);
    println!(
        "WAV (na) escrito en: {} (RMS {rms_na:.4})",
        out_na.display()
    );

    if rms < 1e-4 {
        eprintln!("ADVERTENCIA: el audio parece silencio (RMS muy bajo).");
        std::process::exit(2);
    }
    println!("OK: audio no silencioso.");

    // Verifica el camino real de reproducción (rodio): abre el dispositivo y
    // reproduce la síntesis por los altavoces.
    println!("Reproduciendo por el dispositivo de audio…");
    match AudioPlayer::new(None) {
        Ok(player) => {
            let secs = synth.duration_secs;
            // Nueva API de cola (M5): abre sesión y encola el audio.
            player.new_session(1, 1.0);
            player.enqueue(1, synth.samples.clone(), synth.sample_rate);
            std::thread::sleep(std::time::Duration::from_secs_f32(secs + 0.5));
            println!("OK: reproducción enviada al dispositivo sin errores.");
        }
        Err(e) => {
            eprintln!("No se pudo abrir el dispositivo de audio: {e}");
            std::process::exit(3);
        }
    }
}

fn rms_peak(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sumsq = 0.0f64;
    let mut peak = 0.0f32;
    for &s in samples {
        sumsq += (s as f64) * (s as f64);
        peak = peak.max(s.abs());
    }
    ((sumsq / samples.len() as f64).sqrt() as f32, peak)
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
    w.write_all(&16u32.to_le_bytes())?; // tamaño del subchunk fmt
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits por muestra
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;

    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let val = (clamped * 32767.0) as i16;
        w.write_all(&val.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}
