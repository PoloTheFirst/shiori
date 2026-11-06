import { $, el, html } from "../dom.js";
import { invoke } from "../tauri.js";
import { store, subscribe } from "../store.js";
import { call, segmented, setCover, toggle } from "../ui.js";
import { t } from "../i18n.js";

const VARIABLES = [
  ["title", "VN title"],
  ["romanized", "Romanized title"],
  ["original", "Original title"],
  ["reading", "“Reading”"],
  ["section", "Chapter or route"],
  ["total", "Total time read"],
  ["today", "Time read today"],
  ["total_read", "“Total read: …”"],
  ["today_read", "“Today: …”"],
  ["progress", "How far you are"],
  ["length", "Length on VNDB"],
  ["status", "Library status"],
  ["rating", "VNDB rating"],
  ["year", "Release year"],
  ["engine", "Engine"],
  ["vndb", "VNDB link"],
];

const PRESETS = {
  default: {
    name: "{title}",
    details: "{reading}・{section}",
    state: "{total_read} · {progress}",
    large_text: "{romanized}",
    small_icon: "",
    small_text: "",
    timer: "session",
    buttons: [],
  },
  compact: {
    name: "{title}",
    details: "{section}",
    state: "",
    large_text: "{title}",
    small_icon: "",
    small_text: "",
    timer: "session",
  },
  stats: {
    name: "{title}",
    details: "{total} · {progress}",
    state: "{today_read}",
    large_text: "{title} ({year}) · ★ {rating}",
    small_icon: "app",
    small_text: "{engine}",
    timer: "total",
  },
  original: {
    name: "{original}",
    details: "{romanized}",
    state: "{section}",
    large_text: "{original}",
    small_icon: "",
    small_text: "",
    timer: "session",
  },
};

const TEMPLATE = `
  <div class="pb-head">
    <span data-id="enabled"></span>
  </div>
  <p class="muted">Write each line yourself. Click a variable to insert it where your cursor is. A part between separators (· | ・ -) disappears when its variables are empty.</p>

  <div class="pb-layout">
    <div class="pb-form">
      <div class="card pb-card">
        <div class="pb-presets">
          <span class="muted small-text">Start from</span>
          <button class="btn secondary small" data-preset="default">Default</button>
          <button class="btn secondary small" data-preset="compact">Compact</button>
          <button class="btn secondary small" data-preset="stats">Stats</button>
          <button class="btn secondary small" data-preset="original">Original title</button>
        </div>

        <label class="pb-field"><span>Top line (in bold)</span><input type="text" data-key="name" spellcheck="false" /></label>
        <label class="pb-field"><span>Second line</span><input type="text" data-key="details" spellcheck="false" /></label>
        <label class="pb-field"><span>Third line</span><input type="text" data-key="state" spellcheck="false" /></label>
        <label class="pb-field"><span>Text on the cover (when hovered)</span><input type="text" data-key="large_text" spellcheck="false" /></label>

        <div class="pb-field">
          <span>Small icon on the cover</span>
          <div class="pb-row">
            <select data-id="icon">
              <option value="">None</option>
              <option value="app">App logo</option>
              <option value="url">Image link…</option>
            </select>
            <input type="text" data-id="icon-url" placeholder="https://…" spellcheck="false" hidden />
          </div>
        </div>
        <label class="pb-field" data-id="small-text-field"><span>Text on the small icon (when hovered)</span><input type="text" data-key="small_text" spellcheck="false" /></label>

        <div class="pb-field">
          <span>Timer</span>
          <div class="seg" data-id="timer">
            <button data-v="session">Since you started</button>
            <button data-v="total">Total time read</button>
            <button data-v="off">Off</button>
          </div>
        </div>

        <div class="pb-field">
          <span>Buttons (up to 2)</span>
          <div class="pb-row"><input type="text" data-button="0" data-part="label" placeholder="Label" /><input type="text" data-button="0" data-part="url" placeholder="https://… / {vndb}" spellcheck="false" /></div>
          <div class="pb-row"><input type="text" data-button="1" data-part="label" placeholder="Label" /><input type="text" data-button="1" data-part="url" placeholder="https://…" spellcheck="false" /></div>
          <span class="subtle small-text">Discord doesn't show buttons on your own profile, only to others.</span>
        </div>
      </div>

      <div class="card pb-card">
        <b>Variables</b>
        <div class="pb-vars" data-id="vars"></div>
      </div>
    </div>

    <div class="pb-side">
      <div class="pb-preview">
        <div class="pb-preview-head">
          <span>Playing</span>
          <span class="subtle small-text" data-id="source"></span>
        </div>
        <div class="pb-activity">
          <div class="pb-art">
            <div class="cover" data-id="cover"></div>
            <img class="pb-small" data-id="small" alt="" hidden />
            <span class="pb-tip" data-id="large-tip"></span>
          </div>
          <div class="pb-lines">
            <b data-id="p-name"></b>
            <span data-id="p-details"></span>
            <span data-id="p-state"></span>
            <span class="pb-timer" data-id="p-timer"></span>
          </div>
        </div>
        <div class="pb-buttons" data-id="p-buttons"></div>
      </div>
      <p class="subtle small-text" data-id="partial-hint">VNs set to “Partial” or “Private” privacy still show less, as before.</p>
    </div>
  </div>
`;

let root;
let custom = null;
let lastField = null;
let saveTimer = 0;
let previewJob = 0;
let timerStart = 0;
let iconMode = "";

const part = (id) => $(`[data-id="${id}"]`, root);

function current() {
  return {
    enabled: !!custom.enabled,
    name: custom.name ?? "",
    details: custom.details ?? "",
    state: custom.state ?? "",
    large_text: custom.large_text ?? "",
    small_icon: custom.small_icon ?? "",
    small_text: custom.small_text ?? "",
    timer: custom.timer || "session",
    buttons: (custom.buttons || []).slice(0, 2),
  };
}

function load() {
  custom = { ...PRESETS.default, ...(store.settings.presence_custom || {}) };
  const icon = custom.small_icon || "";
  iconMode = icon === "" || icon === "app" ? icon : "url";
}

function fillForm() {
  for (const input of root.querySelectorAll("[data-key]")) {
    input.value = custom[input.dataset.key] ?? "";
  }
  part("icon").value = iconMode;
  part("icon-url").hidden = iconMode !== "url";
  if (iconMode === "url") {
    part("icon-url").value = custom.small_icon || "";
  }
  part("small-text-field").hidden = !iconMode;
  for (const input of root.querySelectorAll("[data-button]")) {
    const button = (custom.buttons || [])[Number(input.dataset.button)] || {};
    input.value = button[input.dataset.part] || "";
  }
  segmented(part("timer"), custom.timer || "session", (value) => {
    custom.timer = value;
    fillForm();
    changed();
  });
  part("enabled").replaceChildren(
    toggle("Use my layout on Discord", !!custom.enabled, (on) => {
      custom.enabled = on;
      changed();
    }),
  );
}

function readButtons() {
  const buttons = [0, 1].map((i) => {
    const get = (name) => root.querySelector(`[data-button="${i}"][data-part="${name}"]`).value;
    return { label: get("label"), url: get("url") };
  });
  custom.buttons = buttons.filter((b) => b.label || b.url);
}

function changed() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => call("update_settings", { changes: { presence_custom: current() } }), 400);
  preview();
}

async function preview() {
  const job = ++previewJob;
  const result = await invoke("preview_presence", { custom: current() }).catch(() => null);
  if (!result || job !== previewJob) {
    return;
  }
  const activity = result.activity;
  const sources = {
    live: t("your VN right now"),
    last: t("your last VN (no VN running)"),
    sample: t("example values (no VN running)"),
  };
  part("source").textContent = sources[result.source] || "";
  part("p-name").textContent = activity.name;
  part("p-details").textContent = activity.details;
  part("p-state").textContent = activity.state;
  part("p-details").hidden = !activity.details;
  part("p-state").hidden = !activity.state;
  part("large-tip").textContent = activity.large_text;
  part("large-tip").hidden = !activity.large_text;

  const small = part("small");
  small.hidden = !activity.small_image;
  if (activity.small_image) {
    small.src = activity.small_image.startsWith("https://raw.githubusercontent.com/lennmoe/shiori/")
      ? "assets/app_icon.png"
      : activity.small_image;
    small.title = activity.small_text || "";
  }

  setCover(part("cover"), result.cover ? { path: result.cover, label: activity.name } : { url: "assets/app_icon.png" });

  timerStart = activity.start || 0;
  tick();

  part("p-buttons").replaceChildren(
    ...(activity.buttons || []).map(([label, url]) =>
      el("button", { class: "pb-button", title: url, onclick: () => call("open_url", { url }) }, label),
    ),
  );

  for (const chip of part("vars").children) {
    const value = result.vars[chip.dataset.name];
    chip.title = value ? value : t("empty right now");
    chip.classList.toggle("empty", !value);
  }
}

function tick() {
  const node = part("p-timer");
  if (!node) {
    return;
  }
  node.hidden = !timerStart;
  if (!timerStart) {
    return;
  }
  const pad = (n) => String(n).padStart(2, "0");
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - timerStart);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  node.textContent = `${hours ? `${hours}:${pad(minutes)}` : minutes}:${pad(seconds % 60)}`;
}

setInterval(tick, 1000);

function insertVariable(name) {
  const input = lastField || root.querySelector('[data-key="details"]');
  const token = `{${name}}`;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  input.value = input.value.slice(0, start) + token + input.value.slice(end);
  input.focus();
  input.setSelectionRange(start + token.length, start + token.length);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function build(container) {
  root = container;
  root.append(html(TEMPLATE));

  part("vars").append(
    ...VARIABLES.map(([name, label]) =>
      el(
        "button",
        { class: "pb-var", "data-name": name, onmousedown: (e) => e.preventDefault(), onclick: () => insertVariable(name) },
        el("code", {}, `{${name}}`),
        el("span", {}, label),
      ),
    ),
  );

  for (const input of root.querySelectorAll("[data-key]")) {
    input.addEventListener("focus", () => (lastField = input));
    input.addEventListener("input", () => {
      custom[input.dataset.key] = input.value;
      changed();
    });
  }
  for (const input of root.querySelectorAll("[data-button]")) {
    input.addEventListener("focus", () => (lastField = input));
    input.addEventListener("input", () => {
      readButtons();
      changed();
    });
  }
  part("small").onerror = () => (part("small").hidden = true);
  part("icon").onchange = () => {
    iconMode = part("icon").value;
    custom.small_icon = iconMode === "url" ? part("icon-url").value : iconMode;
    fillForm();
    if (part("icon").value === "url") {
      part("icon-url").focus();
    }
    changed();
  };
  part("icon-url").addEventListener("input", () => {
    custom.small_icon = part("icon-url").value;
    changed();
  });

  for (const button of root.querySelectorAll("[data-preset]")) {
    button.onclick = () => {
      const preset = PRESETS[button.dataset.preset];
      custom = { ...custom, ...preset, buttons: custom.buttons };
      iconMode = custom.small_icon === "" || custom.small_icon === "app" ? custom.small_icon : "url";
      fillForm();
      changed();
    };
  }

}

let watching = false;

export function presenceBuilder() {
  const container = el("div", { class: "pb" });
  build(container);
  load();
  fillForm();
  preview();
  if (!watching) {
    watching = true;
    subscribe("snapshot", () => root?.isConnected && preview());
  }
  return container;
}

