import { $, el, html } from "../dom.js";
import {
  addDays,
  capitalize,
  formatPlaytime,
  isoDay,
  lastPlayed,
  midnight,
  parseDay,
  plural,
} from "../format.js";
import { register, show } from "../router.js";
import { store, subscribe } from "../store.js";
import { call, confirmButton, segmented, setCover, toast } from "../ui.js";
import { playGame } from "../components/play.js";
import { drawHeatmap } from "../components/heatmap.js";
import { t } from "../i18n.js";

const TEMPLATE = `
  <div class="page-head lib-head">
    <h2>Library</h2>
    <div class="row">
      <div class="view-menu" data-id="view-menu">
        <button class="btn secondary" data-id="view-button" aria-haspopup="menu">
          <span data-id="view-label"></span> ▾
        </button>
        <div class="menu" data-id="view-list" role="menu" hidden>
          <button data-v="list" role="menuitemradio">List</button>
          <button data-v="grid" role="menuitemradio">Grid</button>
          <button data-v="compact" role="menuitemradio">Compact</button>
          <button data-v="shelves" role="menuitemradio">Shelves</button>
          <button data-v="years" role="menuitemradio">Shelves by year</button>
        </div>
      </div>
      <input type="search" data-id="filter" placeholder="Filter…" />
    </div>
  </div>

  <div class="page-head lib-tools">
    <div class="seg sort-bar">
      <div class="sort-options" data-id="sort">
        <button data-v="most">Most read</button>
        <button data-v="recent">Recent</button>
        <button data-v="az">A–Z</button>
        <button data-v="release">Release date</button>
      </div>
      <span class="sort-sep"></span>
      <button class="sort-dir" data-id="reverse" title="Reverse order"></button>
    </div>
    <div class="row">
      <button class="btn secondary" data-id="wishlist">My VNDB lists</button>
      <select data-id="status">
        <option value="">All statuses</option>
        <option value="playing">Reading</option>
        <option value="finished">Finished</option>
        <option value="stalled">Stalled</option>
        <option value="dropped">Dropped</option>
      </select>
    </div>
  </div>

  <div class="muted" data-id="summary"></div>

  <div class="card" data-id="activity" hidden>
    <div class="card-title">
      <b>Reading days</b>
      <span class="muted small-text" data-id="streaks"></span>
    </div>
    <div data-id="heatmap"></div>
  </div>

  <div class="lib-list" data-id="list"></div>
`;

const SORTERS = {
  most: (a, b) => b.playtime - a.playtime,
  recent: (a, b) => b.last_played - a.last_played,
  az: (a, b) => a.name.localeCompare(b.name),
  release: (a, b) => b.released.localeCompare(a.released),
};

const yearOf = (game) => (/^\d{4}/.test(game.released) ? game.released.slice(0, 4) : "");

const SHELVES = [
  ["playing", "Reading"],
  ["finished", "Finished"],
  ["stalled", "Stalled"],
  ["dropped", "Dropped"],
  ["", "No status"],
];

const VIEWS = {
  list: (shown, most) => shown.map((game) => row(game, most)),
  grid: (shown) => shown.map(tile),
  compact: (shown) => [compactHead(), ...shown.map(line)],
  shelves: (shown) => shelves(shown),
  years: (shown) => yearShelves(shown),
};

let root;
let games = [];
let datesAsked = false;

const part = (id) => $(`[data-id="${id}"]`, root);

function build(container) {
  root = container;
  root.append(html(TEMPLATE));

  part("filter").oninput = render;
  part("status").onchange = render;
  part("wishlist").onclick = () => show("vndb-lists");
  part("reverse").onclick = () => {
    store.settings.library_sort_reverse = !store.settings.library_sort_reverse;
    render();
    call("update_settings", { changes: { library_sort_reverse: store.settings.library_sort_reverse } });
  };

  part("view-button").onclick = () => (part("view-list").hidden = !part("view-list").hidden);
  for (const item of part("view-list").children) {
    item.onclick = () => {
      part("view-list").hidden = true;
      call("update_settings", { changes: { library_view: item.dataset.v } });
    };
  }
  document.addEventListener("click", (event) => {
    if (!part("view-menu").contains(event.target)) {
      part("view-list").hidden = true;
    }
  });

  subscribe("library", () => root.classList.contains("active") && load());
  subscribe("settings", () => root.classList.contains("active") && load());

  let shownKey = playingKey();
  subscribe("snapshot", () => {
    if (playingKey() !== shownKey) {
      shownKey = playingKey();
      if (root.classList.contains("active") && games.length) {
        render();
      }
    }
  });
}

async function load() {
  games = await call("get_library");
  render();
  renderActivity();
}

function readingDays() {
  const days = {};
  for (const game of games) {
    for (const [day, seconds] of Object.entries(game.daily || {})) {
      if (seconds > 0) {
        days[day] ??= { seconds: 0, names: [] };
        days[day].seconds += seconds;
        days[day].names.push(game.name);
      }
    }
  }
  return days;
}

function milestones() {
  const marks = {};
  const add = (day, kind, name) => {
    marks[day] ??= { started: [], finished: [] };
    marks[day][kind].push(name);
  };
  for (const game of games) {
    const read = Object.entries(game.daily || {}).filter(([, seconds]) => seconds > 0).map(([day]) => day);
    if (read.length) {
      add(read.sort()[0], "started", game.name);
    }
    if (game.status === "finished" && game.finished_at > 0) {
      add(isoDay(new Date(game.finished_at * 1000)), "finished", game.name);
    }
  }
  return marks;
}

function streaks(days) {
  const today = midnight(new Date());
  let day = days[isoDay(today)] ? today : addDays(today, -1);
  let current = 0;
  while (days[isoDay(day)]) {
    current++;
    day = addDays(day, -1);
  }

  let longest = 0;
  let run = 0;
  let previous = "";
  for (const text of Object.keys(days).sort()) {
    run = previous && isoDay(addDays(parseDay(previous), 1)) === text ? run + 1 : 1;
    longest = Math.max(longest, run);
    previous = text;
  }
  return { current, longest };
}

function renderActivity() {
  const days = readingDays();
  const yearAgo = isoDay(addDays(new Date(), -365));
  const lastYear = Object.keys(days).filter((day) => day > yearAgo).length;

  part("activity").hidden = !lastYear;
  if (!lastYear) {
    return;
  }

  const { current, longest } = streaks(days);
  part("streaks").textContent = [
    t("{days} in the last year", { days: plural(lastYear, "day") }),
    current && t("streak: {days}", { days: plural(current, "day") }),
    t("best: {days}", { days: plural(longest, "day") }),
  ]
    .filter(Boolean)
    .join("  ·  ");
  drawHeatmap(part("heatmap"), days, milestones());
}

function weekSeconds(daily) {
  const today = midnight(new Date());
  const monday = addDays(today, -((today.getDay() + 6) % 7));
  let total = 0;
  for (let day = monday; day <= today; day = addDays(day, 1)) {
    total += daily[isoDay(day)] || 0;
  }
  return total;
}

function render() {
  const sort = SORTERS[store.settings.library_sort] ? store.settings.library_sort : "most";
  segmented(part("sort"), sort, (value) => {
    store.settings.library_sort = value;
    render();
    call("update_settings", { changes: { library_sort: value } });
  });
  const view = VIEWS[store.settings.library_view] ? store.settings.library_view : "list";
  for (const item of part("view-list").children) {
    item.classList.toggle("on", item.dataset.v === view);
    if (item.dataset.v === view) {
      part("view-label").textContent = item.textContent;
    }
  }

  const reverse = !!store.settings.library_sort_reverse;
  part("reverse").textContent = reverse ? "↑" : "↓";
  if (!datesAsked && (sort === "release" || view === "years") && games.some((game) => game.vndb_id && !game.released)) {
    datesAsked = true;
    call("fill_release_dates").catch(() => {});
  }

  const filter = part("filter").value.trim().toLowerCase();
  const status = part("status").value;
  const shown = games
    .filter((game) => !filter || game.name.toLowerCase().includes(filter))
    .filter((game) => !status || game.status === status)
    .sort(SORTERS[sort]);
  if (reverse) {
    shown.reverse();
  }
  if (sort === "release") {
    shown.sort((a, b) => !yearOf(a) - !yearOf(b));
  }

  const total = games.reduce((sum, game) => sum + game.playtime, 0);
  const week = games.reduce((sum, game) => sum + weekSeconds(game.daily), 0);
  part("summary").textContent = [
    plural(games.length, "game"),
    t("{time} read in total", { time: formatPlaytime(total) }),
    t("{time} this week", { time: formatPlaytime(week) }),
  ].join("  ·  ");

  if (!shown.length) {
    const message = games.length ? "Nothing matches." : "The VNs you read will show up here.";
    part("list").className = "lib-list";
    part("list").replaceChildren(el("div", { class: "card lib-empty" }, message));
    return;
  }

  const most = Math.max(1, ...games.map((game) => game.playtime));
  part("list").className = `lib-list view-${view}`;
  part("list").replaceChildren(...VIEWS[view](shown, most));
}

function opener(game) {
  const open = () => show("game", { key: game.key });
  return {
    role: "button",
    tabindex: "0",
    onclick: open,
    onkeydown: (event) => event.key === "Enter" && open(),
  };
}

function coverOf(game, size = "") {
  const cover = el("div", { class: `cover ${size}` });
  setCover(cover, { path: game.cover, nsfw: game.nsfw, label: game.name });
  return cover;
}

const playingKey = () => (store.snapshot?.detected ? store.snapshot.key : "");
const isPlaying = (game) => !!game.key && game.key === playingKey();

function nowBadge(game) {
  return isPlaying(game) && el("span", { class: "now-badge" }, t("Now reading"));
}

function readText(game) {
  return game.from_vndb && !game.playtime
    ? t("From your VNDB list")
    : t("{time} read", { time: formatPlaytime(game.playtime) });
}

function statusChip(game) {
  if (!game.status) {
    return null;
  }
  const label = SHELVES.find(([status]) => status === game.status)?.[1] || capitalize(game.status);
  return el("span", { class: `status ${game.status}` }, label);
}

function playButton(game, label = "▶  Play", kind = "") {
  const play = el("button", { class: `btn small ${kind}`, title: "Play" }, label);
  play.onclick = (event) => {
    event.stopPropagation();
    playGame(game.key);
  };
  play.hidden = !game.has_path;
  return play;
}

function row(game, most) {
  const meta = el(
    "div",
    { class: "meta" },
    nowBadge(game),
    statusChip(game),
    el("span", {}, readText(game)),
    game.last_played > 0 && el("span", {}, `·  ${lastPlayed(game.last_played)}`),
    game.vndb_id && el("span", {}, `·  ${game.vndb_id}`),
  );

  const bar = el(
    "div",
    { class: "bar" },
    el("i", { style: { width: `${(game.playtime / most) * 100}%` } }),
  );

  const remove = el("button", { class: "btn danger small" });
  confirmButton(remove, "Remove", async () => {
    await call("remove_game", { key: game.key });
    toast(t("{name} was removed (its screenshots are kept)", { name: game.name }));
  });

  return el(
    "div",
    { class: `card lib-row${isPlaying(game) ? " now-playing" : ""}`, ...opener(game) },
    coverOf(game, "small"),
    el("div", { class: "info" }, el("div", { class: "name" }, game.name), meta, bar),
    el("div", { class: "actions" }, playButton(game), remove),
  );
}

function tile(game) {
  return el(
    "div",
    { class: `lib-tile${isPlaying(game) ? " now-playing" : ""}`, ...opener(game) },
    el("div", { class: "tile-cover" }, coverOf(game), statusChip(game), nowBadge(game), playButton(game, "▶")),
    el("div", { class: "name" }, game.name),
    el("div", { class: "muted small-text" }, readText(game)),
  );
}

function compactHead() {
  return el(
    "div",
    { class: "lib-line head" },
    el("span"),
    el("span", {}, "Title"),
    el("span", {}, "Status"),
    el("span", {}, "Read"),
    el("span", {}, "Last played"),
    el("span"),
  );
}

function line(game) {
  return el(
    "div",
    { class: `lib-line${isPlaying(game) ? " now-playing" : ""}`, ...opener(game) },
    coverOf(game),
    el("div", { class: "name" }, game.name, nowBadge(game)),
    el("span", {}, statusChip(game) || ""),
    el(
      "span",
      { class: "muted" },
      game.from_vndb && !game.playtime ? "—" : formatPlaytime(game.playtime),
    ),
    el("span", { class: "muted" }, game.last_played > 0 ? lastPlayed(game.last_played) : "—"),
    el("span", {}, playButton(game, "▶", "secondary")),
  );
}

function shelves(shown) {
  return SHELVES.map(([status, title]) => {
    const group = shown.filter((game) => (game.status || "") === status);
    return group.length ? shelf(title, group) : null;
  }).filter(Boolean);
}

function shelf(title, group) {
  return el(
    "div",
    { class: "shelf" },
    el("div", { class: "shelf-title" }, el("b", {}, title), el("span", { class: "muted" }, String(group.length))),
    el("div", { class: "shelf-row" }, group.map(tile)),
  );
}

function yearShelves(shown) {
  const years = [...new Set(shown.map(yearOf).filter(Boolean))].sort().reverse();
  if (store.settings.library_sort_reverse) {
    years.reverse();
  }
  return [...years, ""]
    .map((year) => {
      const group = shown.filter((game) => yearOf(game) === year);
      return group.length ? shelf(year || t("Unknown year"), group) : null;
    })
    .filter(Boolean);
}

register({
  id: "library",
  build,
  show: load,
  onKey: (event) => event.key === "Escape" && (part("view-list").hidden = true),
});
