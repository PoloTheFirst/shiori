import { $, $$ } from "./dom.js";
import { invoke, listen, currentWindow } from "./tauri.js";
import { store, update, subscribe } from "./store.js";
import { show } from "./router.js";
import { resolveTheme, applyPalette } from "./themes.js";
import { applyBackground } from "./background.js";
import { call, toast } from "./ui.js";
import { offerUpdate } from "./components/update.js";
import { offerRecap } from "./components/recap.js";
import { watchNewVns } from "./components/new-vn.js";
import { welcome } from "./components/welcome.js";
import { loadLanguage, t, translateTree } from "./i18n.js";

import "./pages/home.js";
import "./pages/cover.js";
import "./pages/library.js";
import "./pages/game.js";
import "./pages/wishlist.js";
import "./pages/foryou.js";
import "./pages/screenshots.js";
import "./pages/share.js";
import "./pages/mascots.js";
import "./pages/settings.js";
import "./pages/whatsnew.js";
import "./labs/index.js";

// F11: full screen, like a browser.
document.addEventListener("keydown", async (event) => {
  if (event.key !== "F11") {
    return;
  }
  event.preventDefault();
  const window = currentWindow();
  await window.setFullscreen(!(await window.isFullscreen()));
});

function applyTheme() {
  const palette = resolveTheme(store.settings.theme || "system", store.settings.custom_theme);
  applyPalette(palette);
  applyBackground(store.settings);
  currentWindow()
    .setTheme(palette.mode)
    .catch(() => {});
}

matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => {
  if ((store.settings.theme || "system") === "system") {
    applyTheme();
  }
});

subscribe("settings", applyTheme);

for (const button of $$("nav button")) {
  button.onclick = () => show(button.dataset.page);
}

$("#pause-btn").onclick = () => call("set_paused", { paused: !store.snapshot.paused });

function renderSidebar() {
  const snap = store.snapshot;
  $("#pause-label").textContent = snap.paused ? t("Resume") : t("Pause");
  $("#pause-btn").classList.toggle("paused", !!snap.paused);
  $("#pause-btn").title = snap.paused ? t("Resume") : t("Pause");
  $("#pill-game").className = "pill" + (snap.detected ? " ok" : "");
}

function renderDiscordPill() {
  const status = store.status;
  $("#pill-discord").className = "pill " + (status.discord_ok ? "ok" : "bad");
  $("#pill-discord").title = status.discord_msg || "";
}

subscribe("snapshot", renderSidebar);
subscribe("status", renderDiscordPill);

subscribe("notice", (notice) => toast(notice.title, { error: !notice.ok }));

async function showWhatsNewOnce() {
  const last = store.settings.last_seen_version;
  if (last === store.version) {
    return false;
  }
  const changes = { last_seen_version: store.version };
  if (!last) {
    changes.welcome_done = false;
  }
  await invoke("update_settings", { changes });
  update("settings", { ...store.settings, ...changes });

  const updated = last && last !== store.version;
  if (updated) {
    show("whatsnew", { since: last });
  }
  return updated;
}

async function start() {
  const state = await invoke("get_state");
  store.language = state.language;
  await loadLanguage(state.language);
  translateTree(document.body);

  store.version = state.version;
  store.dataDir = state.data_dir;
  store.autostart = state.autostart;
  store.screenshotRoot = state.screenshot_root;
  store.defaultScreenshotRoot = state.default_screenshot_root;
  update("settings", state.settings);
  update("snapshot", state.snapshot);
  update("status", state.status);

  await listen("snapshot", (event) => update("snapshot", event.payload));
  await listen("status", (event) => update("status", event.payload));
  await listen("settings-changed", (event) => update("settings", event.payload));
  await listen("library-changed", (event) => update("library", event.payload));
  await listen("screenshots-changed", (event) => update("screenshots", event.payload));
  await listen("navigate", (event) => show(event.payload));
  await listen("toast", (event) => update("notice", event.payload));

  const showedNotes = await showWhatsNewOnce();
  if (!showedNotes) {
    show(state.pending_page || "home");
  }
  if (store.settings.welcome_done === false) {
    await welcome();
  }
  watchNewVns();

  if (store.settings.check_updates) {
    await offerUpdate();
  }
  offerRecap();
}

start();
