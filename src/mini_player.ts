// Mini player flotante (M7): muestra el estado de la lectura y sus controles.
// La ventana la crea/destruye el backend; aquí solo pintamos estado y cableamos
// los botones. No debe robar el foco: eso lo garantiza WS_EX_NOACTIVATE en Rust.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";

interface PlayerState {
  status: string;
  paused: boolean;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

function render(state: PlayerState) {
  $("status").textContent = state.status || "Reproduciendo…";
  $("pause_ico").textContent = state.paused ? "▶" : "⏸";
  $("pause_label").textContent = state.paused ? "Reanudar" : "Pausa";
}

async function main() {
  // Estado inicial (la ventana pudo crearse antes de recibir eventos).
  try {
    render(await invoke<PlayerState>("player_state"));
  } catch {
    render({ status: "", paused: false });
  }

  // Actualizaciones en vivo (pausa/reanuda, nueva lectura).
  await listen<PlayerState>("player://update", (e) => render(e.payload));

  $("pause").addEventListener("click", () => {
    // Reflejo optimista; el backend confirmará por evento.
    const resuming = $("pause_label").textContent === "Reanudar";
    render({ status: $("status").textContent ?? "", paused: !resuming });
    void invoke("player_pause_resume");
  });

  $("stop").addEventListener("click", () => {
    // El backend cierra la ventana al detener.
    void invoke("player_stop");
  });

  $("close").addEventListener("click", () => {
    // Cerrar solo descarta el widget; la lectura sigue (se puede parar con el
    // atajo o la bandeja). La ventana se recrea en la siguiente lectura.
    void getCurrentWindow().close();
  });
}

window.addEventListener("DOMContentLoaded", () => {
  void main();
});
