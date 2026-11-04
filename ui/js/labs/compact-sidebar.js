import { $, $$, el } from "../dom.js";
import { store } from "../store.js";
import { call } from "../ui.js";

const ICONS = {
  home: "M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6h-6v6H4a1 1 0 0 1-1-1z",
  library: "M4 3h6a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM7 3v18M20.4 18.9c.2.5-.1 1.1-.6 1.3l-1.9.7c-.5.2-1.1-.1-1.3-.6L11.1 5.1c-.2-.5.1-1.1.6-1.3l1.9-.7c.5-.2 1.1.1 1.3.6Z",
  foryou: "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0zM20 3v4M22 5h-4M4 17v2M5 18H3",
  screenshots: "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM9 11.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM21 16l-5-5-9 9",
  share: "M21 5a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM9 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM21 19a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM8.59 13.51l6.83 3.98M15.41 6.51l-6.82 3.98",
  mascots: "M12 5c.67 0 1.35.09 2 .26 1.78-2 5.03-2.84 6.42-2.26 1.4.58-.42 7-.42 7 .57 1.07 1 2.24 1 3.44C21 17.9 16.97 21 12 21s-9-3-9-7.56c0-1.25.5-2.4 1-3.44 0 0-1.89-6.42-.5-7 1.39-.58 4.72.23 6.5 2.23A9.04 9.04 0 0 1 12 5ZM8 14v.5M16 14v.5M11.25 16.25h1.5L12 17l-.75-.75Z",
  settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6A7 7 0 0 0 5 12a7 7 0 0 0 .1 1.2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6A7 7 0 0 0 19 12z",
};

function icon(path) {
  const box = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  box.setAttribute("viewBox", "0 0 24 24");
  box.setAttribute("class", "labs-nav-icon");
  const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
  line.setAttribute("d", path);
  box.append(line);
  return box;
}

for (const button of $$("nav button")) {
  const label = el("span", { class: "labs-nav-text" });
  label.append(...button.childNodes);
  button.title = label.textContent.trim();
  button.append(icon(ICONS[button.dataset.page] || ICONS.home), label);
}

const toggle = el(
  "button",
  { class: "btn secondary labs-compact-toggle", title: "Compact sidebar" },
  icon("M4 4h16v16H4zM9 4v16"),
);
const showArrow = () =>
  toggle.classList.toggle("on", document.documentElement.classList.contains("labs-compact"));
toggle.onclick = async () => {
  const compact = !document.documentElement.classList.contains("labs-compact");
  document.documentElement.classList.toggle("labs-compact", compact);
  showArrow();
  store.settings.labs_compact_sidebar = compact;
  await call("update_settings", { changes: { labs_compact_sidebar: compact } });
};
$(".side-bottom")?.append(toggle);
new MutationObserver(showArrow).observe(document.documentElement, { attributeFilter: ["class"] });
showArrow();
