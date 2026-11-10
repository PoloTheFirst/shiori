import { store, subscribe } from "../store.js";

function apply() {
  document.documentElement.classList.add("labs-hover");
  document.documentElement.classList.toggle("labs-compact", store.settings.labs_compact_sidebar === true);
  document.documentElement.classList.toggle("labs-topbar", store.settings.labs_topbar !== false);
}

subscribe("settings", apply);
