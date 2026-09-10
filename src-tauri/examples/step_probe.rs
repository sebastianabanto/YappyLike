//! Probe: compara la latencia de **primer audio** (lo que tardas en OÍR una
//! frase) con distinto número de **pasos de denoising** de Supertonic-3, con el
//! modelo ya caliente. Mide varias corridas y reporta min/mediana/max + RTF.
//!
//! Uso (desde `src-tauri/`):
//!   cargo run --release --example step_probe
//! Requiere los modelos ya descargados en `%APPDATA%\YappyLike\models`.

use std::time::Instant;

use yappylike_lib::tts::{default_models_dir, TtsEngine, DEFAULT_TOTAL_STEP};

const LANG: &str = "es";
const VOICE: &str = "M1";
const SPEED: f32 = 1.05;
const SENTENCE: &str = "Este es un texto de prueba en español para medir la latencia.";
const RUNS: usize = 3;
const STEPS: &[usize] = &[8, 16];

fn main() {
    let models_dir = default_models_dir().expect("APPDATA");
    println!("Modelos en: {}\n", models_dir.display());

    let engine = TtsEngine::new(models_dir);
    if !engine.models_present() {
        eprintln!("Faltan modelos. Descárgalos primero (arranca la app).");
        std::process::exit(1);
    }

    // Calentamiento: carga las 4 sesiones ONNX + la voz (fuera de la medición).
    println!("Calentando el modelo…");
    let _ = engine
        .synthesize_chunk("Calentando.", LANG, VOICE, SPEED)
        .expect("calentamiento");

    println!(
        "Frase: \"{}\" ({} car.)",
        SENTENCE,
        SENTENCE.chars().count()
    );
    println!("Voz {VOICE}, idioma {LANG}, velocidad {SPEED}; {RUNS} corridas por caso.\n");
    println!(
        "{:>6} | {:>9} | {:>9} | {:>9} | {:>9} | {:>5}",
        "pasos", "min (s)", "mediana", "max (s)", "audio (s)", "RTF"
    );
    println!("{}", "-".repeat(62));

    for &steps in STEPS {
        let mut times = Vec::with_capacity(RUNS);
        let mut audio = 0.0f32;
        for _ in 0..RUNS {
            let t = Instant::now();
            let s = engine
                .synthesize_chunk_steps(SENTENCE, LANG, VOICE, SPEED, steps)
                .expect("síntesis");
            times.push(t.elapsed().as_secs_f64());
            audio = s.duration_secs;
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let min = times[0];
        let med = times[times.len() / 2];
        let max = *times.last().unwrap();
        let rtf = med / audio as f64;
        let tag = if steps == DEFAULT_TOTAL_STEP {
            " (actual)"
        } else {
            ""
        };
        println!(
            "{:>6} | {:>9.3} | {:>9.3} | {:>9.3} | {:>9.2} | {:>5.2}{}",
            steps, min, med, max, audio, rtf, tag
        );
    }

    println!("\nNota: esto es la síntesis del PRIMER fragmento (lo que tardas en oír).");
    println!("Para el total 'desde la combinación de teclas hasta que suena', súmale la");
    println!("captura de la selección (~0.3-0.4 s ~constantes, el Ctrl+C interno).");
}
