import { el, svg } from "../dom.js";
import { addDays, exactDuration, isoDay, midnight, parseDay, shortDate, shortMonth, weekday } from "../format.js";
import { t } from "../i18n.js";
import { store } from "../store.js";
import { call } from "../ui.js";

const WEEKS = 53;
const CELL = 11;
const GAP = 3;
const PAD_LEFT = 26;
const PAD_TOP = 16;
const LEVELS = [30, 60, 120].map((minutes) => minutes * 60);
const MAX_NAMES = 3;

function level(seconds) {
  if (!seconds) {
    return 0;
  }
  return 1 + LEVELS.filter((limit) => seconds >= limit).length;
}

function someNames(names) {
  const more = names.length > MAX_NAMES ? ` +${names.length - MAX_NAMES}` : "";
  return names.slice(0, MAX_NAMES).join(", ") + more;
}

function tipText(day, info, mark, today) {
  const date = `${weekday(day)} ${shortDate(day, today)}`;
  const lines = [info ? `${date}  ·  ${exactDuration(info.seconds)}  ·  ${someNames(info.names)}` : `${date}  ·  ${t("nothing read")}`];
  if (mark?.finished.length) {
    lines.push(`⚑ ${t("Finished: {names}", { names: someNames(mark.finished) })}`);
  }
  if (mark?.started.length) {
    lines.push(`• ${t("Started: {names}", { names: someNames(mark.started) })}`);
  }
  return lines.join("\n");
}

function marksOf(x, y, mark) {
  const out = [];
  if (mark.started.length) {
    const under = mark.finished.length ? " under-flag" : "";
    out.push(svg("circle", { class: `heatmap-start${under}`, cx: x + CELL / 2, cy: y + CELL / 2, r: 1.8 }));
  }
  if (mark.finished.length) {
    const left = x + 3.5;
    out.push(
      svg("path", {
        class: mark.finished.length > 1 ? "heatmap-flag many" : "heatmap-flag",
        d: `M${left} ${y + 9.5}V${y + 1.5}L${left + 5.5} ${y + 3.75}L${left} ${y + 6}`,
      }),
    );
  }
  return out;
}

const KINDS = [
  ["heatmap_starts", "hide-starts", "• ", "started a VN"],
  ["heatmap_flags", "hide-flags", "⚑ ", "finished a VN"],
];

function legend(box) {
  const choices = KINDS.map(([setting, hidden, symbol, label]) => {
    const shown = store.settings[setting] !== false;
    box.classList.toggle(hidden, !shown);
    const input = el("input", { type: "checkbox" });
    input.checked = shown;
    input.onchange = () => {
      box.classList.toggle(hidden, !input.checked);
      store.settings[setting] = input.checked;
      call("update_settings", { changes: { [setting]: input.checked } });
    };
    return el("label", {}, input, symbol + t(label));
  });
  return el("div", { class: "heatmap-legend muted small-text" }, ...choices, el("span", { class: "labs-tag" }, "beta"));
}

export function drawHeatmap(container, days, marks = {}) {
  const today = midnight(new Date());
  const thisMonday = addDays(today, -((today.getDay() + 6) % 7));
  const firstMonday = addDays(thisMonday, -7 * (WEEKS - 1));
  const width = PAD_LEFT + WEEKS * (CELL + GAP);
  const height = PAD_TOP + 7 * (CELL + GAP);

  const map = svg("svg", { viewBox: `0 0 ${width} ${height}` });
  const tip = el("div", { class: "chart-tip", hidden: true });

  ["Mon", "Wed", "Fri"].forEach((label, i) => {
    const y = PAD_TOP + i * 2 * (CELL + GAP) + CELL - 2;
    map.append(svg("text", { x: 0, y }, t(label)));
  });

  for (let week = 0; week < WEEKS; week++) {
    const monday = addDays(firstMonday, week * 7);
    const sunday = addDays(monday, 6);
    const x = PAD_LEFT + week * (CELL + GAP);

    if (sunday.getDate() <= 7 && week < WEEKS - 1) {
      map.append(svg("text", { x, y: 10 }, shortMonth(sunday)));
    }

    for (let row = 0; row < 7; row++) {
      const day = addDays(monday, row);
      if (day > today) {
        break;
      }
      const info = days[isoDay(day)];
      const mark = marks[isoDay(day)];
      const y = PAD_TOP + row * (CELL + GAP);
      const cell = svg("rect", {
        x,
        y,
        width: CELL,
        height: CELL,
        rx: 2,
        class: `level-${level(info?.seconds)}`,
      });
      cell.addEventListener("mouseenter", () => {
        const left = ((x + CELL / 2) / width) * 100;
        const shift = left < 20 ? "0%" : left > 80 ? "-100%" : "-50%";
        tip.textContent = tipText(day, info, mark, today);
        tip.style.left = `${left}%`;
        tip.style.top = `${(y / height) * 100}%`;
        tip.style.transform = `translate(${shift}, calc(-100% - 6px))`;
        tip.hidden = false;
      });
      cell.addEventListener("mouseleave", () => (tip.hidden = true));
      map.append(cell);
      if (mark) {
        map.append(...marksOf(x, y, mark));
      }
    }
  }

  const shown = Object.keys(marks).some((day) => parseDay(day) >= firstMonday);
  const box = el("div", { class: "heatmap" }, map, tip);
  container.replaceChildren(box, shown ? legend(box) : "");
}
