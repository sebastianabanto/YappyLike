# MANUAL_TESTS — YappyLike

Checklist de pruebas **manuales** (las que dependen de apps GUI y no se pueden
automatizar de forma fiable; ver §10 del prompt). Marca cada casilla al pasar.

Se irá ampliando con cada hito. Estado actual: M2–M7.

---

## Preparación

- [ ] Compilar y arrancar la app: `cargo run` dentro de `src-tauri/` (o el
      binario `target/debug/yappylike.exe`).
- [ ] Verificar que aparece el icono en la bandeja del sistema y que **no** hay
      ninguna ventana ni entrada en la barra de tareas.

---

## M2 — Captura de selección (`cargo run --example capture_probe`)

Ejecuta el probe desde `src-tauri/`. Durante la cuenta atrás, cambia a la app
indicada y **selecciona** un texto; el probe enviará `Ctrl+C` y lo leerá.

Por cada app: seleccionar texto → dejar que el probe capture → verificar que el
texto impreso coincide y que el portapapeles quedó intacto.

- [ ] **Notepad** — texto con acentos y `ñ` (p. ej. `Añoño camión €`).
- [ ] **Chrome / Edge** — un párrafo de una página web.
- [ ] **Firefox** — un párrafo.
- [ ] **VS Code** — una línea de código (verificar que no se altera).
- [ ] **Word** — texto con formato (se restaura el texto plano; el formato del
      portapapeles previo puede no restaurarse: es un límite documentado).
- [ ] **Lector de PDF** — una frase seleccionable.
- [ ] **Discord** — un mensaje.
- [ ] En todos: tras capturar, pegar (`Ctrl+V`) en cualquier campo y confirmar
      que **el portapapeles original se conservó**.

---

## M3 — Hotkeys globales

Con la app residente (sin foco en ella):

- [ ] Seleccionar texto en **Notepad** y pulsar **`Ctrl+Shift+Space`**: en el
      log (`%APPDATA%\YappyLike\logs\`) aparece `lectura: N caracteres
      capturados`. (La reproducción de voz llega en M4.)
- [ ] Repetir el punto anterior manteniendo pulsados `Ctrl+Shift` al disparar:
      la captura debe funcionar igual (el `Ctrl+C` interno suelta los
      modificadores antes de copiar).
- [ ] Pulsar `Ctrl+Shift+Space` **sin** nada seleccionado: log `lectura: sin
      selección`, la app no se cae ni bloquea.
- [ ] Pulsar `Ctrl+Shift+Space` **dos veces muy rápido**: la segunda pulsación
      se ignora (debounce / captura en curso), no se lanzan dos capturas.
- [ ] Pulsar `Ctrl+Shift+S`, `Ctrl+Shift+P`, `Ctrl+Shift+O`: cada uno registra
      su acción en el log (acciones reales en M5/M6).
- [ ] **Conflicto de atajo:** abrir otra app que ya use `Ctrl+Shift+Space`
      (o cambiar el default a uno ocupado) y arrancar YappyLike: aparece una
      **notificación** indicando el atajo que no se pudo registrar, y **el resto
      de atajos sigue funcionando**.
- [ ] Los hotkeys funcionan estando el foco en cualquier app (Word, navegador,
      VS Code…), no solo en el escritorio.

---

## M4 — TTS local (criterio de éxito del proyecto)

**Primer arranque (descarga del modelo):**

- [ ] Borra/renombra `%APPDATA%\YappyLike\models` y arranca la app: se abre la
      ventana **Bienvenido a YappyLike** mostrando el destino y una barra de
      progreso que avanza (~400 MB).
- [ ] Al terminar, la ventana muestra **¡Todo listo!** y un botón para cerrar.
- [ ] Reinicia la app: **no** vuelve a descargar (los archivos ya están y su
      tamaño coincide).
- [ ] Durante la descarga, el botón **Cancelar** la detiene y ofrece
      **Reintentar**.

**Lectura por hotkey (aceptación principal):**

- [ ] En Notepad, selecciona `Este es un texto de prueba en español.` y pulsa
      **Ctrl+Shift+Espacio**: se **escucha** el audio.
- [ ] Mientras suena, pulsa **Ctrl+Shift+S**: se **corta al instante**.
- [ ] Selecciona otro texto y pulsa Ctrl+Shift+Espacio de nuevo: lee el nuevo
      texto **sin reiniciar** la app.
- [ ] Pulsa Ctrl+Shift+Espacio con una lectura en curso: **cancela** la anterior
      y empieza la nueva (reemplaza).
- [ ] Prueba en inglés y en otro idioma; verifica que la voz es inteligible.
- [ ] Idioma por defecto: la app usa `es` (suena mejor para español; M6 lo hará
      configurable).

---

## M5 — Cola, chunking y streaming

Con el modelo ya descargado y la app residente:

- [ ] Selecciona un **párrafo largo** (varias frases) y pulsa
      **Ctrl+Shift+Espacio**: el audio **empieza en ~1 s** (no espera a
      sintetizar todo) y las frases **encadenan sin cortes** perceptibles.
- [ ] **Pausa/Reanuda** con **Ctrl+Shift+P** (o menú de bandeja → *Pause /
      Resume*): la voz se detiene y continúa donde iba.
- [ ] **Stop** con **Ctrl+Shift+S** (o bandeja → *Stop*): corta al instante y la
      cola se vacía.
- [ ] **Skip** (bandeja → *Skip*): salta a la siguiente frase de la cola.
- [ ] Lanza una **lectura nueva** mientras suena otra: la anterior se **descarta**
      y arranca la nueva (no se solapan ni encolan juntas).
- [ ] **Read selection** desde el menú de bandeja: captura la selección actual y
      la lee igual que el hotkey.
- [ ] (Opcional) Cambia el **dispositivo de salida** por defecto de Windows a
      mitad de uso: la app reintenta con el predeterminado del sistema y sigue
      reproduciendo.

> Medición de latencia reproducible: `cargo run --release --example stream_probe`
> imprime el tiempo hasta el primer audio (fragmento 0) frente a sintetizar todo.

---

## M6 — Settings + persistencia

Abre la ventana con el menú de bandeja → **Settings** (o `Ctrl+Shift+O`).

- [ ] La ventana respeta el tema **claro/oscuro** del sistema y no aparece en la
      barra de tareas como una app aparte molesta; al **cerrarla** se destruye
      (reabrirla la crea de nuevo, sin estado viejo).
- [ ] **Voz**: cambia la voz (p. ej. `F2`), pulsa **Preview** y escucha la
      muestra con esa voz; ajusta **Velocidad** y vuelve a probar.
- [ ] **Guardar** y luego **leer una selección**: usa la voz/idioma/velocidad
      nuevos. Reabre Settings: los valores guardados persisten.
- [ ] **Audio**: baja el **Volumen** y guarda; la siguiente lectura suena más
      bajo. Cambia el **dispositivo** de salida y verifica que se usa.
- [ ] **Atajos**: haz clic en «Leer selección», pulsa una combinación nueva
      (p. ej. `Ctrl+Alt+L`), guarda y comprueba que el atajo nuevo funciona y el
      viejo ya no.
- [ ] **Conflicto — duplicado**: asigna el mismo atajo a dos acciones y guarda:
      aparece un error y **no** se guarda.
- [ ] **Conflicto — ocupado**: asigna un atajo que ya use otra app; al guardar,
      el estado avisa de que ese atajo está ocupado (el resto sí se aplica).
- [ ] **General → Iniciar con Windows**: actívalo, guarda y verifica en el
      registro `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` que aparece
      `YappyLike`; desactívalo y guarda: desaparece.
- [ ] **Mostrar notificaciones**: desactívalo; provoca un conflicto de atajo al
      arrancar y confirma que **no** salta la notificación nativa.
- [ ] **Motor**: muestra `Backend: CPU`, la ruta del modelo y el estado (Listo).
- [ ] **Migración**: cierra la app, edita `%APPDATA%\YappyLike\config.toml`
      dejándolo corrupto (o borra un campo) y arranca: la app **no** se cae y usa
      los valores por defecto de lo que falte.

---

## M7 — Mini player

Con el modelo descargado y la app residente:

- [ ] Selecciona texto en **Notepad** y pulsa **Ctrl+Shift+Espacio**: aparece un
      **mini player** flotante (esquina inferior derecha) con la línea de estado
      tipo `Spanish · M1 · 1.05x` y botones Pausa/Reanudar, Stop y ✕.
- [ ] **NO roba el foco (requisito clave)**: al aparecer el mini player, el cursor
      y la **selección en Notepad se mantienen**; puedes seguir escribiendo/
      seleccionando sin que el foco salte a la ventanita. Pulsa su botón **Pausa**
      con el ratón: la reproducción se pausa y **el foco sigue en Notepad** (la
      ventana no se activa ni parpadea en primer plano).
- [ ] **No aparece en la barra de tareas** ni en el **Alt-Tab** (es una
      tool-window always-on-top).
- [ ] **Pausa/Reanudar** desde el mini player y desde **Ctrl+Shift+P**: el botón
      refleja el estado correcto (⏸ «Pausa» ↔ ▶ «Reanudar») en ambos casos.
- [ ] **Stop** en el mini player (o **Ctrl+Shift+S**): corta la lectura y **la
      ventana se cierra**.
- [ ] **Fin natural**: deja terminar una lectura corta; al vaciarse la cola el
      mini player **se cierra solo**.
- [ ] **Reutiliza la ventana**: con una lectura sonando, lanza otra selección con
      el hotkey: el mini player **no parpadea** (se reutiliza) y actualiza el
      estado; no se abren dos ventanas.
- [ ] **Cerrar (✕)**: cierra el widget; la lectura **puede seguir** (párala con el
      atajo/bandeja). Al lanzar otra lectura, el mini player vuelve a aparecer.
- [ ] **Arrastre**: arrastra el mini player por su cabecera a otra posición.
- [ ] Respeta el **tema claro/oscuro** del sistema.

---

## M8 — Build, instalador y portable

**Instalador** (`YappyLike_Setup_x64.exe`):

- [ ] Ejecuta el instalador en un Windows **sin** la app: SmartScreen avisa
      (sin firma) → «Más información» → «Ejecutar de todas formas» → instala
      **sin pedir administrador** (instalación por usuario).
- [ ] Tras instalar, la app arranca residente en la bandeja; en el **primer
      arranque** descarga el modelo (~400 MB) y luego queda lista.
- [ ] En un equipo **sin WebView2**, el instalador lo instala automáticamente
      (modo `downloadBootstrapper`) y la app abre sus ventanas correctamente.
- [ ] Aparece un acceso directo en el menú Inicio; la app se lee/reproduce con
      los atajos.
- [ ] **Iniciar con Windows** (Ajustes → General): al activarlo aparece el valor
      `YappyLike` en `HKCU\...\Run`; al desactivarlo, desaparece.
- [ ] **Desinstalar** desde *Aplicaciones*: elimina el programa; los datos de
      `%APPDATA%\YappyLike` se conservan hasta borrarlos a mano (documentado).

**Portable** (`YappyLike_Portable_x64.zip`):

- [ ] Descomprime en una carpeta cualquiera y ejecuta `YappyLike.exe`: arranca y,
      en el primer uso, descarga el modelo **dentro de `models\`** de esa carpeta.
- [ ] Verifica que `config\config.toml` y `logs\` se crean **dentro de la carpeta
      portable** y que **no** aparece nada en `%APPDATA%\YappyLike`.
- [ ] Copia la carpeta a otra ubicación (o USB) y ejecútala: sigue funcionando con
      su propia config/modelo (no depende de la ruta).
- [ ] Borra el archivo `PORTABLE`: al arrancar, la app vuelve a usar
      `%APPDATA%\YappyLike` (modo normal).

---

## v0.2.0 — Ciclo de vida de ventanas

Con la app residente:

- [ ] **No se cierra al terminar de leer**: lee un texto y deja que termine; al
      cerrarse el mini player, la app **sigue en la bandeja** (antes se cerraba
      sola). Vuelve a leer otra selección para confirmar que sigue operativa.
- [ ] **X de Ajustes = minimizar a bandeja**: abre Ajustes, pulsa la **X**: la
      ventana **desaparece pero la app sigue viva** (icono en bandeja). Vuelve a
      abrir *Settings* desde la bandeja: reaparece **con los valores que tenías**
      (no se recreó de cero).
- [ ] **Exit cierra de verdad**: clic derecho en el icono de bandeja → **Exit**:
      la app se cierra por completo (desaparece de la bandeja y del Administrador
      de tareas).
- [ ] Cerrar la ventana **Welcome** (primer arranque) con la X **no** cierra la
      app (queda en la bandeja).
