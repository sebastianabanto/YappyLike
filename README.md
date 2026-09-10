# YappyLike

Utilidad residente para **Windows 11** que **lee en voz alta el texto que tengas
seleccionado** con un atajo global. Funciona **100 % en local** con un motor TTS
propio (Supertonic): sin nube, sin telemetría, sin peticiones de red tras la
descarga inicial del modelo. **Tu texto nunca sale de tu equipo.**

Selecciona texto en cualquier app (navegador, Word, PDF, VS Code, Discord…),
pulsa **`Ctrl + Shift + Espacio`** y escúchalo.

---

## Requisitos

- **Windows 11 x64** (funciona también en Windows 10 x64 reciente).
- **WebView2 Runtime** — presente por defecto en Windows 11. El **instalador lo
  instala automáticamente** si falta. Para la versión **portable** debe estar ya
  instalado (viene con Edge / Windows Update; también:
  <https://developer.microsoft.com/microsoft-edge/webview2/>).
- **~400 MB de espacio** para el modelo de voz, que se descarga en el **primer
  arranque** (una sola vez).

No necesitas Python, Node, Rust ni terminal para usar la app.

---

## Instalación (instalador)

1. Descarga **`YappyLike_Setup_x64_v<versión>.exe`** (p. ej.
   `YappyLike_Setup_x64_v0.2.0.exe`).
2. Ejecútalo. Es una **instalación por usuario** (no pide permisos de
   administrador) e instala en `%LOCALAPPDATA%\YappyLike`.

### ⚠️ Aviso de SmartScreen (ejecutable sin firmar)

El instalador **no está firmado digitalmente**, así que Windows SmartScreen
mostrará un aviso azul del tipo *«Windows protegió tu PC»*. Es normal en apps sin
certificado de firma (que es de pago). Para continuar:

1. Pulsa **«Más información»** (*More info*).
2. Pulsa **«Ejecutar de todas formas»** (*Run anyway*).

Si prefieres no fiarte, **compila desde el código** (ver más abajo) y obtendrás el
mismo binario a partir de las fuentes.

### Primer arranque

Al abrir la app por primera vez aparece la ventana **Bienvenido a YappyLike** con
una barra de progreso que descarga el modelo de voz (~400 MB). Al terminar, la app
queda **residente en la bandeja del sistema** (icono junto al reloj); no ocupa la
barra de tareas.

---

## Uso

Atajos por defecto (configurables en *Ajustes → Atajos*):

| Atajo | Acción |
|---|---|
| `Ctrl + Shift + Espacio` | Leer la selección actual |
| `Ctrl + Shift + P` | Pausar / Reanudar |
| `Ctrl + Shift + S` | Detener |
| `Ctrl + Shift + O` | Abrir Ajustes |

Al leer aparece un **mini reproductor** flotante (esquina inferior derecha) con
pausa/stop/cerrar. **No roba el foco**, así que no interrumpe lo que estés
haciendo. El icono de bandeja ofrece las mismas acciones y *Salir*.

La app es **residente**: cerrar la ventana de Ajustes (la **X**) la **oculta a la
bandeja**, no cierra el programa. Para cerrar YappyLike del todo, **clic derecho en
el icono de la bandeja → Exit**.

En **Ajustes** puedes cambiar la voz (`M1`..`M5`, `F1`..`F5`), el idioma, la
velocidad, el volumen, el dispositivo de salida y los atajos.

### Iniciar con Windows

En **Ajustes → General → «Iniciar con Windows»**. La app se registra en
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (por usuario, sin
administrador) y arranca minimizada en la bandeja.

---

## Versión portable

`YappyLike_Portable_x64.zip` — descomprímelo en cualquier carpeta (una memoria
USB, por ejemplo) y ejecuta **`YappyLike.exe`**. Estructura:

```
YappyLike_Portable_x64\
├─ YappyLike.exe
├─ PORTABLE        ← marcador: activa el modo portable
├─ models\         ← el modelo se descarga aquí en el primer arranque
├─ config\         ← configuración (config.toml)
└─ logs\           ← registros
```

En modo portable **la app solo escribe dentro de su propia carpeta** (config,
modelo y logs), sin tocar `%APPDATA%` ni el registro (salvo que actives «Iniciar
con Windows», que sí usa el registro por diseño). El marcador es el archivo vacío
`PORTABLE` junto al ejecutable; si lo borras, la app vuelve al modo normal
(`%APPDATA%\YappyLike`).

> **Nota:** la versión portable **requiere WebView2 ya instalado** (ver
> Requisitos). El instalador normal se encarga de ello; el ZIP no.

---

## Privacidad

- Todo el procesamiento (síntesis de voz) ocurre **en tu equipo**.
- La **única** conexión de red es la **descarga del modelo** en el primer
  arranque. Después, la app **no hace ninguna petición de red**.
- **Sin telemetría ni analítica.** Tu texto nunca se envía a ningún servidor.

---

## Compilar desde el código

Requisitos: [Rust](https://rustup.rs/) (stable, toolchain MSVC), [Node.js](https://nodejs.org/)
18+ y las [dependencias de Tauri en Windows](https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri build
```

Salidas en `src-tauri/target/release/`:

- Ejecutable: `yappylike.exe` (el instalado se llama `YappyLike.exe`).
- Instalador NSIS: `bundle/nsis/YappyLike_<versión>_x64-setup.exe`.

Para renombrar los artefactos a los nombres de distribución y generar el ZIP
portable:

```powershell
pwsh scripts/package_release.ps1
```

Deja en `dist-release/` los ficheros **versionados**
`YappyLike_Setup_x64_v<versión>.exe` y `YappyLike_Portable_x64_v<versión>.zip`
(la versión sale de `tauri.conf.json`, así que builds de versiones distintas no se
pisan entre sí).

---

## Desinstalar

- **Instalador:** *Configuración → Aplicaciones → YappyLike → Desinstalar* (o el
  desinstalador en `%LOCALAPPDATA%\YappyLike`).
- **Datos:** el modelo y la config viven en `%APPDATA%\YappyLike`
  (modo normal) o en la carpeta portable. Bórralos a mano si quieres eliminar todo
  rastro. Si activaste «Iniciar con Windows», el valor `YappyLike` en la clave
  `Run` se elimina al desactivarlo en Ajustes o al desinstalar.
