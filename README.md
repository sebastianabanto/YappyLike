# YappyLike — Offline text-to-speech for Windows

[![Version](https://img.shields.io/badge/version-0.3.0-2563eb.svg)](https://github.com/sebastianabanto/YappyLike)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4.svg)](https://github.com/sebastianabanto/YappyLike#requirements--requisitos)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24c8db.svg)](https://tauri.app/)
[![Backend](https://img.shields.io/badge/backend-Rust-dea584.svg)](https://www.rust-lang.org/)
[![Frontend](https://img.shields.io/badge/frontend-TypeScript-3178c6.svg)](https://www.typescriptlang.org/)
[![Last commit](https://img.shields.io/github/last-commit/sebastianabanto/YappyLike?label=last%20commit)](https://github.com/sebastianabanto/YappyLike/commits/master/)

YappyLike is a free, local-first Windows desktop app that reads selected text
aloud with a global keyboard shortcut. It is an **offline text-to-speech (TTS)
reader for Windows 10/11**: your text stays on your computer, there is no cloud
API, no telemetry, and no account required after the voice model is downloaded.

Select text in a browser, PDF, Word, VS Code, Discord, or any other Windows app,
then press **`Ctrl + Shift + Space`** to hear it. YappyLike runs quietly in the
system tray and uses the local [Supertonic](https://github.com/supertone-inc/supertonic)
TTS engine through ONNX Runtime.

> YappyLike is currently distributed as source code and Windows x64 build
> artifacts. The installer is unsigned; see the SmartScreen note below.

## YappyLike en español

YappyLike es una aplicación de escritorio para **leer texto seleccionado en voz
alta en Windows**, con un atajo de teclado global. Funciona de forma local y
privada: no sube tu texto a la nube, no usa telemetría y no necesita una cuenta.

Selecciona texto en el navegador, un PDF, Word, VS Code, Discord u otra
aplicación de Windows y pulsa **`Ctrl + Shift + Espacio`**. La aplicación queda
en la bandeja del sistema y utiliza el motor TTS local
[Supertonic](https://github.com/supertone-inc/supertonic) mediante ONNX Runtime.

## Features / Funciones

- **Global hotkey / Atajo global:** read selected text from almost any Windows app.
- **Local TTS / Voz local:** CPU inference with Supertonic; no speech cloud service.
- **Privacy-first / Privacidad:** selected text is processed locally and is not uploaded.
- **Tray app / Bandeja del sistema:** stays resident without occupying the taskbar.
- **Streaming playback / Reproducción progresiva:** long text is split into chunks and starts playing as it is synthesized.
- **Clipboard support / Portapapeles:** reads copied text when direct selection capture is unavailable.
- **WAV export / Exportación WAV:** export the current text as a WAV file.
- **Configurable voices / Voces configurables:** M1–M5 and F1–F5, speed, language, volume, output device, and hotkeys.
- **Portable mode / Modo portable:** keep configuration, logs, and downloaded models beside the executable.
- **Windows-native desktop app:** built with Tauri 2, Rust, TypeScript, and Vite; no Electron.

## Default shortcuts / Atajos predeterminados

| Shortcut / Atajo | Action / Acción |
|---|---|
| `Ctrl + Shift + Space` | Read selected text / Leer selección |
| `Ctrl + Shift + P` | Pause or resume / Pausar o reanudar |
| `Ctrl + Shift + S` | Stop / Detener |
| `Ctrl + Shift + O` | Open settings / Abrir ajustes |

All shortcuts can be changed in **Settings → Hotkeys** / **Ajustes → Atajos**.

## Requirements / Requisitos

- Windows 11 x64, or a recent Windows 10 x64 installation.
- [Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).
  Windows 11 normally includes it; the installer downloads it when needed.
- Approximately **400 MB** of disk space for the local voice model, downloaded on
  first use.

The end user does not need Python, Node.js, Rust, or a terminal to run the
installer. The portable ZIP requires WebView2 to already be installed.

## Installation / Instalación

Download the versioned Windows x64 installer, for example
`YappyLike_Setup_x64_v0.3.0.exe`, and run it. The installer uses a per-user
installation under `%LOCALAPPDATA%\YappyLike` and does not require administrator
privileges.

On first launch, YappyLike downloads the voice model once. After that, voice
synthesis works locally without network requests.

### Windows SmartScreen

The current installer is not digitally signed. Windows may show a blue
SmartScreen warning. If you trust the downloaded file, choose **More info → Run
anyway**. You can also build the application yourself from source using the
instructions below.

### Portable mode / Modo portable

Extract `YappyLike_Portable_x64_v0.3.0.zip` and run `YappyLike.exe`:

```text
YappyLike_Portable_x64/
├─ YappyLike.exe
├─ PORTABLE       marker file
├─ models/        downloaded voice model
├─ config/        config.toml
└─ logs/          runtime logs
```

Portable mode stores application data in its own directory instead of
`%APPDATA%`. Enabling **Start with Windows / Iniciar con Windows** intentionally
uses the per-user Windows `Run` registry key.

## Privacy / Privacidad

- Selected and copied text is processed on the local computer.
- There is no telemetry, analytics, login, or cloud speech API.
- The only network operation is the first-run download of the voice model.
- Runtime configuration and logs are local and are excluded from version control.

See [SECURITY.md](SECURITY.md) for vulnerability reporting and credential
handling guidance.

## Build from source / Compilar desde el código

### Prerequisites / Dependencias de desarrollo

- Rust stable with the MSVC toolchain.
- Node.js 18 or newer and npm.
- Windows prerequisites for [Tauri](https://tauri.app/start/prerequisites/).

### Build / Compilación

```bash
npm install
npm run build
npm run tauri build
```

To create the versioned installer and portable ZIP:

```powershell
pwsh scripts/package_release.ps1
```

The release artifacts are written to `dist-release/`. The Tauri executable is
generated under `src-tauri/target/release/`.

## Technical overview / Resumen técnico

YappyLike is a Windows-first Tauri 2 application with a Rust backend and a
vanilla TypeScript/Vite frontend. The speech pipeline uses Supertonic model
assets and ONNX Runtime with CPU execution. It captures selected text through
the Windows clipboard, chunks long text, synthesizes progressively, and plays
audio through the selected Windows output device.

The repository includes focused modules for selection capture, global hotkeys,
text chunking, model downloads and SHA-256 verification, speech synthesis,
audio playback, settings persistence, tray integration, and the floating mini
player.

## Project status / Estado del proyecto

The current codebase is version **0.3.0**. It includes clipboard reading, WAV
export, pronunciation replacements, configurable settings, tray operation,
portable packaging, and local Supertonic speech synthesis.

## Uninstall / Desinstalar

Uninstall the application from **Windows Settings → Apps → YappyLike**. The
downloaded model, configuration, and logs may remain in `%APPDATA%\YappyLike`
until removed manually. Portable mode keeps them in the portable directory.

## License / Licencia

See [docs/LICENSING.md](docs/LICENSING.md) for the model and dependency license
information. The voice model is downloaded separately and is not bundled in this
repository.

## Keywords / Palabras clave

offline text to speech, local TTS, private text reader, Windows screen reader,
read selected text aloud, global hotkey text reader, Windows accessibility,
Spanish text to speech, English text to speech, offline voice synthesis,
Supertonic TTS, ONNX Runtime, Rust Tauri desktop app, portable Windows app,
lector de texto en voz alta, lectura de selección, síntesis de voz local,
aplicación Windows privada, lector de PDF y navegador.
