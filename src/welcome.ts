// Ventana Welcome (primer arranque): descarga del modelo con progreso.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface DownloadInfo {
  needed: boolean;
  target_dir: string;
  approx_mb: number;
}

interface ProgressPayload {
  downloaded: number;
  total: number;
  percent: number;
  file: string;
  file_index: number;
  file_count: number;
}

const $ = (id: string) => document.getElementById(id) as HTMLElement;

function mb(bytes: number): string {
  return (bytes / (1024 * 1024)).toFixed(1);
}

function show(el: HTMLElement, on: boolean) {
  el.classList.toggle("hidden", !on);
}

async function main() {
  const info = await invoke<DownloadInfo>("download_info");
  $("size").textContent = String(info.approx_mb);
  $("target").textContent = info.target_dir;

  const startBtn = $("start") as HTMLButtonElement;
  const cancelBtn = $("cancel") as HTMLButtonElement;
  const closeBtn = $("close") as HTMLButtonElement;
  const retryBtn = $("retry") as HTMLButtonElement;

  function beginUi() {
    show(startBtn, false);
    show(retryBtn, false);
    show(cancelBtn, true);
    show($("progress-area"), true);
    $("status").textContent = "Iniciando descarga…";
    $("fill").style.width = "0%";
  }

  function markReady() {
    document.body.classList.add("done");
    $("title").textContent = "¡Todo listo!";
    $("intro").textContent =
      "El modelo de voz está instalado. Selecciona texto en cualquier app y pulsa " +
      "Ctrl+Shift+Espacio para escucharlo.";
    show($("progress-area"), false);
    show(startBtn, false);
    show(cancelBtn, false);
    show(retryBtn, false);
    show(closeBtn, true);
  }

  function markError(message: string) {
    $("status").textContent = message;
    show(cancelBtn, false);
    show(retryBtn, true);
  }

  startBtn.addEventListener("click", () => {
    beginUi();
    invoke("start_download");
  });
  retryBtn.addEventListener("click", () => {
    beginUi();
    invoke("start_download");
  });
  cancelBtn.addEventListener("click", () => invoke("cancel_download"));
  closeBtn.addEventListener("click", () => getCurrentWindow().close());

  // Primer arranque: si faltan modelos, arranca la descarga automáticamente
  // (el usuario puede cancelar). Si ya está todo, muestra "listo".
  if (info.needed) {
    beginUi();
    invoke("start_download");
  } else {
    markReady();
  }

  await listen<ProgressPayload>("download://progress", (e) => {
    const p = e.payload;
    $("fill").style.width = `${p.percent.toFixed(1)}%`;
    $("status").textContent =
      `Descargando ${p.file_index + 1}/${p.file_count} — ${p.file}  ` +
      `(${mb(p.downloaded)} / ${mb(p.total)} MB, ${p.percent.toFixed(0)}%)`;
  });

  await listen("download://done", () => markReady());
  await listen<{ message: string }>("download://error", (e) =>
    markError(e.payload.message),
  );
}

window.addEventListener("DOMContentLoaded", () => {
  main().catch((err) => {
    const s = document.getElementById("status");
    if (s) s.textContent = `Error: ${err}`;
  });
});
