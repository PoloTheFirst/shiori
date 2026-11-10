import { $, el, html } from "../dom.js";
import { fileUrl, pickFile, pickSaveFile } from "../tauri.js";
import { register } from "../router.js";
import { store, subscribe } from "../store.js";
import { call, confirmButton, openDialog, toast, toggle } from "../ui.js";
import { t } from "../i18n.js";
import { hotkeyInput } from "../components/hotkey-input.js";
import { discordButton } from "../components/discord-button.js";
import {
  applyBalloon,
  BALLOON_DEFAULT,
  BALLOON_PRESETS,
  balloonStyle,
  playBalloon,
} from "../balloon-style.js";

const TEMPLATE = `
  <div class="page-head">
    <h2>Mascots</h2>
    <div class="row">
      <span data-id="enabled"></span>
      <button class="btn" data-id="add">Add a mascot…</button>
    </div>
  </div>
  <p class="muted">
    Click a mascot to put it on your desktop, click it again to take it off. Pick several to have
    them all at once. On the desktop, drag them anywhere, click to chat, double-click to open the
    app, right-click for more. A PNG with a transparent background looks best.
  </p>
  <div class="mascot-grid" data-id="grid"></div>

  <div class="card community mascot-community">
    <div>
      <b>Share your mascots</b>
      <div class="muted small-text">
        Press “Share…” on a mascot to save it as a file, then post it on the Discord. You'll find
        other people's mascots there too.
      </div>
    </div>
    <span data-id="discord-slot"></span>
  </div>

  <div class="card mascot-options">
    <div class="card-title"><b>Options</b></div>
    <div data-id="options"></div>
  </div>
`;

const CUSTOM = "custom";

const LINE_FIELDS = [
  ["start", "When you start a VN", "{name} becomes the VN's title."],
  ["stop", "When you stop reading"],
  ["idle", "When you go idle"],
  ["back", "When you come back"],
  ["section", "New chapter or route", "{section} becomes its name."],
  ["screenshot", "After a screenshot"],
];

const LIST_FIELDS = [
  ["pokes", "When you click it"],
  ["chatter", "Now and then"],
];

const VOICE_MOMENTS = [
  ...LINE_FIELDS.map(([key, label]) => [key, label]),
  ["poke", "When you click it"],
  ["chatter", "Now and then"],
];

const CHATTER_MINUTES = [0, 1, 3, 5, 10, 15, 30];

const PACK_EXTENSION = "shiorimascot";
const MASCOT_FILTERS = [
  { name: "Images or shared mascots", extensions: ["png", "webp", "gif", PACK_EXTENSION] },
];
const PACK_FILTERS = [{ name: "Shared mascot", extensions: [PACK_EXTENSION] }];
const AUDIO_FILTERS = [
  { name: "Audio", extensions: ["mp3", "wav", "ogg", "m4a", "opus", "flac"] },
];

let root;

const part = (id) => $(`[data-id="${id}"]`, root);

function build(container) {
  root = container;
  root.append(html(TEMPLATE));
  part("add").onclick = addMascot;
  part("discord-slot").append(discordButton({ label: "Share on Discord" }));
  subscribe("settings", () => root.classList.contains("active") && load());
}

const save = (changes) => call("update_settings", { changes });

function options() {
  const settings = store.settings;
  const backToCorner = el("button", { class: "btn secondary" }, "Put them back in the corner");
  backToCorner.onclick = () => save({ mascot_positions: {}, mascot_pos: null });

  const chatter = el(
    "select",
    {},
    ...CHATTER_MINUTES.map((minutes) =>
      el(
        "option",
        { value: minutes, selected: minutes === (settings.mascot_chatter_minutes || 0) },
        minutes ? t("Every {n} min", { n: minutes }) : t("Never"),
      ),
    ),
  );
  chatter.onchange = () => save({ mascot_chatter_minutes: Number(chatter.value) });

  const volume = el("input", {
    type: "range",
    min: 0,
    max: 100,
    step: 5,
    value: settings.mascot_voice_volume ?? 80,
  });
  const volumeLabel = el("span", { class: "muted", style: { width: "44px", textAlign: "right" } });
  const showVolume = () =>
    (volumeLabel.textContent = Number(volume.value) ? `${volume.value}%` : t("Off"));
  volume.oninput = showVolume;
  volume.onchange = () => save({ mascot_voice_volume: Number(volume.value) });
  showVolume();

  const clickKey = hotkeyInput(settings.mascot_click_hotkey, (value) =>
    save({ mascot_click_hotkey: value }),
  );

  return [
    toggle("Speech balloons", settings.mascot_talk, (on) => save({ mascot_talk: on })),
    el("div", { class: "field" }, el("span", {}, "Talk on their own"), chatter),
    el("div", { class: "hint-text" }, "After this long without saying anything."),
    el(
      "div",
      { class: "field" },
      el("span", {}, "Voice volume"),
      el("div", { class: "row" }, volume, volumeLabel),
    ),
    el("div", { class: "hint-text" }, "For the sounds you give a mascot with “Voices…”."),
    el("div", { class: "field" }, el("span", {}, "Key to switch clicks on or off"), clickKey),
    el(
      "div",
      { class: "hint-text" },
      "Works everywhere, even in a game: every mascot lets clicks go through, or becomes clickable " +
        "again. Handy if you play with the mascots on screen. Each mascot also has its own “View only”.",
    ),
    toggle(
      "Keep them above other windows",
      settings.mascot_topmost,
      (on) => save({ mascot_topmost: on }),
      "Off: they stay on the desktop, behind your windows.",
    ),
    el("div", { class: "row" }, backToCorner),
  ];
}

async function load() {
  part("enabled").replaceChildren(
    toggle("Show on desktop", store.settings.mascot_enabled, (on) => save({ mascot_enabled: on })),
  );
  part("options").replaceChildren(...options());
  const [items, personalities, aiReady] = await Promise.all([
    call("list_mascots"),
    call("mascot_personalities"),
    call("cutout_ready"),
  ]);
  part("grid").replaceChildren(...items.map((item) => card(item, personalities)));
  if (aiReady) {
    part("options").append(cutoutRow());
  }
}

function cutoutRow() {
  const remove = el("button", { class: "btn danger small" });
  confirmButton(remove, "Remove", async () => {
    await call("remove_cutout");
    load();
  });
  return el("div", { class: "field" }, el("span", {}, "AI model for cutouts (185 MB)"), remove);
}

function card(item, personalities) {
  const picture = el("img", {
    src: item.built_in ? "assets/mascot.png" : fileUrl(item.file),
    alt: "",
    draggable: "false",
  });

  let remove = null;
  if (!item.built_in) {
    remove = el("button", { class: "btn danger small" });
    confirmButton(remove, "Remove", () => call("remove_mascot", { id: item.id }));
  }

  const flip = () => call("show_mascot", { id: item.id, shown: !item.shown });

  const sizeText = (px) => t("Size: {px}px", { px });
  const sizeLabel = el("span", { class: "small-text" }, sizeText(item.height));
  const size = el("input", {
    type: "range",
    min: 120,
    max: 1200,
    step: 10,
    value: item.height,
    title: "Size",
  });
  size.onclick = (event) => event.stopPropagation();
  size.oninput = () => (sizeLabel.textContent = sizeText(size.value));
  size.onchange = () => call("set_mascot_size", { id: item.id, height: Number(size.value) });

  const personality = el(
    "select",
    { title: "Personality: changes what this mascot says" },
    ...[...personalities, { id: CUSTOM, name: t("Custom") }].map(({ id, name }) =>
      el("option", { value: id, selected: id === item.personality }, name),
    ),
  );
  const editLines = el("button", { class: "btn secondary small" }, "Write its lines…");
  editLines.hidden = item.personality !== CUSTOM;
  editLines.onclick = (event) => {
    event.stopPropagation();
    writeLines(item);
  };
  personality.onclick = (event) => event.stopPropagation();
  personality.onchange = () => {
    call("set_mascot_personality", { id: item.id, personality: personality.value });
    editLines.hidden = personality.value !== CUSTOM;
    if (personality.value === CUSTOM) {
      writeLines(item);
    }
  };

  const viewOnly = toggle(
    "View only",
    item.view_only,
    (on) => {
      item.view_only = on;
      call("set_mascot_view_only", { id: item.id, viewOnly: on });
    },
    "Clicks go through it.",
  );
  viewOnly.onclick = (event) => event.stopPropagation();

  const voices = el("button", { class: "btn secondary small" }, "Voices…");
  voices.onclick = (event) => {
    event.stopPropagation();
    editVoices(item);
  };

  const bubble = el("button", { class: "btn secondary small" }, "Bubble…");
  bubble.onclick = (event) => {
    event.stopPropagation();
    editBalloon(item);
  };

  const rename = el("button", { class: "mascot-rename", title: "Rename" }, "✎");
  rename.onclick = (event) => {
    event.stopPropagation();
    renameMascot(item);
  };

  const share = el("button", { class: "btn secondary small" }, "Share…");
  share.onclick = (event) => {
    event.stopPropagation();
    shareMascot(item);
  };

  return el(
    "div",
    {
      class: `mascot-card ${item.shown ? "on" : ""}`,
      role: "button",
      tabindex: "0",
      onclick: flip,
      onkeydown: (event) => {
        if (event.key === "Enter" && event.target === event.currentTarget) {
          flip();
        }
      },
    },
    el("div", { class: "mascot-pic" }, picture),
    el(
      "div",
      { class: "mascot-meta" },
      el("div", { class: "mascot-name" }, el("b", {}, item.name), rename),
      el("span", { class: "small-text" }, item.shown ? "On your desktop" : "Click to show"),
      el("div", { class: "mascot-size" }, sizeLabel, size),
      el(
        "div",
        { class: "mascot-size" },
        el("span", { class: "small-text" }, "Personality"),
        personality,
        editLines,
        voices,
        bubble,
      ),
      el("div", { class: "mascot-size" }, viewOnly),
      el("div", { class: "row" }, share, remove),
    ),
  );
}

async function writeLines(item) {
  const defaults = await call("personality_lines", { personality: "cheerful" });
  const saved = item.custom || {};

  const inputs = {};
  const fields = LINE_FIELDS.map(([key, label, hint]) => {
    inputs[key] = el("input", { type: "text", value: saved[key] || "", placeholder: defaults[key] });
    return el(
      "label",
      { class: "line-field" },
      el("span", {}, label),
      inputs[key],
      hint && el("span", { class: "small-text muted" }, hint),
    );
  });
  const lists = LIST_FIELDS.map(([key, label]) => {
    inputs[key] = el("textarea", { rows: 4, placeholder: (defaults[key] || []).join("\n") });
    inputs[key].value = (saved[key] || []).join("\n");
    return el(
      "label",
      { class: "line-field" },
      el("span", {}, label),
      inputs[key],
      el("span", { class: "small-text muted" }, "One per line, picked at random."),
    );
  });

  const answer = await openDialog({
    title: t("{name}'s lines", { name: t(item.name) }),
    wide: true,
    body: el(
      "div",
      { class: "line-fields" },
      el(
        "p",
        { class: "muted small-text" },
        "Leave a line empty to keep the one shown in grey.",
      ),
      ...fields,
      ...lists,
    ),
    actions: [
      { label: "Cancel", kind: "secondary", value: null },
      { label: "Save", value: "save" },
    ],
  });
  if (answer !== "save") {
    return;
  }

  const lines = {};
  for (const [key] of LINE_FIELDS) {
    lines[key] = inputs[key].value.trim();
  }
  for (const [key] of LIST_FIELDS) {
    lines[key] = inputs[key].value
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
  }
  await call("set_mascot_custom_lines", { id: item.id, lines });
  item.custom = lines;
}

const fileName = (path) => path.split(/[\\/]/).pop();

const mascotVolume = (id) => store.settings.mascot_voice_volumes?.[id] ?? 100;

let preview = null;
function playPreview(path, id) {
  preview?.pause();
  preview = new Audio(fileUrl(path));
  preview.volume = ((store.settings.mascot_voice_volume ?? 80) / 100) * (mascotVolume(id) / 100);
  preview.play().catch(() => {});
}

function voiceVolume(item) {
  const range = el("input", { type: "range", min: 0, max: 100, step: 5, value: mascotVolume(item.id) });
  const label = el("span", { class: "muted", style: { width: "44px", textAlign: "right" } });
  const show = () => (label.textContent = Number(range.value) ? `${range.value}%` : t("Off"));
  range.oninput = show;
  range.onchange = async () => {
    const volumes = { ...(store.settings.mascot_voice_volumes || {}), [item.id]: Number(range.value) };
    store.settings.mascot_voice_volumes = volumes;
    await call("update_settings", { changes: { mascot_voice_volumes: volumes } });
  };
  show();
  return el(
    "div",
    { class: "field" },
    el("span", {}, "This mascot's volume"),
    el("div", { class: "row" }, range, label),
  );
}

async function editVoices(item) {
  const body = el("div", { class: "line-fields" });
  const volume = voiceVolume(item);

  const render = async () => {
    const voices = await call("mascot_voices", { id: item.id });
    const rows = VOICE_MOMENTS.map(([moment, label]) => {
      const files = (voices[moment] || []).map((path) => {
        const play = el("button", { class: "btn secondary small", title: "Play" }, "▶");
        play.onclick = () => playPreview(path, item.id);
        const remove = el("button", { class: "btn danger small", title: "Remove" }, "✕");
        remove.onclick = async () => {
          await call("remove_mascot_voice", { path });
          render();
        };
        return el("div", { class: "voice-file" }, play, el("span", {}, fileName(path)), remove);
      });

      const add = el("button", { class: "btn secondary small" }, "Add a sound…");
      add.onclick = async () => {
        const path = await pickFile({ title: "Choose a sound", filters: AUDIO_FILTERS });
        if (path) {
          await call("add_mascot_voice", { id: item.id, event: moment, path });
          render();
        }
      };

      return el(
        "div",
        { class: "voice-moment" },
        el("div", { class: "row" }, el("b", {}, label), add),
        files.length
          ? el("div", { class: "voice-files" }, ...files)
          : el("span", { class: "small-text muted" }, "No sound: it only shows the text."),
      );
    });
    body.replaceChildren(
      el(
        "p",
        { class: "muted small-text" },
        "Sounds play with what the mascot says. For “When you click it” and “Now and then”, sounds go " +
          "with the lines in order: the 1st sound (by file name) with the 1st line, and so on. " +
          "Elsewhere, one is picked at random.",
      ),
      volume,
      ...rows,
    );
  };

  await render();
  await openDialog({
    title: t("{name}'s voice", { name: t(item.name) }),
    wide: true,
    body,
    actions: [{ label: "Done", value: true }],
  });
  preview?.pause();
}

const BALLOON_ANIM_NAMES = [
  ["pop", "Pop"],
  ["fade", "Fade"],
  ["bounce", "Bounce"],
  ["slide", "Slide"],
  ["type", "Letter by letter"],
];

const BALLOON_FONT_NAMES = [
  ["app", "App font"],
  ["rounded", "Rounded"],
  ["serif", "Serif"],
  ["mono", "Monospace"],
  ["hand", "Handwritten"],
];

function hexOf(color) {
  const ctx = document.createElement("canvas").getContext("2d");
  ctx.fillStyle = "#000000";
  ctx.fillStyle = color.trim() || "#000000";
  const value = ctx.fillStyle;
  if (value.startsWith("#")) {
    return value;
  }
  const [r, g, b] = value.match(/\d+/g).map(Number);
  return `#${[r, g, b].map((n) => n.toString(16).padStart(2, "0")).join("")}`;
}

function themeColor(name) {
  return hexOf(getComputedStyle(document.documentElement).getPropertyValue(name));
}

function sameStyle(a, b, keys = Object.keys(BALLOON_DEFAULT)) {
  return keys.every((key) => JSON.stringify(a[key] ?? null) === JSON.stringify(b[key] ?? null));
}

const LOOK_KEYS = Object.keys(BALLOON_DEFAULT).filter((key) => !["pos", "size", "anim"].includes(key));

function pictureBox(img, stage) {
  const box = img.getBoundingClientRect();
  const frame = stage.getBoundingClientRect();
  const ratio = img.naturalWidth && img.naturalHeight ? img.naturalWidth / img.naturalHeight : box.width / box.height;
  let width = box.width;
  let height = box.height;
  if (width / height > ratio) {
    width = height * ratio;
  } else {
    height = width / ratio;
  }
  return {
    left: box.left - frame.left + (box.width - width) / 2,
    top: box.top - frame.top + (box.height - height) / 2,
    width,
    height,
  };
}

function draggableBubble(bubble, picture, stage, getPos, setPos) {
  const place = () => {
    const pic = pictureBox(picture, stage);
    const w = bubble.offsetWidth;
    const h = bubble.offsetHeight;
    const pos = getPos();
    let x;
    let y;
    let toRight;
    if (pos) {
      x = pic.left + pos[0] * pic.width - w / 2;
      y = pic.top + pos[1] * pic.height - h / 2;
      toRight = pos[0] < 0.5;
    } else {
      x = pic.left - w + pic.width / 6;
      toRight = x >= 0;
      if (!toRight) {
        x = pic.left + pic.width - pic.width / 6;
      }
      y = pic.top + pic.height / 10;
    }
    x = Math.max(0, Math.min(x, stage.clientWidth - w));
    y = Math.max(0, Math.min(y, stage.clientHeight - h));
    bubble.style.left = `${x}px`;
    bubble.style.top = `${y}px`;
    bubble.classList.toggle("to-right", toRight);
  };

  bubble.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    bubble.setPointerCapture(event.pointerId);
    bubble.classList.add("dragging");
    const startX = event.clientX - bubble.offsetLeft;
    const startY = event.clientY - bubble.offsetTop;
    const move = (e) => {
      const pic = pictureBox(picture, stage);
      const x = Math.max(0, Math.min(e.clientX - startX, stage.clientWidth - bubble.offsetWidth));
      const y = Math.max(0, Math.min(e.clientY - startY, stage.clientHeight - bubble.offsetHeight));
      const cx = (x + bubble.offsetWidth / 2 - pic.left) / pic.width;
      const cy = (y + bubble.offsetHeight / 2 - pic.top) / pic.height;
      setPos([Math.round(cx * 1000) / 1000, Math.round(cy * 1000) / 1000]);
      place();
    };
    const up = () => {
      bubble.classList.remove("dragging");
      bubble.removeEventListener("pointermove", move);
      bubble.removeEventListener("pointerup", up);
      bubble.removeEventListener("pointercancel", up);
    };
    bubble.addEventListener("pointermove", move);
    bubble.addEventListener("pointerup", up);
    bubble.addEventListener("pointercancel", up);
  });

  picture.addEventListener("load", place);
  new ResizeObserver(place).observe(bubble);
  return place;
}

async function editBalloon(item) {
  let draft = balloonStyle(store.settings, item.id);

  const sampleText = t("Hi! This is how I'll talk to you while you read.");
  const words = el("span", { class: "balloon-text" }, sampleText);
  const bubble = el("div", { class: "balloon to-right" }, words);
  const replay = () => playBalloon(bubble, words, sampleText, draft);
  const picture = el("img", {
    src: item.built_in ? "assets/mascot.png" : fileUrl(item.file),
    alt: "",
    draggable: "false",
  });
  const stage = el("div", { class: "balloon-stage" }, picture, bubble);
  const controls = el("div", { class: "balloon-controls" });
  const autoPlace = el("button", { class: "btn secondary small" }, "Put it back automatically");
  const placeBubble = draggableBubble(
    bubble,
    picture,
    stage,
    () => draft.pos,
    (pos) => {
      draft.pos = pos;
      autoPlace.disabled = false;
    },
  );
  autoPlace.disabled = !draft.pos;
  autoPlace.onclick = () => {
    draft.pos = null;
    autoPlace.disabled = true;
    placeBubble();
  };

  const update = (changes) => {
    draft = { ...draft, ...changes };
    applyBalloon(bubble, draft);
    placeBubble();
  };

  const colorInput = (key, label, fallback) => {
    const input = el("input", { type: "color", value: draft[key] || themeColor(fallback) });
    input.oninput = () => update({ [key]: input.value });
    return el("label", {}, input, label);
  };

  const range = (key, label, min, max, unit = "px") => {
    const input = el("input", { type: "range", min, max, step: 1, value: draft[key] });
    const shown = el("span", { class: "muted" });
    const show = () => (shown.textContent = `${input.value}${unit}`);
    input.oninput = () => {
      show();
      update({ [key]: Number(input.value) });
    };
    show();
    return el("div", { class: "field" }, el("span", {}, label), el("div", { class: "row" }, input, shown));
  };

  const render = () => {
    applyBalloon(bubble, draft);
    requestAnimationFrame(placeBubble);
    const presets = el(
      "div",
      { class: "balloon-presets" },
      ...BALLOON_PRESETS.map(([name, preset]) => {
        const style = { ...BALLOON_DEFAULT, ...preset };
        const sample = el("span", { class: "balloon" }, el("span", { class: "balloon-text" }, "Aa"));
        applyBalloon(sample, { ...style, tail: false, shadow: false, size: 12 });
        const button = el(
          "button",
          { class: `balloon-preset ${sameStyle(style, draft, LOOK_KEYS) ? "on" : ""}`, title: name },
          sample,
          el("span", { class: "small-text" }, name),
        );
        button.onclick = () => {
          draft = { ...style, size: draft.size, pos: draft.pos, anim: draft.anim };
          render();
        };
        return button;
      }),
    );

    const font = el(
      "select",
      {},
      ...BALLOON_FONT_NAMES.map(([value, label]) =>
        el("option", { value, selected: value === draft.font }, t(label)),
      ),
    );
    font.onchange = () => update({ font: font.value });

    const anim = el(
      "select",
      {},
      ...BALLOON_ANIM_NAMES.map(([value, label]) =>
        el("option", { value, selected: value === draft.anim }, t(label)),
      ),
    );
    anim.onchange = () => {
      update({ anim: anim.value });
      replay();
    };
    const again = el("button", { class: "btn secondary small", title: "Replay" }, "▶");
    again.onclick = replay;

    controls.replaceChildren(
      el("div", { class: "field-label" }, "Style"),
      presets,
      el(
        "div",
        { class: "custom-colors" },
        colorInput("bg", "Background", "--surface"),
        colorInput("border", "Border", "--accent"),
        colorInput("text", "Text", "--text"),
      ),
      range("width", "Border width", 0, 6),
      range("radius", "Roundness", 0, 30),
      range("size", "Text size", 11, 20),
      el("div", { class: "field" }, el("span", {}, "Font"), font),
      el("div", { class: "field" }, el("span", {}, "Animation"), el("div", { class: "row" }, again, anim)),
      toggle("Tail", draft.tail, (on) => update({ tail: on }), "Points at the mascot."),
      toggle("Shadow", draft.shadow, (on) => update({ shadow: on })),
      toggle("Bold text", draft.bold, (on) => update({ bold: on })),
    );
  };
  render();

  const answer = await openDialog({
    title: t("{name}'s bubble", { name: t(item.name) }),
    width: 1000,
    body: el(
      "div",
      { class: "balloon-editor" },
      el(
        "div",
        { class: "balloon-left" },
        stage,
        el("div", { class: "hint-text" }, "Drag the bubble to choose where it shows up next to the mascot."),
        autoPlace,
      ),
      controls,
    ),
    actions: [
      { label: "Reset", kind: "secondary", value: "reset" },
      { label: "Cancel", kind: "secondary", value: null },
      { label: "Save", value: "save" },
    ],
  });
  if (!answer) {
    return;
  }
  const balloons = { ...(store.settings.mascot_balloons || {}) };
  if (answer === "reset" || sameStyle(draft, BALLOON_DEFAULT)) {
    delete balloons[item.id];
  } else {
    balloons[item.id] = draft;
  }
  store.settings.mascot_balloons = balloons;
  await call("update_settings", { changes: { mascot_balloons: balloons } });
}

async function renameMascot(item) {
  const input = el("input", { type: "text", maxlength: 60, style: { width: "100%", boxSizing: "border-box" } });
  input.value = t(item.name);
  const answer = await openDialog({
    title: "Rename this mascot",
    body: input,
    actions: [
      { label: "Cancel", kind: "secondary", value: null },
      { label: "Save", value: "save" },
    ],
    onOpen: (finish) => {
      input.select();
      input.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
          finish("save");
        }
      });
    },
  });
  const name = input.value.trim();
  if (answer === "save" && name && name !== t(item.name)) {
    await call("rename_mascot", { id: item.id, name });
  }
}

async function shareMascot(item) {
  const name = t(item.name);
  const path = await pickSaveFile({
    title: "Share this mascot",
    defaultPath: `${name}.${PACK_EXTENSION}`,
    filters: PACK_FILTERS,
  });
  if (path) {
    await call("export_mascot", { id: item.id, path });
    toast(t("Saved. Send this file so others can add {name}.", { name }));
  }
}

async function addMascot() {
  const path = await pickFile({ title: "Choose a mascot image", filters: MASCOT_FILTERS });
  if (path) {
    await call("add_mascot", { path });
  }
}

register({ id: "mascots", build, show: load });
