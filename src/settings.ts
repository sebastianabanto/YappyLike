// Ventana de Ajustes (M6): carga la config, permite editarla y la guarda.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Config {
  general: {
    start_with_windows: boolean;
    launch_minimized: boolean;
    show_notifications: boolean;
  };
  voice: { voice: string; lang: string; speed: number };
  hotkeys: {
    read: string;
    read_clipboard: string;
    stop: string;
    pause_resume: string;
    settings: string;
  };
  audio: { output_device: string | null; volume: number };
  export: { dir: string | null };
  replacements: Replacement[];
}

interface Replacement {
  from: string;
  to: string;
  enabled: boolean;
}

interface EngineStatus {
  backend: string;
  model_dir: string;
  ready: boolean;
}

interface RegisterOutcome {
  action: string;
  hotkey: string;
  error: string | null;
}

interface SaveResult {
  saved: boolean;
  hotkey_conflicts: RegisterOutcome[];
  error: string | null;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const DEFAULT_DEVICE = "__default__";

function setStatus(msg: string, kind: "" | "ok" | "err" = "") {
  const el = $("status");
  el.textContent = msg;
  el.className = `status ${kind}`;
}

// --- Captura de atajos ---------------------------------------------------- //

/** Traduce un KeyboardEvent a la forma de texto que entiende el backend. */
function comboFromEvent(e: KeyboardEvent): string | null {
  const code = e.code;
  // Ignora pulsaciones que son solo un modificador.
  const modifierCodes = [
    "ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight",
    "AltLeft", "AltRight", "MetaLeft", "MetaRight",
  ];
  if (modifierCodes.includes(code)) return null;

  let key: string | null = null;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) key = code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) key = code;
  else if (code === "Space") key = "Space";
  else if (code === "Enter") key = "Enter";
  else if (code === "Tab") key = "Tab";
  else if (code === "Backspace") key = "Backspace";
  else if (code === "Delete") key = "Delete";
  else if (code === "Insert") key = "Insert";
  else if (code === "Home") key = "Home";
  else if (code === "End") key = "End";
  else if (code === "PageUp") key = "PageUp";
  else if (code === "PageDown") key = "PageDown";
  else if (code === "ArrowUp") key = "Up";
  else if (code === "ArrowDown") key = "Down";
  else if (code === "ArrowLeft") key = "Left";
  else if (code === "ArrowRight") key = "Right";
  if (!key) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Win");
  parts.push(key);
  return parts.join("+");
}

function wireHotkeyInput(input: HTMLInputElement) {
  const original = () => input.dataset.value ?? "";
  input.addEventListener("focus", () => {
    input.classList.add("capturing");
    input.value = "Pulsa una combinación…";
  });
  input.addEventListener("blur", () => {
    input.classList.remove("capturing");
    input.value = original();
  });
  input.addEventListener("keydown", (e) => {
    e.preventDefault();
    if (e.key === "Escape") {
      input.blur();
      return;
    }
    const combo = comboFromEvent(e);
    if (combo) {
      input.dataset.value = combo;
      input.value = combo;
      input.blur();
    }
  });
}

// --- Carga y guardado ----------------------------------------------------- //

async function populate() {
  const [cfg, voices, devices, engine, version] = await Promise.all([
    invoke<Config>("get_settings"),
    invoke<string[]>("list_voices"),
    invoke<string[]>("list_output_devices"),
    invoke<EngineStatus>("engine_status"),
    invoke<string>("app_version"),
  ]);

  $("app_version").textContent = `v${version}`;

  // General
  ($("start_with_windows") as HTMLInputElement).checked = cfg.general.start_with_windows;
  ($("launch_minimized") as HTMLInputElement).checked = cfg.general.launch_minimized;
  ($("show_notifications") as HTMLInputElement).checked = cfg.general.show_notifications;

  // Voice
  const voiceSel = $("voice") as HTMLSelectElement;
  voiceSel.innerHTML = "";
  for (const v of voices) {
    const opt = document.createElement("option");
    opt.value = v;
    opt.textContent = v;
    voiceSel.appendChild(opt);
  }
  voiceSel.value = cfg.voice.voice;
  ($("lang") as HTMLSelectElement).value = cfg.voice.lang;

  const speed = $("speed") as HTMLInputElement;
  speed.value = String(cfg.voice.speed);
  const syncSpeed = () => ($("speed_val").textContent = `${Number(speed.value).toFixed(2)}×`);
  speed.addEventListener("input", syncSpeed);
  syncSpeed();

  // Hotkeys
  const hk = cfg.hotkeys;
  setHotkey("hk_read", hk.read);
  setHotkey("hk_read_clipboard", hk.read_clipboard);
  setHotkey("hk_stop", hk.stop);
  setHotkey("hk_pause_resume", hk.pause_resume);
  setHotkey("hk_settings", hk.settings);

  // Engine
  $("model_dir").textContent = engine.model_dir;
  $("engine_state").textContent = engine.ready
    ? "Listo"
    : "Modelo no descargado";

  // Audio
  const devSel = $("device") as HTMLSelectElement;
  devSel.innerHTML = "";
  const def = document.createElement("option");
  def.value = DEFAULT_DEVICE;
  def.textContent = "(Predeterminado del sistema)";
  devSel.appendChild(def);
  for (const d of devices) {
    const opt = document.createElement("option");
    opt.value = d;
    opt.textContent = d;
    devSel.appendChild(opt);
  }
  devSel.value = cfg.audio.output_device ?? DEFAULT_DEVICE;
  if (devSel.selectedIndex < 0) devSel.value = DEFAULT_DEVICE;

  const vol = $("volume") as HTMLInputElement;
  vol.value = String(cfg.audio.volume);
  const syncVol = () => ($("volume_val").textContent = `${Math.round(Number(vol.value) * 100)}%`);
  vol.addEventListener("input", syncVol);
  syncVol();

  // Export
  ($("export_dir") as HTMLInputElement).value = cfg.export.dir ?? "";

  // Pronunciación
  replacements = cfg.replacements ?? [];
  updateReplSummary();
}

function setHotkey(id: string, value: string) {
  const input = $(id) as HTMLInputElement;
  input.dataset.value = value;
  input.value = value;
}

// --- Diccionario de pronunciación (feature #7) ---------------------------- //

/** Estado en memoria de los reemplazos; se persiste al pulsar Guardar. */
let replacements: Replacement[] = [];

/** Resumen en la sección Pronunciación (fuera del modal). */
function updateReplSummary() {
  const valid = replacements.filter((r) => r.from.trim());
  const active = valid.filter((r) => r.enabled).length;
  $("repl_summary").textContent =
    valid.length === 0
      ? "Sin reemplazos"
      : `${valid.length} reemplazo${valid.length === 1 ? "" : "s"} · ${active} activo${active === 1 ? "" : "s"}`;
}

/** Dibuja la tabla del modal, filtrando por el buscador (palabra original). */
function renderReplTable() {
  const q = ($("repl_search") as HTMLInputElement).value.trim().toLowerCase();
  const tbody = $("repl_tbody");
  tbody.innerHTML = "";
  let shown = 0;
  for (const r of replacements) {
    if (q && !r.from.toLowerCase().includes(q)) continue;
    shown++;
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="col-en"><input type="checkbox" class="repl-en" title="Activar" /></td>
      <td><input type="text" class="repl-from" placeholder="palabra" /></td>
      <td><input type="text" class="repl-to" placeholder="reemplazo" /></td>
      <td class="col-del"><button type="button" class="repl-del" title="Quitar">✕</button></td>
    `;
    const en = tr.querySelector(".repl-en") as HTMLInputElement;
    const from = tr.querySelector(".repl-from") as HTMLInputElement;
    const to = tr.querySelector(".repl-to") as HTMLInputElement;
    en.checked = r.enabled;
    from.value = r.from;
    to.value = r.to;
    en.addEventListener("change", () => {
      r.enabled = en.checked;
    });
    from.addEventListener("input", () => {
      r.from = from.value;
    });
    to.addEventListener("input", () => {
      r.to = to.value;
    });
    tr.querySelector(".repl-del")!.addEventListener("click", () => {
      const i = replacements.indexOf(r);
      if (i >= 0) replacements.splice(i, 1);
      renderReplTable();
    });
    tbody.appendChild(tr);
  }
  $("repl_count").textContent = q
    ? `${shown} de ${replacements.length} coincidencia${shown === 1 ? "" : "s"}`
    : `${replacements.length} palabra${replacements.length === 1 ? "" : "s"}`;
}

/** Importa pares `palabra,reemplazo` del textarea, actualizando duplicados. */
function importCsv() {
  const raw = ($("repl_csv") as HTMLTextAreaElement).value;
  const byFrom = new Map<string, Replacement>();
  for (const r of replacements) byFrom.set(r.from.trim().toLowerCase(), r);

  let added = 0;
  let updated = 0;
  let skipped = 0;
  for (const line of raw.split(/\r?\n/)) {
    const t = line.trim();
    if (!t) continue;
    const idx = t.indexOf(",");
    if (idx < 0) {
      skipped++;
      continue;
    }
    const from = t.slice(0, idx).trim();
    const to = t.slice(idx + 1).trim();
    if (!from) {
      skipped++;
      continue;
    }
    const key = from.toLowerCase();
    const existing = byFrom.get(key);
    if (existing) {
      existing.to = to;
      existing.enabled = true;
      updated++;
    } else {
      const nr: Replacement = { from, to, enabled: true };
      replacements.push(nr);
      byFrom.set(key, nr);
      added++;
    }
  }
  const parts = [`Añadidas: ${added}`, `Actualizadas: ${updated}`];
  if (skipped) parts.push(`Ignoradas: ${skipped}`);
  $("repl_import_status").textContent = parts.join(" · ");
  ($("repl_csv") as HTMLTextAreaElement).value = "";
  renderReplTable();
}

function openReplModal() {
  ($("repl_search") as HTMLInputElement).value = "";
  $("repl_import_panel").classList.add("hidden");
  $("repl_import_status").textContent = "";
  renderReplTable();
  $("repl_modal").classList.remove("hidden");
}

function closeReplModal() {
  $("repl_modal").classList.add("hidden");
  updateReplSummary();
}

function collect(): Config {
  const devSel = $("device") as HTMLSelectElement;
  const device = devSel.value === DEFAULT_DEVICE ? null : devSel.value;
  return {
    general: {
      start_with_windows: ($("start_with_windows") as HTMLInputElement).checked,
      launch_minimized: ($("launch_minimized") as HTMLInputElement).checked,
      show_notifications: ($("show_notifications") as HTMLInputElement).checked,
    },
    voice: {
      voice: ($("voice") as HTMLSelectElement).value,
      lang: ($("lang") as HTMLSelectElement).value,
      speed: Number(($("speed") as HTMLInputElement).value),
    },
    hotkeys: {
      read: ($("hk_read") as HTMLInputElement).dataset.value ?? "",
      read_clipboard: ($("hk_read_clipboard") as HTMLInputElement).dataset.value ?? "",
      stop: ($("hk_stop") as HTMLInputElement).dataset.value ?? "",
      pause_resume: ($("hk_pause_resume") as HTMLInputElement).dataset.value ?? "",
      settings: ($("hk_settings") as HTMLInputElement).dataset.value ?? "",
    },
    audio: {
      output_device: device,
      volume: Number(($("volume") as HTMLInputElement).value),
    },
    export: {
      dir: (($("export_dir") as HTMLInputElement).value.trim() || null),
    },
    replacements: replacements
      .filter((r) => r.from.trim())
      .map((r) => ({ from: r.from.trim(), to: r.to, enabled: r.enabled })),
  };
}

async function save() {
  const saveBtn = $("save") as HTMLButtonElement;
  saveBtn.disabled = true;
  setStatus("Guardando…");
  try {
    const res = await invoke<SaveResult>("save_settings", { config: collect() });
    if (res.error) {
      setStatus(res.error, "err");
    } else if (res.hotkey_conflicts.length > 0) {
      const list = res.hotkey_conflicts
        .map((c) => `${c.hotkey} (${c.action})`)
        .join(", ");
      setStatus(`Guardado, pero estos atajos están ocupados: ${list}`, "err");
    } else {
      setStatus("Ajustes guardados.", "ok");
    }
  } catch (e) {
    setStatus(`Error al guardar: ${e}`, "err");
  } finally {
    saveBtn.disabled = false;
  }
}

async function main() {
  ["hk_read", "hk_read_clipboard", "hk_stop", "hk_pause_resume", "hk_settings"].forEach((id) =>
    wireHotkeyInput($(id) as HTMLInputElement),
  );

  await populate();

  $("preview").addEventListener("click", () => {
    invoke("preview_voice", {
      voice: ($("voice") as HTMLSelectElement).value,
      lang: ($("lang") as HTMLSelectElement).value,
      speed: Number(($("speed") as HTMLInputElement).value),
    });
  });
  // Diccionario de pronunciación (modal).
  $("repl_manage").addEventListener("click", openReplModal);
  $("repl_close").addEventListener("click", closeReplModal);
  $("repl_done").addEventListener("click", closeReplModal);
  $("repl_modal").addEventListener("click", (e) => {
    if (e.target === $("repl_modal")) closeReplModal();
  });
  $("repl_search").addEventListener("input", renderReplTable);
  $("repl_import_toggle").addEventListener("click", () => {
    $("repl_import_panel").classList.toggle("hidden");
  });
  $("repl_import_do").addEventListener("click", importCsv);
  $("repl_addrow").addEventListener("click", () => {
    replacements.push({ from: "", to: "", enabled: true });
    ($("repl_search") as HTMLInputElement).value = "";
    renderReplTable();
  });
  $("save").addEventListener("click", () => void save());
  $("cancel").addEventListener("click", () => getCurrentWindow().close());
}

window.addEventListener("DOMContentLoaded", () => {
  main().catch((err) => setStatus(`Error: ${err}`, "err"));
});
