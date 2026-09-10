//! Probe de M5: mide el **tiempo hasta el primer audio** con streaming por
//! fragmentos y verifica la reproducción encadenada (cola) por el dispositivo.
//!
//! Compara la latencia de sintetizar solo el primer fragmento (lo que tarda en
//! empezar a sonar el streaming) contra sintetizar todo el texto (lo que tardaba
//! el modo no-streaming de M4). Reproduce además todos los fragmentos en cola,
//! encolando el N+1 mientras suena el N.
//!
//! Uso (desde `src-tauri/`): `cargo run --example stream_probe`
//! Requiere los modelos ya descargados en `%APPDATA%\YappyLike\models`.

use std::time::Instant;

use yappylike_lib::audio::AudioPlayer;
use yappylike_lib::tts::{default_models_dir, TtsEngine};

const LANG: &str = "es";
const VOICE: &str = "M1";
const SPEED: f32 = 1.05;

// Texto de varias frases: el primer fragmento debe sonar mucho antes que el
// texto completo.
const CHUNKS: &[&str] = &[
    "Este es un texto de prueba en español para medir la latencia del streaming.",
    "El segundo fragmento se sintetiza mientras el primero ya se está reproduciendo.",
    "Y el tercero llega justo a tiempo para que no haya cortes entre frases.",
];

fn main() {
    let models_dir = default_models_dir().expect("APPDATA");
    println!("Modelos en: {}", models_dir.display());

    let engine = TtsEngine::new(models_dir);
    if !engine.models_present() {
        eprintln!("Faltan modelos. Descárgalos primero.");
        std::process::exit(1);
    }

    // Calentamiento: carga el modelo (fuera de la medición del objetivo <1s).
    println!("Calentando el modelo…");
    let _ = engine
        .synthesize_chunk("Calentando.", LANG, VOICE, SPEED)
        .expect("calentamiento");

    // --- Caso realista: selección de UNA oración corta (aceptación M4) --- //
    let short = "Este es un texto de prueba en español.";
    let ts = Instant::now();
    let short_synth = engine
        .synthesize_chunk(short, LANG, VOICE, SPEED)
        .expect("frase corta");
    let short_audio = ts.elapsed();
    println!(
        "  oración corta ({} car., {:.2} s audio): primer audio en {:.3} s [{}]",
        short.chars().count(),
        short_synth.duration_secs,
        short_audio.as_secs_f64(),
        if short_audio.as_secs_f64() < 1.0 {
            "< 1 s ✓"
        } else {
            "≥ 1 s"
        }
    );

    // --- Latencia hasta el primer audio (streaming) --- //
    let t0 = Instant::now();
    let first = engine
        .synthesize_chunk(CHUNKS[0], LANG, VOICE, SPEED)
        .expect("fragmento 0");
    let first_audio = t0.elapsed();

    // --- Latencia sintetizando TODO (modo no-streaming de M4) --- //
    let full_text = CHUNKS.join(" ");
    let t1 = Instant::now();
    let _full = engine
        .synthesize(&full_text, LANG, VOICE, SPEED)
        .expect("texto completo");
    let full_synth = t1.elapsed();

    println!("--- Latencia (modelo caliente) ---");
    println!(
        "  primer audio (streaming, fragmento 0): {:.3} s",
        first_audio.as_secs_f64()
    );
    println!(
        "  sintetizar todo (no-streaming M4):     {:.3} s",
        full_synth.as_secs_f64()
    );
    let target = if first_audio.as_secs_f64() < 1.0 {
        "CUMPLE < 1 s"
    } else {
        "NO cumple < 1 s"
    };
    println!("  objetivo primer audio < 1 s: {target}");
    println!("  fragmento 0: {:.2} s de audio", first.duration_secs);

    // --- Reproducción encadenada (cola) --- //
    println!("Reproduciendo en cola (encola N+1 mientras suena N)…");
    let player = match AudioPlayer::new(None) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("No se pudo abrir el dispositivo de audio: {e}");
            std::process::exit(3);
        }
    };
    let gen = 1;
    player.new_session(gen, 1.0);

    let mut total_secs = 0.0f32;
    for (i, chunk) in CHUNKS.iter().enumerate() {
        // Contrapresión: no adelantar más de un fragmento.
        while player.queued_len() >= 2 {
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let synth = engine
            .synthesize_chunk(chunk, LANG, VOICE, SPEED)
            .expect("síntesis de fragmento");
        println!("  encolando fragmento {i} ({:.2} s)", synth.duration_secs);
        total_secs += synth.duration_secs + 0.3;
        player.enqueue(gen, synth.samples, synth.sample_rate);
    }

    // Espera a que termine de sonar todo.
    std::thread::sleep(std::time::Duration::from_secs_f32(total_secs + 1.0));
    println!("OK: reproducción encadenada sin errores.");
}
