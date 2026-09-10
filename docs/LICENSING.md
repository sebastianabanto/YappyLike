# LICENSING — Motor TTS (Supertonic 3)

Resultado de leer la licencia **real** de los pesos, según pide el §3 del prompt.

## Fuente

- Código y ejemplos: <https://github.com/supertone-inc/supertonic>
- Pesos: <https://huggingface.co/Supertone/supertonic-3>
- Licencia del repo de pesos: **BigScience Open RAIL-M** (18 de agosto de 2022),
  archivo `LICENSE` en la raíz del repo de Hugging Face.

## Qué permite (resumen)

- **Uso comercial y no comercial:** permitido. La licencia concede un permiso de
  copyright «perpetuo, mundial, no exclusivo, sin cargo y libre de regalías»,
  siempre respetando las restricciones de uso del Anexo A.
- **Redistribución de los pesos:** permitida **con condiciones**: entregar copia
  de la licencia a los receptores, propagar las restricciones de uso como
  cláusulas vinculantes, marcar los archivos modificados y conservar los avisos
  de copyright/atribución.
- **Restricciones de uso (Anexo A):** prohíben usos como generar desinformación
  dañina, suplantación/deepfakes sin consentimiento, acoso, difusión de datos
  personales para causar daño, decisiones automatizadas con efectos legales,
  asesoramiento médico, usos policiales predictivos, etc.

## Impacto en YappyLike

- **El uso previsto está permitido.** YappyLike lee en voz alta un texto **que
  el usuario selecciona y controla**, de forma local. No incurre en ninguno de
  los usos prohibidos del Anexo A. → **No se activa ninguna condición de STOP.**
- **Distribución de los pesos:** aunque la licencia permite redistribuir con
  condiciones, YappyLike **no empaqueta los pesos** en el instalador. Se
  **descargan en el primer arranque** desde Hugging Face por HTTPS (era el diseño
  elegido de todas formas, §3). Así evitamos propagar las obligaciones de
  redistribución dentro del binario.
- **Aviso al usuario:** la pantalla de Privacidad / Acerca de debe indicar que la
  voz la genera el modelo Supertonic 3 bajo licencia Open RAIL-M y enlazar a la
  licencia y al repositorio oficiales, trasladando las restricciones de uso.

## Nota sobre responsabilidad de uso

Las restricciones de uso recaen en el usuario final (quien decide qué texto se
sintetiza). YappyLike es una utilidad de lectura de propósito general; la app
mostrará el aviso de licencia y el enlace, pero no puede vigilar el contenido que
el usuario decide leer (además, §11: el texto nunca sale del proceso).
