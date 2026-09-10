//! Configuración persistente (§6, M6).
//!
//! Se guarda como TOML en `%APPDATA%\YappyLike\config.toml` (o `./config/` en
//! modo portable). La carga es **tolerante**: si el archivo no existe, está
//! corrupto o le faltan campos, se recuperan los valores por defecto sin
//! romper (cada `struct` lleva `#[serde(default)]`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hotkeys::{Hotkey, HotkeyConfig};
use crate::replacements::{self, Replacement};
use crate::speech::SpeechSettings;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("E/S en {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("no se pudo serializar la config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("combinación de atajo inválida en «{field}»: {value}")]
    BadHotkey { field: String, value: String },
}

pub type Result<T> = std::result::Result<T, SettingsError>;

/// Sección General.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub start_with_windows: bool,
    pub launch_minimized: bool,
    pub show_notifications: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            // La app es tray-only: "minimizada" es su estado natural al arrancar.
            launch_minimized: true,
            show_notifications: true,
        }
    }
}

/// Sección Voice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceCfg {
    /// Voz de estilo (`M1`..`M5`, `F1`..`F5`).
    pub voice: String,
    /// Idioma: `auto`/`na` (agnóstico) o un código (`es`, `en`, `fr`, `de`…).
    pub lang: String,
    /// Velocidad aplicada en el modelo (0.9–1.5; default 1.05).
    pub speed: f32,
}

impl Default for VoiceCfg {
    fn default() -> Self {
        let d = SpeechSettings::default();
        Self {
            voice: d.voice,
            lang: d.lang,
            speed: d.speed,
        }
    }
}

/// Sección Hotkeys (formas de texto, p. ej. `Ctrl+Shift+Space`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeysCfg {
    pub read: String,
    pub read_clipboard: String,
    pub stop: String,
    pub pause_resume: String,
    pub settings: String,
}

impl Default for HotkeysCfg {
    fn default() -> Self {
        let d = HotkeyConfig::default();
        Self {
            read: d.read.to_string(),
            read_clipboard: d.read_clipboard.to_string(),
            stop: d.stop.to_string(),
            pause_resume: d.pause_resume.to_string(),
            settings: d.settings.to_string(),
        }
    }
}

/// Sección Audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioCfg {
    /// Dispositivo de salida por nombre; `None`/ausente = el predeterminado.
    pub output_device: Option<String>,
    /// Volumen (0.0–2.0; 1.0 = original).
    pub volume: f32,
}

impl Default for AudioCfg {
    fn default() -> Self {
        let d = SpeechSettings::default();
        Self {
            output_device: d.output_device,
            volume: d.volume,
        }
    }
}

/// Sección Export (feature #2): carpeta de guardado de los WAV exportados.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportCfg {
    /// Carpeta de destino; `None`/vacío = la de por defecto
    /// (`Documentos\YappyLike`, o `<portable>\exports`).
    pub dir: Option<String>,
}

/// Configuración completa de la app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub voice: VoiceCfg,
    pub hotkeys: HotkeysCfg,
    pub audio: AudioCfg,
    pub export: ExportCfg,
    /// Diccionario de reemplazos de pronunciación (feature #7). Si la clave falta
    /// en el TOML (config antigua o primer arranque), se rellena con la semilla.
    #[serde(default = "replacements::seed")]
    pub replacements: Vec<Replacement>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            voice: VoiceCfg::default(),
            hotkeys: HotkeysCfg::default(),
            audio: AudioCfg::default(),
            export: ExportCfg::default(),
            replacements: replacements::seed(),
        }
    }
}

impl Config {
    /// Carga la config desde la ruta por defecto, recuperando defaults ante
    /// cualquier problema (archivo ausente, corrupto o con campos faltantes).
    pub fn load() -> Self {
        Self::load_from(&config_file())
    }

    /// Como [`Config::load`] pero desde una ruta concreta (para tests).
    pub fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("no hay config previa; usando valores por defecto");
                return Self::default();
            }
            Err(e) => {
                tracing::warn!("no se pudo leer la config ({e}); usando defaults");
                return Self::default();
            }
        };
        match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!("config corrupta ({e}); usando valores por defecto");
                Self::default()
            }
        }
    }

    /// Guarda la config en la ruta por defecto.
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_file())
    }

    /// Como [`Config::save`] pero a una ruta concreta (para tests).
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| SettingsError::Io {
                path: dir.display().to_string(),
                source: e,
            })?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text).map_err(|e| SettingsError::Io {
            path: path.display().to_string(),
            source: e,
        })
    }

    /// Construye los ajustes de voz/audio del [`crate::speech::SpeechService`].
    pub fn to_speech_settings(&self) -> SpeechSettings {
        SpeechSettings {
            voice: self.voice.voice.clone(),
            lang: self.voice.lang.clone(),
            speed: self.voice.speed,
            volume: self.audio.volume,
            output_device: self.audio.output_device.clone(),
            replacements: self.replacements.clone(),
        }
    }

    /// Construye la configuración de atajos, validando cada combinación.
    pub fn to_hotkey_config(&self) -> Result<HotkeyConfig> {
        Ok(HotkeyConfig {
            read: parse_hotkey("read", &self.hotkeys.read)?,
            read_clipboard: parse_hotkey("read_clipboard", &self.hotkeys.read_clipboard)?,
            stop: parse_hotkey("stop", &self.hotkeys.stop)?,
            pause_resume: parse_hotkey("pause_resume", &self.hotkeys.pause_resume)?,
            settings: parse_hotkey("settings", &self.hotkeys.settings)?,
        })
    }
}

fn parse_hotkey(field: &str, value: &str) -> Result<Hotkey> {
    value
        .parse::<Hotkey>()
        .map_err(|_| SettingsError::BadHotkey {
            field: field.to_string(),
            value: value.to_string(),
        })
}

/// Ruta del archivo de configuración.
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Directorio de configuración: `<dir_exe>\config` en modo portable, si no
/// `%APPDATA%\YappyLike`.
pub fn config_dir() -> PathBuf {
    if let Some(root) = portable_root() {
        return root.join("config");
    }
    appdata_dir()
}

fn appdata_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("YappyLike")
}

/// Raíz del modo portable (§6, M8): si junto al ejecutable existe un marcador
/// `PORTABLE`, devuelve `<dir_exe>`. Toda la app (config, modelos y logs) vive
/// entonces dentro de esa carpeta y **no** se escribe fuera de ella.
///
/// Es la fuente de verdad compartida por [`config_dir`], [`crate::tts`] y
/// [`crate::logging`] para decidir dónde guardar sus datos.
pub fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.join("PORTABLE").exists() {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

/// Nombre del valor en la clave `Run` del registro.
#[cfg(windows)]
const RUN_VALUE: &str = "YappyLike";

/// Activa o desactiva el arranque con Windows (clave `Run` de `HKCU`).
#[cfg(windows)]
pub fn set_start_with_windows(enabled: bool) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")?;
    if enabled {
        let exe = std::env::current_exe()?;
        run.set_value(RUN_VALUE, &format!("\"{}\"", exe.display()))?;
        tracing::info!("arranque con Windows activado");
    } else {
        match run.delete_value(RUN_VALUE) {
            Ok(()) => tracing::info!("arranque con Windows desactivado"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Stub no-Windows (la app es Windows-only; permite compilar/testear en otros SO).
#[cfg(not(windows))]
pub fn set_start_with_windows(_enabled: bool) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "yappylike_test_{}_{}.toml",
            name,
            std::process::id()
        ));
        p
    }

    #[test]
    fn roundtrip_guardar_y_cargar() {
        let path = temp_path("roundtrip");
        let mut cfg = Config::default();
        cfg.voice.voice = "F3".to_string();
        cfg.voice.speed = 1.2;
        cfg.audio.volume = 0.7;
        cfg.general.start_with_windows = true;
        cfg.save_to(&path).expect("guardar");

        let loaded = Config::load_from(&path);
        assert_eq!(loaded, cfg);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn archivo_inexistente_da_defaults() {
        let path = temp_path("inexistente_xyz");
        let _ = std::fs::remove_file(&path);
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn archivo_corrupto_da_defaults() {
        let path = temp_path("corrupto");
        std::fs::write(&path, "esto no es toml válido = = = [[[").expect("escribir");
        assert_eq!(Config::load_from(&path), Config::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn campo_faltante_usa_default_sin_romper() {
        let path = temp_path("parcial");
        // Solo se fija la voz; el resto (idioma, velocidad, audio, atajos…)
        // debe recuperarse por defecto.
        std::fs::write(&path, "[voice]\nvoice = \"F2\"\n").expect("escribir");

        let loaded = Config::load_from(&path);
        assert_eq!(loaded.voice.voice, "F2");
        assert_eq!(loaded.voice.speed, VoiceCfg::default().speed);
        assert_eq!(loaded.voice.lang, VoiceCfg::default().lang);
        assert_eq!(loaded.audio, AudioCfg::default());
        assert_eq!(loaded.hotkeys, HotkeysCfg::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn convierte_a_hotkey_config_valido() {
        let cfg = Config::default();
        let hk = cfg.to_hotkey_config().expect("atajos válidos por defecto");
        assert_eq!(hk, HotkeyConfig::default());
    }

    #[test]
    fn hotkey_invalido_es_error() {
        let mut cfg = Config::default();
        cfg.hotkeys.read = "Ctrl+Shift+???".to_string();
        assert!(cfg.to_hotkey_config().is_err());
    }
}
