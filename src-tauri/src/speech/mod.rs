//! Orquestación de la lectura con **streaming** (M5): captura → chunking →
//! síntesis por fragmentos → cola de audio.
//!
//! El punto clave es el pipeline productor/consumidor: en cuanto el fragmento 0
//! está sintetizado se empieza a reproducir, mientras un hilo de síntesis va
//! generando los siguientes. Así el primer audio suena a los pocos cientos de ms
//! del hotkey, sin esperar a sintetizar todo el texto.
//!
//! Una lectura nueva **reemplaza** a la anterior: cada sesión lleva un número de
//! generación; al empezar otra, el hilo de síntesis viejo se detiene en su
//! próxima comprobación y el audio anterior se descarta.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::audio::AudioPlayer;
use crate::chunking::{chunk_text_streaming, max_len_for_lang};
use crate::player_ui::{self, PlayerState};
use crate::tts::{TtsEngine, CHUNK_SILENCE_SECS, DEFAULT_VOICE};

/// Cuánto puede adelantarse el productor: sintetiza el siguiente fragmento
/// mientras suena el actual, pero espera si ya hay 2 encolados (el que suena +
/// uno por delante). Cumple "sintetizar N+1 mientras se reproduce N".
const MAX_LOOKAHEAD: usize = 2;

/// Espera entre comprobaciones de contrapresión del productor.
const BACKPRESSURE_POLL: Duration = Duration::from_millis(15);

/// Margen tras encolar el último fragmento antes de vigilar el vaciado de la
/// cola: `queued_len` se refresca en el poll del hilo de audio (~50 ms), así que
/// esperamos un poco para no cerrar el mini player antes de tiempo.
const DRAIN_SETTLE: Duration = Duration::from_millis(120);

/// Espera entre comprobaciones de vaciado de la cola (cierre del mini player).
const DRAIN_POLL: Duration = Duration::from_millis(100);

/// Ajustes de voz y salida. Se cargan/persisten desde la config (M6,
/// [`crate::settings`]) y se aplican con [`SpeechService::apply_settings`].
#[derive(Debug, Clone)]
pub struct SpeechSettings {
    pub voice: String,
    /// Idioma; `na` = agnóstico (§4, default `auto` → `na`).
    pub lang: String,
    pub speed: f32,
    /// Volumen de reproducción (0.0 = silencio, 1.0 = original).
    pub volume: f32,
    /// Dispositivo de salida por nombre; `None` = el predeterminado de Windows.
    pub output_device: Option<String>,
    /// Diccionario de reemplazos de pronunciación (feature #7). Se aplica al
    /// texto antes de trocear/sintetizar.
    pub replacements: Vec<crate::replacements::Replacement>,
}

impl Default for SpeechSettings {
    fn default() -> Self {
        Self {
            voice: DEFAULT_VOICE.to_string(),
            // Default `es`: suena mejor para español que el agnóstico `na`.
            // M6 permitirá cambiarlo (auto/na, es, en, fr, de…).
            lang: "es".to_string(),
            speed: 1.05,
            volume: 1.0,
            output_device: None,
            replacements: Vec::new(),
        }
    }
}

/// Servicio de voz compartido: motor TTS (carga perezosa) + audio (carga
/// perezosa del dispositivo en la primera reproducción).
pub struct SpeechService {
    engine: Arc<TtsEngine>,
    audio: Mutex<Option<Arc<AudioPlayer>>>,
    settings: Mutex<SpeechSettings>,
    /// Generación de la sesión de lectura actual (para reemplazar la anterior).
    generation: Arc<AtomicU64>,
    /// `true` si la reproducción actual está pausada.
    paused: AtomicBool,
    /// Handle de la app para gestionar el mini player (M7); se fija al arrancar.
    app: Mutex<Option<AppHandle>>,
    /// Línea de estado actual del mini player (`Spanish · M1 · 1.05x`).
    status: Mutex<String>,
}

impl SpeechService {
    pub fn new(engine: Arc<TtsEngine>) -> Self {
        Self {
            engine,
            audio: Mutex::new(None),
            settings: Mutex::new(SpeechSettings::default()),
            generation: Arc::new(AtomicU64::new(0)),
            paused: AtomicBool::new(false),
            app: Mutex::new(None),
            status: Mutex::new(String::new()),
        }
    }

    /// Fija el handle de la app (mini player). Se llama una vez al arrancar.
    pub fn set_app_handle(&self, app: AppHandle) {
        *self.app.lock().unwrap_or_else(|p| p.into_inner()) = Some(app);
    }

    /// Copia del handle de la app, si ya se fijó.
    fn app(&self) -> Option<AppHandle> {
        self.app.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Estado actual para la UI del mini player (comando inicial + eventos).
    pub fn player_state(&self) -> PlayerState {
        PlayerState {
            status: self
                .status
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone(),
            paused: self.paused.load(Ordering::SeqCst),
        }
    }

    /// Lee `text` en voz alta con **streaming**. Nunca hace panic: registra y
    /// sale ante cualquier error. Reemplaza cualquier lectura en curso.
    pub fn speak(&self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if !self.engine.models_present() {
            tracing::warn!("lectura solicitada pero el modelo aún no está descargado");
            return;
        }

        let (voice, lang, speed, volume, replacements) = {
            let s = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            (
                s.voice.clone(),
                s.lang.clone(),
                s.speed,
                s.volume,
                s.replacements.clone(),
            )
        };

        // Diccionario de pronunciación (feature #7): reescribe términos antes de
        // trocear, de modo que la síntesis sigue siendo una sola por fragmento
        // (se conserva la entonación).
        let text = crate::replacements::apply(text, &replacements);
        let chunks = chunk_text_streaming(&text, max_len_for_lang(&lang));

        // Abre el dispositivo (perezoso) antes de arrancar la sesión.
        let audio = match self.audio() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("dispositivo de audio no disponible: {e}");
                return;
            }
        };

        // Nueva generación: invalida la lectura anterior y su hilo de síntesis.
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.paused.store(false, Ordering::SeqCst);
        audio.new_session(gen, volume);

        // Mini player (M7): aparece al empezar a leer; reutiliza la ventana si
        // una lectura anterior la dejó abierta.
        let status = player_ui::status_line(&lang, &voice, speed);
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = status.clone();
        let app = self.app();
        if let Some(app) = &app {
            player_ui::ensure_shown(
                app,
                PlayerState {
                    status,
                    paused: false,
                },
            );
        }

        let engine = Arc::clone(&self.engine);
        let generation = Arc::clone(&self.generation);
        let t0 = Instant::now();

        std::thread::spawn(move || {
            let mut first = true;
            for chunk in chunks {
                let chunk = chunk.trim();
                if chunk.is_empty() {
                    continue;
                }
                // ¿Nos reemplazó otra lectura? Si es así, abortamos.
                if generation.load(Ordering::SeqCst) != gen {
                    return;
                }
                // Contrapresión: no adelantar más de MAX_LOOKAHEAD fragmentos.
                while audio.queued_len() >= MAX_LOOKAHEAD {
                    if generation.load(Ordering::SeqCst) != gen {
                        return;
                    }
                    std::thread::sleep(BACKPRESSURE_POLL);
                }

                let synth = match engine.synthesize_chunk(chunk, &lang, &voice, speed) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("síntesis de fragmento fallida: {e}");
                        continue;
                    }
                };

                if generation.load(Ordering::SeqCst) != gen {
                    return;
                }

                let mut samples = synth.samples;
                if !first {
                    // Silencio breve entre fragmentos (se antepone al siguiente).
                    let silence = (CHUNK_SILENCE_SECS * synth.sample_rate as f32) as usize;
                    let mut buf = Vec::with_capacity(silence + samples.len());
                    buf.extend(std::iter::repeat_n(0.0f32, silence));
                    buf.append(&mut samples);
                    samples = buf;
                }

                audio.enqueue(gen, samples, synth.sample_rate);

                if first {
                    tracing::info!(
                        "primer audio encolado en {:?} (síntesis del fragmento 0)",
                        t0.elapsed()
                    );
                    first = false;
                }
            }

            // Todos los fragmentos encolados: espera a que la cola se vacíe para
            // cerrar el mini player. Si otra lectura/stop nos reemplaza (cambia
            // la generación), no tocamos la ventana: esa sesión la gestiona.
            let Some(app) = app else {
                return;
            };
            std::thread::sleep(DRAIN_SETTLE);
            loop {
                if generation.load(Ordering::SeqCst) != gen {
                    return;
                }
                if audio.queued_len() == 0 {
                    break;
                }
                std::thread::sleep(DRAIN_POLL);
            }
            if generation.load(Ordering::SeqCst) == gen {
                player_ui::hide(&app);
            }
        });
    }

    /// Detiene la reproducción al instante y corta la síntesis en curso.
    pub fn stop(&self) {
        // Invalida la sesión: el hilo de síntesis se detendrá.
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
        if let Some(audio) = self.audio_ref() {
            audio.stop();
        }
        // Cierra el mini player (M7).
        if let Some(app) = self.app() {
            player_ui::hide(&app);
        }
    }

    /// Alterna pausa/reanudar la reproducción actual.
    pub fn toggle_pause(&self) {
        let Some(audio) = self.audio_ref() else {
            return;
        };
        // `fetch_xor` devuelve el valor previo: si estaba pausado, reanuda.
        let was_paused = self.paused.fetch_xor(true, Ordering::SeqCst);
        if was_paused {
            audio.resume();
        } else {
            audio.pause();
        }
        // Refleja el nuevo estado en el mini player si está abierto (M7).
        if let Some(app) = self.app() {
            player_ui::update(&app, self.player_state());
        }
    }

    /// Salta al siguiente fragmento de la cola.
    pub fn skip(&self) {
        if let Some(audio) = self.audio_ref() {
            audio.skip();
        }
    }

    /// Sintetiza **todo** `text` y lo escribe como WAV en `out_dir` (feature #2).
    ///
    /// A diferencia de [`Self::speak`], no reproduce ni usa la cola de audio:
    /// concatena las muestras de todos los fragmentos (con el mismo silencio
    /// intermedio que la lectura) y las vuelca a disco. Aplica la **velocidad**
    /// configurada, pero **no el volumen** (el archivo va a nivel original).
    /// Devuelve la ruta escrita o un mensaje de error legible.
    pub fn export_to_wav(
        &self,
        text: &str,
        out_dir: &std::path::Path,
    ) -> std::result::Result<std::path::PathBuf, String> {
        if text.trim().is_empty() {
            return Err("No hay texto para exportar.".to_string());
        }
        if !self.engine.models_present() {
            return Err("El modelo aún no está descargado.".to_string());
        }

        let (voice, lang, speed, replacements) = {
            let s = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            (
                s.voice.clone(),
                s.lang.clone(),
                s.speed,
                s.replacements.clone(),
            )
        };

        // Mismo diccionario de pronunciación que la lectura (feature #7).
        let text = crate::replacements::apply(text, &replacements);

        let mut all: Vec<f32> = Vec::new();
        let mut sample_rate = 0u32;
        let mut first = true;
        for chunk in chunk_text_streaming(&text, max_len_for_lang(&lang)) {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            let synth = self
                .engine
                .synthesize_chunk(chunk, &lang, &voice, speed)
                .map_err(|e| format!("síntesis fallida: {e}"))?;
            sample_rate = synth.sample_rate;
            if !first {
                // Mismo silencio entre fragmentos que en la reproducción.
                let silence = (CHUNK_SILENCE_SECS * sample_rate as f32) as usize;
                all.extend(std::iter::repeat_n(0.0f32, silence));
            }
            all.extend(synth.samples);
            first = false;
        }

        if all.is_empty() || sample_rate == 0 {
            return Err("No se generó audio.".to_string());
        }

        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("no se pudo crear la carpeta de destino: {e}"))?;
        let path = crate::export::build_output_path(out_dir);
        crate::export::write_wav_pcm16(&path, &all, sample_rate)
            .map_err(|e| format!("no se pudo escribir el WAV: {e}"))?;
        Ok(path)
    }

    /// Copia de los ajustes actuales (para valores por defecto de la UI).
    pub fn current_settings(&self) -> SpeechSettings {
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Aplica un conjunto de ajustes completo (voz/idioma/velocidad/volumen/
    /// dispositivo). Empuja al reproductor solo lo que cambió (volumen y/o
    /// dispositivo); voz/idioma/velocidad afectan a la siguiente lectura.
    pub fn apply_settings(&self, new: SpeechSettings) {
        let old = {
            let mut guard = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            let old = guard.clone();
            *guard = new.clone();
            old
        };
        if let Some(audio) = self.audio_ref() {
            if (old.volume - new.volume).abs() > f32::EPSILON {
                audio.set_volume(new.volume);
            }
            if old.output_device != new.output_device {
                audio.set_device(new.output_device);
            }
        }
    }

    /// Sintetiza y reproduce un texto de muestra con la voz/idioma/velocidad
    /// dados (botón *Preview* de Settings). No toca los ajustes guardados.
    pub fn preview(&self, voice: &str, lang: &str, speed: f32, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if !self.engine.models_present() {
            tracing::warn!("preview solicitado pero el modelo aún no está descargado");
            return;
        }
        let audio = match self.audio() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("dispositivo de audio no disponible: {e}");
                return;
            }
        };
        let volume = self.current_settings().volume;
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.paused.store(false, Ordering::SeqCst);
        audio.new_session(gen, volume);
        // Un preview reemplaza la sesión actual: si había un mini player abierto
        // de una lectura, ciérralo (el preview no muestra mini player).
        if let Some(app) = self.app() {
            player_ui::hide(&app);
        }

        let engine = Arc::clone(&self.engine);
        let (voice, lang, text) = (voice.to_string(), lang.to_string(), text.to_string());
        std::thread::spawn(
            move || match engine.synthesize_chunk(&text, &lang, &voice, speed) {
                Ok(s) => audio.enqueue(gen, s.samples, s.sample_rate),
                Err(e) => tracing::error!("preview: síntesis fallida: {e}"),
            },
        );
    }

    /// Ajusta el volumen y lo recuerda para las siguientes lecturas.
    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 2.0);
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .volume = volume;
        if let Some(audio) = self.audio_ref() {
            audio.set_volume(volume);
        }
    }

    /// Cambia el dispositivo de salida (`None` = el predeterminado del sistema).
    pub fn set_output_device(&self, device: Option<String>) {
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .output_device = device.clone();
        if let Some(audio) = self.audio_ref() {
            audio.set_device(device);
        }
    }

    /// Lista los dispositivos de salida disponibles (para Settings de M6).
    pub fn list_output_devices(&self) -> Vec<String> {
        AudioPlayer::list_output_devices()
    }

    /// Reproductor ya inicializado, o `None` si aún no se abrió el dispositivo.
    fn audio_ref(&self) -> Option<Arc<AudioPlayer>> {
        self.audio
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(Arc::clone)
    }

    /// Devuelve el reproductor, inicializando el dispositivo la primera vez.
    fn audio(&self) -> crate::audio::Result<Arc<AudioPlayer>> {
        let mut guard = self.audio.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            let device = self
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .output_device
                .clone();
            *guard = Some(Arc::new(AudioPlayer::new(device)?));
        }
        Ok(Arc::clone(
            guard.as_ref().expect("inicializado justo arriba"),
        ))
    }
}
