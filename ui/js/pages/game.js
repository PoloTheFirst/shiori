import { $, el, html } from "../dom.js";
import {
  addDays,
  formatPlaytime,
  isoDay,
  lastPlayed,
  lengthText,
  midnight,
  parseDay,
  readingProgress,
  shortDate,
  weekday,
} from "../format.js";
import { invoke, fileUrl } from "../tauri.js";
import { register, show } from "../router.js";
import { store, subscribe } from "../store.js";
import { ask, call, openDialog, segmented, setCover, toast } from "../ui.js";
import { drawChart } from "../components/chart.js";
import { LAUNCHERS, createShortcut, locateGame, locateTool, playGame } from "../components/play.js";
import { t } from "../i18n.js";

const PRIVACY_HELP = {
  full: "Discord shows the name, the current section and the cover.",
  partial: "Discord shows the name and cover, but not where you are in the story.",
  private: "Discord only shows a generic “Visual Novel” — no name, no cover.",
  off: "Nothing is shared on Discord while this game is open.",
};

const TEMPLATE = `
  <button class="back" data-id="back">← Library</button>

  <div class="banner info dupe" data-id="dupe" hidden>
    <i></i><span data-id="dupe-text"></span>
    <button class="btn small" data-id="dupe-merge">Merge them</button>
  </div>

  <div class="game-grid">
    <div class="card game-head">
      <div class="cover mid" data-id="cover"></div>
      <div class="details">
        <h2 data-id="name"></h2>
        <button class="path" data-id="path" title="Show in folder"></button>
        <div class="row" data-id="vndb"></div>
        <button class="link" data-id="change-cover">Change VN / cover…</button>
      </div>
    </div>

    <div class="col">
      <div class="stats">
        <div class="stat">
          <span>Total read</span><b data-id="total"></b>
          <button class="stat-edit" data-id="edit-total" title="Edit total read">✎</button>
        </div>
        <div class="stat"><span>This week</span><b data-id="week"></b></div>
        <div class="stat"><span>Sessions</span><b data-id="sessions"></b></div>
        <div class="stat"><span>Last played</span><b data-id="last"></b></div>
      </div>

      <div class="card" data-id="progress" hidden>
        <div class="card-title">
          <b>Progress</b>
          <span class="muted" data-id="progress-text"></span>
        </div>
        <div class="bar"><i data-id="progress-bar"></i></div>
        <div class="hint-text">
          Compared with the average length on VNDB. Your reading speed and routes change it.
        </div>
      </div>
    </div>

    <div class="col">
      <div class="card">
        <div class="field">
          <b>Status</b>
          <div class="seg" data-id="status">
            <button data-v="playing">Reading</button>
            <button data-v="finished">Finished</button>
            <button data-v="stalled">Stalled</button>
            <button data-v="dropped">Dropped</button>
          </div>
        </div>
        <div class="field">
          <b>Rating</b>
          <div class="row">
            <span class="muted small-text" data-id="vote-state"></span>
            <select data-id="vote"></select>
          </div>
        </div>
        <div class="hint-text" data-id="vndb-note"></div>
      </div>

      <div class="card">
        <div class="field">
          <b>Launch with</b>
          <div class="seg" data-id="launcher">
            <button data-v="">Normal</button>
            <button data-v="le">Locale Emulator</button>
            <button data-v="ntlea">NTLEA</button>
          </div>
        </div>
        <div class="hint-text" data-id="launcher-note"></div>
      </div>

      <div class="card">
        <div class="field">
          <b>Discord privacy</b>
          <div class="seg" data-id="privacy">
            <button data-v="full">Full</button>
            <button data-v="partial">Partial</button>
            <button data-v="private">Private</button>
            <button data-v="off">Off</button>
          </div>
        </div>
        <div class="hint-text" data-id="privacy-help"></div>
      </div>
    </div>

    <div class="card">
      <div class="card-title">
        <b>Time read</b>
        <div class="seg" data-id="range">
          <button data-v="days">30 days</button>
          <button data-v="weeks">12 weeks</button>
        </div>
      </div>
      <div data-id="chart"></div>
      <div class="hint-text" data-id="history-note"></div>
    </div>
  </div>

  <div class="card">
    <div class="card-title">
      <div class="row"><b>Screenshots</b><span class="muted" data-id="shot-count"></span></div>
      <button class="btn secondary small" data-id="all-shots">View all</button>
    </div>
    <div class="shot-strip" data-id="shots"></div>
    <div class="hint-text" data-id="shots-hint"></div>
  </div>

  <div class="game-footer">
    <button class="btn" data-id="play">▶  Play</button>
    <button
      class="btn secondary"
      data-id="locate"
      title="Pick the program Play starts, like the game's launcher. The app won't change it."
    >
      Locate…
    </button>
    <button
      class="btn secondary"
      data-id="shortcut"
      title="Starts the game with the options above (Locale Emulator, Steam…) and Shiori RPC"
    >
      Desktop shortcut
    </button>
    <button class="btn danger" data-id="remove"></button>
  </div>
`;

const VOTES = [0];
for (let vote = 100; vote >= 10; vote -= 5) {
  VOTES.push(vote);
}

let root;
let key = "";
let game = null;
let range = "days";

const part = (id) => $(`[data-id="${id}"]`, root);

function build(container) {
  root = container;
  root.append(html(TEMPLATE));

  part("back").onclick = () => show("library");
  part("change-cover").onclick = () => show("cover", { key, back: "game" });
  part("all-shots").onclick = () => show("screenshots", { key, back: "game" });
  part("play").onclick = () => playGame(key);
  part("shortcut").onclick = () => createShortcut(key, game.name);
  part("locate").onclick = async () => (await locateGame(key)) && load();
  part("vote").onchange = saveVote;

  part("edit-total").onclick = editTotal;

  subscribe("library", (change) => isShown() && (!change?.key || change.key === key) && load());
  subscribe("screenshots", (change) => isShown() && change?.key === key && load());
}

function isShown() {
  return root.classList.contains("active");
}

async function showPage(params) {
  if (params.key !== key) {
    range = "days";
  }
  key = params.key;
  await load();
}

async function load() {
  game = await call("get_game", { key });
  const entry = game.entry;

  part("name").textContent = game.name;
  const exe = entry.launch_path || entry.path;
  part("path").textContent = exe || t("Executable not located yet");
  part("path").disabled = !exe;
  part("path").onclick = () => call("show_in_folder", { path: exe });
  setCover(part("cover"), { path: game.cover, nsfw: entry.cover_nsfw, label: game.name });

  renderVndb(entry);
  renderStatus(entry);
  renderPrivacy(entry.privacy || "full");
  renderVote(entry);
  renderLauncher(entry);
  renderDuplicates();
  renderStats(entry);
  renderProgress(entry);
  renderChart();
  renderShots();
  document.dispatchEvent(new CustomEvent("game-loaded", { detail: game }));

  part("play").hidden = !exe;
  part("shortcut").hidden = !exe;
  part("remove").textContent = t("Delete…");
  part("remove").onclick = () => deleteGame(key, game.name);
}

async function deleteGame(key, name) {
  const sure = await openDialog({
    title: t("Delete {name}?", { name }),
    body: el(
      "div",
      { class: "delete-list" },
      el("p", { class: "muted" }, "This removes from Shiori:"),
      el(
        "ul",
        {},
        el("li", {}, "its time read, history and status"),
        el("li", {}, "its covers and VNDB info saved on this PC"),
      ),
      el("p", { class: "hint-text" }, "Screenshots are kept. If you start it again, it comes back as a new VN."),
    ),
    actions: [
      { label: "Cancel", kind: "secondary", value: false },
      { label: "Delete", kind: "danger", value: true },
    ],
  });
  if (!sure) {
    return;
  }
  await call("delete_game", { key });
  toast(t("{name} was deleted (its screenshots are kept)", { name }));
  show("library");
}

function renderVndb(entry) {
  const box = part("vndb");
  const vnId = entry.vndb_id;
  const matched = entry.matched_vndb_id;
  const link = (id) =>
    el(
      "button",
      {
        class: "btn secondary small",
        onclick: () => call("open_url", { url: `https://vndb.org/${id}` }),
      },
      `VNDB ${id}`,
    );

  if (vnId) {
    box.replaceChildren(link(vnId));
  } else if (matched) {
    const confirm = el("button", { class: "btn small", onclick: confirmMatch }, "Confirm");
    box.replaceChildren(
      link(matched),
      el("span", { class: "muted small-text" }, "found automatically"),
      confirm,
    );
  } else {
    box.replaceChildren(
      el(
        "span",
        { class: "muted small-text" },
        "Not matched on VNDB — pick it with “Change VN / cover…”.",
      ),
    );
  }
}

async function confirmMatch() {
  await call("confirm_match", { key });
  toast("VNDB entry confirmed");
}

function renderStatus(entry) {
  segmented(part("status"), entry.status || "", async (value) => {
    const status = value === entry.status ? "" : value;
    await call("update_game", { key, fields: { status } });
  });
}

function renderPrivacy(value) {
  part("privacy-help").textContent = t(PRIVACY_HELP[value] || "");
  segmented(part("privacy"), value, async (picked) => {
    await call("update_game", { key, fields: { privacy: picked } });
    renderPrivacy(picked);
  });
}

function voteLabel(vote) {
  return vote ? String(vote / 10) : "No rating";
}

function renderVote(entry) {
  const canRate = game.has_token && !!entry.vndb_id;
  const select = part("vote");
  const current = entry.vndb_vote || 0;

  const choices = VOTES.includes(current)
    ? VOTES
    : [...VOTES, current].sort((a, b) => (a && b ? b - a : a - b));
  select.replaceChildren(
    ...choices.map((v) => el("option", { value: v, selected: v === current }, voteLabel(v))),
  );
  select.disabled = !canRate;

  let note;
  if (!game.has_token) {
    note = "Add your VNDB token in Settings to rate it and sync its status to your VNDB list.";
  } else if (!entry.vndb_id) {
    note = "Confirm the VNDB match above to rate it and sync it to your VNDB list.";
  } else if (game.vndb_sync) {
    note = "Status and rating are sent to your VNDB list.";
  } else {
    note =
      "The rating is sent to your VNDB list. Turn on list sync in Settings for the status too.";
  }
  part("vndb-note").textContent = t(note);
  part("vote-state").textContent = "";

  if (canRate) {
    refreshVote();
  }
}

async function refreshVote() {
  const forKey = key;
  part("vote-state").textContent = t("Checking VNDB…");
  try {
    const vote = await invoke("get_vote", { key: forKey });
    if (forKey === key && (vote || 0) !== (game.entry.vndb_vote || 0)) {
      game.entry.vndb_vote = vote || 0;
      renderVote(game.entry);
    }
    part("vote-state").textContent = "";
  } catch (error) {
    part("vote-state").textContent = String(error);
  }
}

async function saveVote() {
  const vote = Number(part("vote").value) || null;
  part("vote").disabled = true;
  part("vote-state").textContent = t("Saving…");
  try {
    await invoke("set_vote", { key, vote });
    game.entry.vndb_vote = vote || 0;
    part("vote-state").textContent = t("Saved on VNDB ✓");
  } catch (error) {
    part("vote-state").textContent = String(error);
    renderVote(game.entry);
  }
  part("vote").disabled = false;
}

function renderLauncher(entry) {
  const launcher = entry.launcher in LAUNCHERS ? entry.launcher : "";

  segmented(part("launcher"), launcher, async (value) => {
    if (value && !toolIsSet(value) && !(await locateTool(value))) {
      return;
    }
    await call("update_game", { key, fields: { launcher: value } });
  });

  if (!launcher) {
    part("launcher-note").textContent = t(
      "For Japanese VNs with garbled text or that won't start: Play can go through Locale Emulator or NTLEA.",
    );
  } else if (game.tool) {
    part("launcher-note").textContent = t("Play starts it through {tool}.", { tool: game.tool });
  } else {
    part("launcher-note").textContent = t("{exe} can't be found anymore — Play will ask where it is.", {
      exe: LAUNCHERS[launcher].exe,
    });
  }
}

function toolIsSet(launcher) {
  const path = launcher === "le" ? store.settings.locale_emulator_path : store.settings.ntlea_path;
  return !!path;
}

function daySeconds(day) {
  return (game.entry.daily || {})[isoDay(day)] || 0;
}

function renderStats(entry) {
  const today = midnight(new Date());
  const monday = addDays(today, -((today.getDay() + 6) % 7));
  let week = 0;
  for (let day = monday; day <= today; day = addDays(day, 1)) {
    week += daySeconds(day);
  }

  part("total").textContent = formatPlaytime(entry.playtime_seconds || 0);
  part("week").textContent = formatPlaytime(week);
  part("sessions").textContent = entry.sessions || "—";
  part("last").textContent = lastPlayed(entry.last_played) || "—";
}

function renderDuplicates() {
  const other = game.duplicates[0];
  part("dupe").hidden = !other;
  if (!other) {
    return;
  }
  part("dupe-text").textContent = t("Looks like the same game as “{name}” ({time} read).", {
    name: other.name,
    time: formatPlaytime(other.playtime),
  });
  part("dupe-merge").onclick = () => mergeWith(other);
}

async function mergeWith(other) {
  const keepThis = (game.entry.playtime_seconds || 0) >= other.playtime;
  const [from, into, name] = keepThis
    ? [other.key, key, game.name]
    : [key, other.key, other.name];

  const sure = await ask(
    "Merge these entries?",
    t("Time read, history, sessions and screenshots are combined into “{name}”. This can't be undone.", {
      name,
    }),
    { yes: "Merge" },
  );
  if (!sure) {
    return;
  }
  await call("merge_games", { from, into });
  toast(t("Merged into {name}", { name }));
  show("game", { key: into });
}

async function renderProgress(entry) {
  const shownKey = key;
  const vnId = entry.vndb_id || entry.matched_vndb_id;
  const minutes = vnId ? await invoke("vn_length", { vnId }).catch(() => 0) : 0;
  if (shownKey !== key) {
    return;
  }

  const percent = readingProgress(entry.playtime_seconds, minutes);
  part("progress").hidden = percent === null;
  if (percent === null) {
    return;
  }
  part("progress-bar").style.width = `${Math.min(percent, 100)}%`;
  part("progress-text").textContent =
    percent < 100
      ? t("{percent}% · average {length}", { percent, length: lengthText(minutes) })
      : t("Past the average ({length})", { length: lengthText(minutes) });
}

function dailyPoints(today) {
  const points = [];
  for (let back = 29; back >= 0; back--) {
    const day = addDays(today, -back);
    points.push({ day, seconds: daySeconds(day), tip: `${weekday(day)} ${shortDate(day, today)}` });
  }
  return points;
}

function weeklyPoints(today) {
  const thisMonday = addDays(today, -((today.getDay() + 6) % 7));
  const points = [];
  for (let back = 11; back >= 0; back--) {
    const monday = addDays(thisMonday, -7 * back);
    let seconds = 0;
    for (let i = 0; i < 7; i++) {
      const day = addDays(monday, i);
      if (day <= today) {
        seconds += daySeconds(day);
      }
    }
    points.push({ day: monday, seconds, tip: t("Week of {date}", { date: shortDate(monday, today) }) });
  }
  return points;
}

function renderChart() {
  segmented(part("range"), range, (value) => {
    range = value;
    renderChart();
  });

  const today = midnight(new Date());
  const points = range === "weeks" ? weeklyPoints(today) : dailyPoints(today);

  const every = range === "weeks" ? 3 : 7;
  for (const [i, point] of points.entries()) {
    point.axis = (points.length - 1 - i) % every === 0 ? shortDate(point.day, today) : "";
  }
  drawChart(part("chart"), points);

  const days = Object.keys(game.entry.daily || {}).sort();
  part("history-note").textContent = days.length
    ? t("History since {date}. Time read before that only counts in the total.", {
        date: shortDate(parseDay(days[0]), today),
      })
    : t("History fills in as you read — time read before it was tracked only counts in the total.");
}

async function editTotal() {
  const seconds = game.entry.playtime_seconds || 0;
  const hours = el("input", { type: "number", min: 0, value: Math.floor(seconds / 3600) });
  const minutes = el("input", {
    type: "number",
    min: 0,
    max: 59,
    value: Math.floor((seconds % 3600) / 60),
  });

  const answer = await openDialog({
    title: "Total read",
    body: el(
      "div",
      { style: { display: "grid", gap: "12px" } },
      el("div", { class: "row" }, hours, "h", minutes, "min"),
      el(
        "p",
        { class: "muted small-text" },
        "Time read before the app tracked it, or a correction. The day-by-day history " +
          "(this week, the chart) stays as it is.",
      ),
    ),
    actions: [
      { label: "Cancel", kind: "secondary", value: null },
      { label: "Save", value: "save" },
    ],
    onOpen: (finish) => {
      hours.select();
      for (const input of [hours, minutes]) {
        input.onkeydown = (event) => {
          if (event.key === "Enter") {
            finish("save");
          }
        };
      }
    },
  });
  if (!answer) {
    return;
  }

  const h = Number(hours.value || 0);
  const m = Number(minutes.value || 0);
  if (!Number.isInteger(h) || !Number.isInteger(m) || h < 0 || m < 0) {
    toast("Enter whole numbers of hours and minutes.", { error: true });
    return;
  }
  await call("set_playtime", { key, seconds: h * 3600 + m * 60 });
  setTimeout(load, 400);
}

async function renderShots() {
  const strip = part("shots");
  part("shot-count").textContent = game.shot_count ? String(game.shot_count) : "";
  part("all-shots").hidden = !game.shot_count;

  const hotkey = game.hotkey;
  part("shots-hint").hidden = game.shot_count > 0;
  part("shots-hint").textContent = hotkey
    ? t("Press {key} while reading to capture the game's window.", { key: hotkey })
    : t("Pick a screenshot key in Settings to capture the game's window.");

  strip.replaceChildren();
  for (const path of game.shots) {
    const img = el("img", { alt: "" });
    strip.append(
      el(
        "button",
        { onclick: () => show("screenshots", { key, select: path, back: "game" }) },
        img,
      ),
    );
    invoke("thumbnail", { path, width: 320, height: 180 }).then(
      (thumb) => (img.src = fileUrl(thumb)),
    );
  }
}

register({
  id: "game",
  nav: "library",
  build,
  show: showPage,
  onKey: (event) => event.key === "Escape" && show("library"),
});
