// YappyLike — placeholder de frontend.
// M1 arranca sin ninguna ventana (solo tray). Las pantallas reales
// (Welcome, Settings, Mini player) se agregan en M4/M6/M7.
window.addEventListener("DOMContentLoaded", () => {
  const app = document.querySelector<HTMLElement>("#app");
  if (app) app.textContent = "YappyLike";
});
