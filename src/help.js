// Window controls for the help page. The main page does the same in main.js.
const appWindow = window.__TAURI__.window.getCurrentWindow();

const winMax = document.getElementById("win-max");

async function syncMaximizeButton() {
  const maximized = await appWindow.isMaximized();
  winMax.dataset.maximized = String(maximized);
  winMax.setAttribute("aria-label", maximized ? "Restore" : "Maximize");
  winMax.title = maximized ? "Restore" : "Maximize";
}

document.getElementById("win-min").addEventListener("click", () => appWindow.minimize());
winMax.addEventListener("click", () => appWindow.toggleMaximize());
document.getElementById("win-close").addEventListener("click", () => appWindow.close());

appWindow.onResized(syncMaximizeButton);
syncMaximizeButton();