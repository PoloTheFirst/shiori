import { $, el, html, svg } from "../dom.js";
import { lengthText } from "../format.js";
import { invoke, listen } from "../tauri.js";
import { register } from "../router.js";
import { store } from "../store.js";
import { call, segmented, setCover } from "../ui.js";
import { language as uiLanguage, t } from "../i18n.js";
import { vndbText } from "../labs/util.js";

const PAGE_SIZE = 24;
const GHOSTS = { details: 8, list: 10, covers: 18, cards: 12 };
const VIEWS = Object.keys(GHOSTS);
const POPULAR_VOTES = 1000;
const OPEN_ICON = "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5";
const HIDE_ICON =
  "M3 3l18 18M10.6 5.1A10 10 0 0 1 12 5c6 0 9.5 7 9.5 7a17 17 0 0 1-3.2 4.1M6.6 6.6A17 17 0 0 0 2.5 12s3.5 7 9.5 7a10 10 0 0 0 5.4-1.6M9.9 9.9a3 3 0 0 0 4.2 4.2";
const STAGES = {
  list: "Fetching your VNDB list…",
  favorites: "Fetching the VNs you liked…",
  taste: "Working out your taste…",
  search: "Searching VNDB… {done} / {total}",
  rank: "Ranking the results…",
};
const NO_FILTERS = {
  length: "",
  rating: "",
  released: "",
  language: "",
  developer: "",
  popularity: "",
};

const year = (vn) => Number(vn.year) || 0;
const CHOICES = {
  length: [
    ["short", "Under 10h", (vn) => vn.minutes > 0 && vn.minutes < 600],
    ["fair", "10 to 30h", (vn) => vn.minutes >= 600 && vn.minutes < 1800],
    ["long", "30 to 50h", (vn) => vn.minutes >= 1800 && vn.minutes < 3000],
    ["longer", "50 to 70h", (vn) => vn.minutes >= 3000 && vn.minutes < 4200],
    ["longest", "70h and more", (vn) => vn.minutes >= 4200],
  ],
  rating: [
    ["75", "7.5 and up", (vn) => vn.rating >= 7.5],
    ["80", "8.0 and up", (vn) => vn.rating >= 8],
    ["85", "8.5 and up", (vn) => vn.rating >= 8.5],
  ],
  released: [
    ["2020", "2020 and later", (vn) => year(vn) >= 2020],
    ["2010", "2010s", (vn) => year(vn) >= 2010 && year(vn) < 2020],
    ["2000", "2000s", (vn) => year(vn) >= 2000 && year(vn) < 2010],
    ["old", "Before 2000", (vn) => year(vn) > 0 && year(vn) < 2000],
  ],
  popularity: [
    ["known", "Well known", (vn) => vn.votes >= POPULAR_VOTES],
    ["gems", "Hidden gems", (vn) => vn.votes < POPULAR_VOTES],
  ],
};
const UNKNOWN_LAST = (value) => value || Number.MAX_SAFE_INTEGER;
const SORTS = [
  ["match", "Best match", () => 0],
  ["rating", "Highest rated", (a, b) => b.rating - a.rating],
  ["newest", "Newest", (a, b) => year(b) - year(a)],
  ["oldest", "Oldest", (a, b) => UNKNOWN_LAST(year(a)) - UNKNOWN_LAST(year(b))],
  ["shortest", "Shortest", (a, b) => UNKNOWN_LAST(a.minutes) - UNKNOWN_LAST(b.minutes)],
  ["longest", "Longest", (a, b) => b.minutes - a.minutes],
];

const viewIcon = (path) =>
  `<svg class="reco-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${path}"/></svg>`;

const TEMPLATE = `
  <div class="page-head">
    <div>
      <h2>For you</h2>
      <div class="muted" data-id="based"></div>
      <div class="muted small-text" data-id="partial" hidden>
        VNDB limited the number of requests, so this list may be incomplete. Try Refresh in a few minutes.
      </div>
    </div>
    <button class="btn secondary" data-id="refresh">Refresh</button>
  </div>

  <div class="reco-taste" data-id="taste-row" hidden>
    <b>Your taste</b>
    <div class="chips" data-id="taste"></div>
  </div>

  <div class="reco-filters" data-id="filters" hidden>
    <label class="reco-filter"><span>Length</span><select data-id="length"></select></label>
    <label class="reco-filter"><span>Rating</span><select data-id="rating"></select></label>
    <label class="reco-filter"><span>Released</span><select data-id="released"></select></label>
    <label class="reco-filter"><span>Language</span><select data-id="language"></select></label>
    <label class="reco-filter"><span>Developer</span><select data-id="developer"></select></label>
    <label class="reco-filter"><span>Popularity</span><select data-id="popularity"></select></label>
    <label class="reco-filter"><span>Sort by</span><select data-id="sort"></select></label>
    <button class="btn secondary small" data-id="reset" hidden>Reset filters</button>
  </div>

  <div class="reco-bar" data-id="bar" hidden>
    <div class="seg" data-id="signal">
      <button data-v="all">Combined</button>
      <button data-v="tags">Tags</button>
      <button data-v="studios">Studios</button>
      <button data-v="staff">Staff</button>
      <button data-v="voices">Voices</button>
    </div>
    <div class="seg reco-views" data-id="view">
      <button data-v="details" title="Detailed" aria-label="Detailed">${viewIcon("M4 5h6v6H4zM14 6h6M14 10h6M4 15h6v4H4zM14 15h6M14 19h6")}</button>
      <button data-v="list" title="List" aria-label="List">${viewIcon("M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01")}</button>
      <button data-v="covers" title="Covers" aria-label="Covers">${viewIcon("M4 4h4v4H4zM10 4h4v4h-4zM16 4h4v4h-4zM4 10h4v4H4zM10 10h4v4h-4zM16 10h4v4h-4zM4 16h4v4H4zM10 16h4v4h-4zM16 16h4v4h-4z")}</button>
      <button data-v="cards" title="Large covers" aria-label="Large covers">${viewIcon("M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z")}</button>
    </div>
  </div>

  <div class="row reco-count" data-id="count-row" hidden>
    <span class="muted" data-id="count"></span>
    <button class="btn secondary small" data-id="restore" hidden></button>
  </div>

  <div class="card reco-loading" data-id="loading" role="status" hidden>
    <b data-id="stage"></b>
    <div class="bar"><i data-id="loading-bar"></i></div>
    <span class="muted small-text" data-id="elapsed"></span>
  </div>
  <div class="reco-grid" data-id="grid"></div>
  <div class="card reco-message muted" data-id="message" hidden></div>
  <button class="btn secondary reco-more" data-id="more" hidden>Show more</button>
`;

let root;
let found = null;
let loading = false;
let filters = { ...NO_FILTERS };
let sort = "match";
let signal = "all";
let themes = new Set();
let shownCount = PAGE_SIZE;

const part = (id) => $(`[data-id="${id}"]`, root);
const hiddenIds = () => store.settings.reco_hidden || [];
const currentView = () => (VIEWS.includes(store.settings.reco_view) ? store.settings.reco_view : "details");

function build(container) {
  root = container;
  root.append(html(TEMPLATE));

  part("refresh").onclick = () => load(true);
  part("restore").onclick = () => saveHidden([]);
  part("reset").onclick = resetFilters;
  part("more").onclick = () => {
    shownCount += PAGE_SIZE;
    renderList();
  };
  for (const name of Object.keys(NO_FILTERS)) {
    part(name).onchange = (event) => {
      filters[name] = event.target.value;
      shownCount = PAGE_SIZE;
      renderList();
    };
  }
  part("sort").onchange = (event) => {
    sort = event.target.value;
    renderList();
  };
  listen("reco-progress", (event) => showStage(event.payload));
  fill("sort", SORTS.map(([value, label]) => ({ value, label: t(label) })), false);
  for (const name of Object.keys(CHOICES)) {
    fill(name, CHOICES[name].map(([value, label]) => ({ value, label: t(label) })));
  }
}

function fill(name, options, withAny = true) {
  const select = part(name);
  const items = withAny ? [{ value: "", label: t("Any") }, ...options] : options;
  select.replaceChildren(
    ...items.map(({ value, label }) => {
      const option = el("option", { value });
      option.textContent = label;
      return option;
    }),
  );
}

function languageName(code) {
  try {
    return new Intl.DisplayNames([uiLanguage], { type: "language" }).of(code) || code;
  } catch {
    return code;
  }
}

function countedOptions(vns, read, label = (value) => value) {
  const counts = new Map();
  for (const vn of vns) {
    const value = read(vn);
    if (value) {
      counts.set(value, (counts.get(value) || 0) + 1);
    }
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([value, count]) => ({ value, label: `${label(value)} (${count})` }));
}

function ghostCard(view) {
  const line = (width) => el("div", { class: "reco-ghost", style: { width, height: "13px" } });
  if (view === "list") {
    return el("div", { class: "reco-row" }, el("div", { class: "reco-ghost reco-cover" }), line("45%"));
  }
  if (view === "details") {
    return el(
      "div",
      { class: "card reco-card" },
      el("div", { class: "reco-ghost reco-cover" }),
      el("div", { class: "reco-info" }, line("70%"), line("50%"), line("90%"), line("85%"), line("60%")),
    );
  }
  return el("div", { class: "reco-tile" }, el("div", { class: "reco-ghost reco-cover" }), line("75%"));
}

function showStage({ stage, percent, done, total }) {
  if (loading && STAGES[stage]) {
    part("stage").textContent = t(STAGES[stage], { done, total });
    part("loading-bar").style.width = `${percent}%`;
  }
}

function showLoading() {
  for (const id of ["taste-row", "filters", "bar", "count-row", "more", "message"]) {
    part(id).hidden = true;
  }
  const view = currentView();
  part("grid").dataset.view = view;
  part("grid").setAttribute("aria-busy", "true");
  part("grid").replaceChildren(...Array.from({ length: GHOSTS[view] }, () => ghostCard(view)));
  part("stage").textContent = t("Fetching data…");
  part("loading-bar").style.width = "2%";

  const started = Date.now();
  const tick = () => {
    const seconds = Math.floor((Date.now() - started) / 1000);
    part("elapsed").textContent = t("This can take a few seconds.  {seconds} s", { seconds });
  };
  part("loading").hidden = false;
  tick();
  return setInterval(tick, 1000);
}

async function load(refresh = false) {
  if (loading) {
    return;
  }
  if (found && !refresh) {
    render();
    return;
  }

  loading = true;
  part("refresh").disabled = true;
  const timer = showLoading();
  try {
    found = await invoke("recommendations", { refresh });
    render();
  } catch (error) {
    part("grid").replaceChildren();
    message(`${t("Couldn't load the recommendations.")} ${error}`);
  } finally {
    clearInterval(timer);
    loading = false;
    part("refresh").disabled = false;
    part("loading").hidden = true;
    part("grid").removeAttribute("aria-busy");
  }
}

function message(text) {
  part("message").hidden = !text;
  part("message").textContent = text;
}

async function saveHidden(ids) {
  store.settings.reco_hidden = ids;
  renderList();
  await call("update_settings", { changes: { reco_hidden: ids } });
}

function resetFilters() {
  filters = { ...NO_FILTERS };
  themes = new Set();
  shownCount = PAGE_SIZE;
  syncSelects();
  renderTaste();
  renderList();
}

function syncSelects() {
  for (const [name, value] of Object.entries(filters)) {
    part(name).value = value;
  }
  part("sort").value = sort;
}

function renderTaste() {
  part("taste-row").hidden = !found.taste.length;
  part("taste").replaceChildren(
    ...found.taste.map((tag) => {
      const on = themes.has(tag);
      const chip = el("button", { class: `chip ${on ? "accent" : "quiet"}`, "aria-pressed": String(on) });
      chip.textContent = tag;
      chip.onclick = () => {
        if (on) {
          themes.delete(tag);
        } else {
          themes.add(tag);
        }
        shownCount = PAGE_SIZE;
        renderTaste();
        renderList();
      };
      return chip;
    }),
  );
}

function renderBar() {
  segmented(part("signal"), signal, (value) => {
    signal = value;
    shownCount = PAGE_SIZE;
    renderBar();
    renderList();
  });
  segmented(part("view"), currentView(), (value) => {
    store.settings.reco_view = value;
    renderBar();
    renderList();
    call("update_settings", { changes: { reco_view: value } });
  });
}

function render() {
  part("based").textContent = found.liked
    ? t("Based on the {count} VNs you liked most", { count: found.liked })
    : "";
  part("partial").hidden = !found.partial;
  part("filters").hidden = !found.vns.length;
  part("bar").hidden = !found.vns.length;

  fill("language", countedOptions(found.vns, (vn) => vn.language, languageName));
  fill("developer", countedOptions(found.vns, (vn) => vn.developer));
  if (![...part("language").options].some((o) => o.value === filters.language)) {
    filters.language = "";
  }
  if (![...part("developer").options].some((o) => o.value === filters.developer)) {
    filters.developer = "";
  }
  syncSelects();
  renderTaste();
  renderBar();
  renderList();
}

function matches(vn) {
  const chosen = (name) => {
    const choice = CHOICES[name].find(([value]) => value === filters[name]);
    return !choice || choice[2](vn);
  };
  return (
    chosen("length") &&
    chosen("rating") &&
    chosen("released") &&
    chosen("popularity") &&
    (!filters.language || vn.language === filters.language) &&
    (!filters.developer || vn.developer === filters.developer) &&
    (signal === "all" || vn.signals[signal] > 0) &&
    [...themes].every((theme) => vn.matched.includes(theme))
  );
}

const strength = (vn) => (signal === "all" ? vn.score : vn.signals[signal]);

function renderList() {
  if (!found) {
    return;
  }
  const hidden = hiddenIds();
  const compare = SORTS.find(([value]) => value === sort)[2];
  const all = found.vns
    .filter((vn) => !hidden.includes(vn.id) && matches(vn))
    .sort((a, b) => compare(a, b) || strength(b) - strength(a) || b.score - a.score);
  const shown = all.slice(0, shownCount);

  const view = currentView();
  const draw = { details: detailCard, list: listRow, covers: coverTile, cards: largeTile }[view];
  part("grid").dataset.view = view;
  part("grid").replaceChildren(...shown.map((vn, index) => draw(vn, index + 1)));
  part("more").hidden = all.length <= shown.length;

  part("count-row").hidden = !found.vns.length;
  part("count").textContent = t("{count} results", { count: all.length });
  part("restore").hidden = !hidden.length;
  part("restore").textContent = t("Show {count} hidden", { count: hidden.length });

  const filtered = themes.size > 0 || Object.values(filters).some(Boolean);
  part("reset").hidden = !filtered;

  if (!found.liked) {
    message(t("Rate or read a few VNs first: recommendations come from the ones you liked."));
  } else if (!all.length) {
    message(t("Nothing matches these filters."));
  } else {
    message("");
  }
}

function textBlock(tag, attrs, text) {
  const node = el(tag, attrs);
  node.textContent = text;
  return node;
}

function metaText(vn, { withRating = true } = {}) {
  return [
    withRating && vn.rating > 0 && vn.rating.toFixed(1),
    vn.year,
    vn.developer,
    lengthText(vn.minutes),
  ]
    .filter(Boolean)
    .join("  ·  ");
}

function reasonText(vn) {
  if (vn.credits.length) {
    return t("Creators you like: {names}", { names: vn.credits.join(", ") });
  }
  if (vn.matched.length) {
    return t("Themes you like: {names}", { names: vn.matched.slice(0, 3).join(", ") });
  }
  return "";
}

const openVn = (vn) => call("open_url", { url: `https://vndb.org/${vn.id}` });

function iconButton(path, label, onclick) {
  const icon = svg("svg", { class: "reco-icon", viewBox: "0 0 24 24", "aria-hidden": "true" });
  icon.append(svg("path", { d: path }));
  return el("button", { class: "reco-action", title: label, "aria-label": label, onclick }, icon);
}

function actions(vn) {
  return el(
    "div",
    { class: "reco-actions" },
    iconButton(OPEN_ICON, "View on VNDB", (event) => {
      event.stopPropagation();
      openVn(vn);
    }),
    iconButton(HIDE_ICON, "Not interested", (event) => {
      event.stopPropagation();
      saveHidden([...hiddenIds(), vn.id]);
    }),
  );
}

const thumbnailOf = (url) => url.replace(/^(https:\/\/t\.vndb\.org\/cv)\//, "$1.t/");

function coverOf(vn) {
  const cover = el("div", { class: "cover reco-cover" });
  const full = { url: vn.image_url, nsfw: vn.nsfw, label: vn.title };
  setCover(cover, { ...full, url: thumbnailOf(vn.image_url || "") });
  const img = cover.querySelector("img");
  if (img && img.src !== vn.image_url) {
    img.onerror = () => setCover(cover, full);
  }
  return cover;
}

function titleButton(vn) {
  const button = textBlock("button", { class: "reco-title" }, vn.title);
  button.onclick = () => openVn(vn);
  return button;
}

function detailCard(vn, place) {
  const description = vn.description ? vndbText(vn.description) : null;
  description?.classList.add("reco-description");
  return el(
    "article",
    { class: "card reco-card" },
    coverOf(vn),
    el(
      "div",
      { class: "reco-info" },
      el(
        "div",
        { class: "reco-head" },
        textBlock("span", { class: "reco-place" }, String(place)),
        actions(vn),
      ),
      titleButton(vn),
      textBlock("div", { class: "muted small-text" }, metaText(vn)),
      description,
      vn.because && textBlock("div", { class: "small-text" }, t("Because you liked {name}", { name: vn.because })),
      textBlock("div", { class: "muted small-text reco-reason" }, reasonText(vn)),
    ),
  );
}

function listRow(vn, place) {
  return el(
    "article",
    { class: "reco-row" },
    textBlock("span", { class: "reco-place" }, String(place)),
    coverOf(vn),
    el(
      "div",
      { class: "reco-info" },
      titleButton(vn),
      textBlock("div", { class: "muted small-text reco-reason" }, reasonText(vn)),
    ),
    textBlock("span", { class: "muted small-text reco-meta" }, metaText(vn, { withRating: false })),
    textBlock("b", { class: "reco-score" }, vn.rating > 0 ? vn.rating.toFixed(1) : ""),
    actions(vn),
  );
}

function tile(vn, place, below) {
  const art = el(
    "div",
    { class: "reco-art" },
    coverOf(vn),
    textBlock("span", { class: "reco-badge reco-badge-place" }, String(place)),
    vn.rating > 0 && textBlock("span", { class: "reco-badge reco-badge-rating" }, vn.rating.toFixed(1)),
    actions(vn),
  );
  art.onclick = () => openVn(vn);
  return el("article", { class: "reco-tile" }, art, ...below);
}

function coverTile(vn, place) {
  return tile(vn, place, [titleButton(vn)]);
}

function largeTile(vn, place) {
  return tile(vn, place, [
    titleButton(vn),
    textBlock("div", { class: "muted small-text" }, metaText(vn, { withRating: false })),
    textBlock("div", { class: "muted small-text reco-reason" }, reasonText(vn)),
  ]);
}

register({ id: "foryou", build, show: () => load() });
