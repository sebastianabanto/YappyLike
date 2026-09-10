//! Cola de reproducción con streaming (M5).
//!
//! `rodio::OutputStream` es `!Send`, así que el dispositivo y el `Sink` viven en
//! un **hilo de audio dedicado** al que se le envían comandos por un canal. El
//! resto de la app solo encola muestras (`enqueue`) y controla la reproducción
//! (pause/resume/stop/skip/volumen/dispositivo) sin tocar el dispositivo.
//!
//! El streaming funciona así: el productor (hilo de síntesis, en `speech`) va
//! generando fragmentos y los **encola** en el `Sink`, que los reproduce en
//! orden y sin cortes. Para no adelantarse sin límite, el productor consulta
//! [`AudioPlayer::queued_len`] y espera cuando ya hay un fragmento por delante
//! (a lo sumo sintetiza el N+1 mientras suena el N).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::cpal::{self};
use rodio::{OutputStream, OutputStreamHandle, Sink};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no se pudo inicializar el dispositivo de audio: {0}")]
    Device(String),
    #[error("el hilo de audio no está disponible")]
    ThreadGone,
}

pub type Result<T> = std::result::Result<T, AudioError>;

/// Cada cuánto revisa el hilo la longitud de la cola mientras hay audio sonando
/// (para reflejarla en `queued` y darle contrapresión al productor).
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Comandos que procesa el hilo de audio.
enum AudioCmd {
    /// Abre una sesión de reproducción nueva: descarta la anterior y crea un
    /// `Sink` limpio con el volumen dado. Las muestras se marcan con `gen`.
    NewSession { gen: u64, volume: f32 },
    /// Encola muestras mono en la sesión `gen` (se ignoran si la sesión cambió).
    Enqueue {
        gen: u64,
        samples: Vec<f32>,
        sample_rate: u32,
    },
    /// Pausa la reproducción (conserva la cola).
    Pause,
    /// Reanuda tras una pausa.
    Resume,
    /// Detiene y vacía la cola al instante.
    Stop,
    /// Salta al siguiente fragmento encolado.
    Skip,
    /// Ajusta el volumen (0.0–1.0+) de la sesión actual y de las siguientes.
    SetVolume(f32),
    /// Cambia el dispositivo de salida (`None` = el predeterminado de Windows).
    SetDevice(Option<String>),
}

/// Reproductor de audio. Barato de clonar vía `Arc`; los métodos solo envían un
/// comando al hilo dedicado (no bloquean).
pub struct AudioPlayer {
    tx: Sender<AudioCmd>,
    /// Número de fragmentos encolados (incluye el que suena). Lo actualiza el
    /// hilo de audio; el productor lo lee para autolimitar el adelanto.
    queued: Arc<AtomicUsize>,
}

impl AudioPlayer {
    /// Arranca el hilo de audio e inicializa el dispositivo (`None` = el
    /// predeterminado del sistema).
    pub fn new(device: Option<String>) -> Result<Self> {
        let (tx, rx) = channel::<AudioCmd>();
        // Canal de un solo uso para reportar el resultado de abrir el dispositivo.
        let (ready_tx, ready_rx) = channel::<Result<()>>();
        let queued = Arc::new(AtomicUsize::new(0));
        let queued_thread = Arc::clone(&queued);

        thread::Builder::new()
            .name("yappylike-audio".into())
            .spawn(move || audio_loop(rx, ready_tx, queued_thread, device))
            .map_err(|e| AudioError::Device(e.to_string()))?;

        // Espera a saber si el dispositivo abrió bien.
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx, queued }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AudioError::ThreadGone),
        }
    }

    /// Abre una sesión de reproducción nueva con el `gen` y volumen dados.
    /// Descarta cualquier reproducción en curso.
    pub fn new_session(&self, gen: u64, volume: f32) {
        self.send(AudioCmd::NewSession { gen, volume });
    }

    /// Encola muestras (mono, `f32` en [-1, 1]) en la sesión `gen`.
    pub fn enqueue(&self, gen: u64, samples: Vec<f32>, sample_rate: u32) {
        self.send(AudioCmd::Enqueue {
            gen,
            samples,
            sample_rate,
        });
    }

    /// Pausa la reproducción sin perder la cola.
    pub fn pause(&self) {
        self.send(AudioCmd::Pause);
    }

    /// Reanuda tras una pausa.
    pub fn resume(&self) {
        self.send(AudioCmd::Resume);
    }

    /// Detiene la reproducción y vacía la cola al instante.
    pub fn stop(&self) {
        self.send(AudioCmd::Stop);
    }

    /// Salta al siguiente fragmento encolado.
    pub fn skip(&self) {
        self.send(AudioCmd::Skip);
    }

    /// Ajusta el volumen (0.0 = silencio, 1.0 = original).
    pub fn set_volume(&self, volume: f32) {
        self.send(AudioCmd::SetVolume(volume));
    }

    /// Cambia el dispositivo de salida (`None` = el predeterminado del sistema).
    pub fn set_device(&self, device: Option<String>) {
        self.send(AudioCmd::SetDevice(device));
    }

    /// Número de fragmentos encolados (incluido el que suena). Base de la
    /// contrapresión del productor de streaming.
    pub fn queued_len(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }

    /// Lista los dispositivos de salida disponibles por nombre.
    pub fn list_output_devices() -> Vec<String> {
        let host = cpal::default_host();
        match host.output_devices() {
            Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
            Err(e) => {
                tracing::warn!("no se pudieron enumerar dispositivos de salida: {e}");
                Vec::new()
            }
        }
    }

    fn send(&self, cmd: AudioCmd) {
        if self.tx.send(cmd).is_err() {
            tracing::error!("el hilo de audio no está disponible; comando descartado");
        }
    }
}

/// Abre un `OutputStream` en el dispositivo dado por nombre; si no se indica o
/// no se encuentra, usa el predeterminado del sistema.
fn open_stream(device_name: Option<&str>) -> Result<(OutputStream, OutputStreamHandle)> {
    if let Some(name) = device_name {
        let host = cpal::default_host();
        match host.output_devices() {
            Ok(devs) => {
                for dev in devs {
                    if dev.name().ok().as_deref() == Some(name) {
                        return OutputStream::try_from_device(&dev)
                            .map_err(|e| AudioError::Device(e.to_string()));
                    }
                }
            }
            Err(e) => tracing::warn!("no se pudieron enumerar dispositivos: {e}"),
        }
        tracing::warn!("dispositivo de audio '{name}' no encontrado; usando el predeterminado");
    }
    OutputStream::try_default().map_err(|e| AudioError::Device(e.to_string()))
}

/// Bucle del hilo de audio: posee el `OutputStream` (debe seguir vivo) y el
/// `Sink` de la sesión actual.
fn audio_loop(
    rx: std::sync::mpsc::Receiver<AudioCmd>,
    ready_tx: Sender<Result<()>>,
    queued: Arc<AtomicUsize>,
    initial_device: Option<String>,
) {
    let (mut stream, mut handle) = match open_stream(initial_device.as_deref()) {
        Ok(pair) => {
            let _ = ready_tx.send(Ok(()));
            pair
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

    let mut sink: Option<Sink> = None;
    let mut cur_gen: u64 = 0;

    loop {
        // Mientras haya algo sonando, despertamos cada POLL_INTERVAL para
        // reflejar la longitud de la cola (contrapresión). Si no, bloqueamos.
        let playing = sink.as_ref().map(|s| !s.empty()).unwrap_or(false);
        let cmd = if playing {
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        };

        if let Some(s) = &sink {
            queued.store(s.len(), Ordering::Relaxed);
        }

        let Some(cmd) = cmd else { continue };

        match cmd {
            AudioCmd::NewSession { gen, volume } => {
                if let Some(old) = sink.take() {
                    old.stop();
                }
                match Sink::try_new(&handle) {
                    Ok(s) => {
                        s.set_volume(volume);
                        sink = Some(s);
                        cur_gen = gen;
                        queued.store(0, Ordering::Relaxed);
                    }
                    Err(e) => tracing::error!("no se pudo crear el sink de audio: {e}"),
                }
            }
            AudioCmd::Enqueue {
                gen,
                samples,
                sample_rate,
            } => {
                if gen == cur_gen {
                    if let Some(s) = &sink {
                        s.append(SamplesBuffer::new(1, sample_rate, samples));
                        queued.store(s.len(), Ordering::Relaxed);
                    }
                }
            }
            AudioCmd::Pause => {
                if let Some(s) = &sink {
                    s.pause();
                }
            }
            AudioCmd::Resume => {
                if let Some(s) = &sink {
                    s.play();
                }
            }
            AudioCmd::Stop => {
                if let Some(s) = sink.take() {
                    s.stop();
                }
                queued.store(0, Ordering::Relaxed);
            }
            AudioCmd::Skip => {
                if let Some(s) = &sink {
                    s.skip_one();
                }
            }
            AudioCmd::SetVolume(v) => {
                if let Some(s) = &sink {
                    s.set_volume(v);
                }
            }
            AudioCmd::SetDevice(name) => match open_stream(name.as_deref()) {
                Ok((s, h)) => {
                    if let Some(old) = sink.take() {
                        old.stop();
                    }
                    stream = s;
                    handle = h;
                    queued.store(0, Ordering::Relaxed);
                    tracing::info!("dispositivo de audio cambiado");
                }
                Err(e) => tracing::error!("no se pudo cambiar de dispositivo de audio: {e}"),
            },
        }
    }

    // Al cerrarse el canal (app saliendo), soltamos sink y stream explícitamente.
    drop(sink);
    drop(stream);
}
