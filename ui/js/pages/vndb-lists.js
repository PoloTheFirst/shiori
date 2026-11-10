import { $, el, html } from "../dom.js";
import { lengthText } from "../format.js";
import { register, show } from "../router.js";
import { store } from "../store.js";
import { call, setCover, toggle } from "../ui.js";
import { t } from "../i18n.js";
import { invoke } from "../tauri.js";

const problem = (error) => t(typeof error === "string" ? error : error?.detail || String(error));

const STALE_AFTER_MS = 10 * 60 * 1000;
const WISHLIST = 5;
const HIDDEN_LABELS = [6];

const TEMPLATE = `
  <button class="back" data-id="back">← Library</button>
  <div class="page-head">
    <h2>My VNDB lists</h2>
    <div class="row">
      <input type="search" data-id="filter" placeholder="Filter…" />
      <select data-id="sort">
        <option value="az">A–Z</option>
        <option value="rating">Rating</option>
        <option value="year">Release date</option>
        <option value="length">Length</option>
      </select>
      <button class="btn" data-id="random">Random pick</button>
    </div>
  </div>
  <div class="vndb-labels" data-id="labels"></div>
  <div class="row vndb-tools" data-id="tools"></div>
  <div class="subtle small-text" data-id="status"></div>
  <div class="lib-list view-grid" data-id="grid"></div>
`;

let root;
let labels = [];
let label = WISHLIST;
let loadedLabelsAt = 0;
const lists = new Map();
let hideOwned = false;

const part = (id) => $(`[data-id="${id}"]`, root);

function build(container) {
  root = container;
  root.append(html(TEMPLATE));
  part("back").onclick = () => show("library");
  part("filter").oninput = render;
  part("sort").onchange = render;
  part("random").onclick = () => {
    const current = labels.find((l) => l.id === label);
    show("wishlist", { label, name: current?.label || "", from: "vndb-lists" });
  };
  part("tools").append(
    toggle("Hide VNs already in my Library", hideOwned, (on) => {
      hideOwned = on;
      render();
    }),
  );
}

function labelName(item) {
  return item.id < 10 ? t(item.label) : item.label;
}

function renderLabels() {
  part("labels").replaceChildren(
    ...labels.map((item) =>
      el(
        "button",
        {
          class: `chip vndb-label ${item.id === label ? "on" : ""}`,
          title: item.private ? t("Private list") : "",
          onclick: () => {
            label = item.id;
            renderLabels();
            loadList();
          },
        },
        item.private ? "🔒 " : "",
        labelName(item),
        el("span", { class: "count" }, String(item.count)),
      ),
    ),
  );
}

async function loadLabels() {
  if (labels.length && Date.now() - loadedLabelsAt < STALE_AFTER_MS) {
    return true;
  }
  part("status").textContent = t("Loading your lists…");
  try {
    const result = await invoke("get_vndb_labels");
    labels = (result.labels || []).filter((item) => !HIDDEN_LABELS.includes(item.id));
    loadedLabelsAt = Date.now();
    if (!labels.some((item) => item.id === label)) {
      label = labels[0]?.id ?? WISHLIST;
    }
    renderLabels();
    return true;
  } catch (error) {
    part("status").textContent = problem(error);
    part("grid").replaceChildren(
      el("button", { class: "btn secondary", onclick: () => show("settings") }, "Open Settings"),
    );
    return false;
  }
}

async function loadList() {
  const wanted = label;
  const cached = lists.get(wanted);
  if (cached && Date.now() - cached.at < STALE_AFTER_MS) {
    render();
    return;
  }
  part("status").textContent = t("Loading…");
  part("grid").replaceChildren();
  try {
    const { vns, in_library } = await invoke("get_vndb_list", { label: wanted });
    lists.set(wanted, { vns, owned: new Set(in_library), at: Date.now() });
    if (wanted === label) {
      render();
    }
  } catch (error) {
    if (wanted === label) {
      part("status").textContent = problem(error);
    }
  }
}

const SORTS = {
  az: (a, b) => a.title.localeCompare(b.title),
  rating: (a, b) => b.rating - a.rating,
  year: (a, b) => String(b.year || "").localeCompare(String(a.year || "")),
  length: (a, b) => (a.length_minutes || 1e9) - (b.length_minutes || 1e9),
};

function render() {
  const list = lists.get(label);
  if (!list) {
    return;
  }
  const query = part("filter").value.trim().toLowerCase();
  const shown = list.vns
    .filter((vn) => !hideOwned || !list.owned.has(vn.id))
    .filter(
      (vn) =>
        !query ||
        [vn.title, vn.alt_title, ...(vn.other_titles || [])].some((title) =>
          String(title || "").toLowerCase().includes(query),
        ),
    )
    .sort(SORTS[part("sort").value] || SORTS.az);

  part("random").disabled = !list.vns.length;
  part("status").textContent = list.vns.length
    ? t("{count} VNs", { count: shown.length })
    : t("This list is empty on VNDB.");
  part("grid").replaceChildren(...shown.map((vn) => tile(vn, list.owned.has(vn.id))));
}

function tile(vn, owned) {
  const cover = el("div", { class: "cover" });
  setCover(cover, { url: vn.image_url, nsfw: vn.nsfw, label: vn.title });
  const rating = vn.rating > 10 ? vn.rating / 10 : vn.rating;
  const meta = [vn.year, vn.rating > 0 && `★ ${rating.toFixed(1)}`, lengthText(vn.length_minutes)]
    .filter(Boolean)
    .join(" · ");
  return el(
    "div",
    {
      class: "lib-tile",
      tabindex: "0",
      title: vn.alt_title && vn.alt_title !== vn.title ? vn.alt_title : "",
      onclick: () => call("open_url", { url: `https://vndb.org/${vn.id}` }),
      onkeydown: (event) => event.key === "Enter" && call("open_url", { url: `https://vndb.org/${vn.id}` }),
    },
    el(
      "div",
      { class: "tile-cover" },
      cover,
      owned && el("span", { class: "chip status" }, t("In your Library")),
    ),
    el("div", { class: "name" }, vn.title),
    el("div", { class: "muted small-text" }, meta),
  );
}

async function load() {
  if (!store.settings.vndb_token?.trim()) {
    labels = [];
    part("labels").replaceChildren();
    part("status").textContent = t("Add your VNDB token in Settings to see your lists.");
    part("grid").replaceChildren(
      el("button", { class: "btn secondary", onclick: () => show("settings") }, "Open Settings"),
    );
    part("random").disabled = true;
    return;
  }
  if (await loadLabels()) {
    loadList();
  }
}

register({
  id: "vndb-lists",
  nav: "library",
  build,
  show: load,
  onKey: (event) => {
    if (event.key === "Escape") {
      show("library");
    }
  },
});
