import { el, svg } from "../dom.js";
import { invoke, listen } from "../tauri.js";
import { call, openDialog, toast } from "../ui.js";
import { store } from "../store.js";
import { dayMonth } from "../format.js";
import { t } from "../i18n.js";
import { raw, vndbText } from "./util.js";

let basicCutout = false;

const ROLES = {
  main: "Protagonist",
  primary: "Main character",
  side: "Side character",
  appears: "Makes an appearance",
};
const ROLE_ORDER = Object.keys(ROLES);

let loading = 0;

function stats(char) {
  const out = [];
  if (char.age) out.push(t("{n} years old", { n: char.age }));
  if (char.height) out.push(`${char.height} cm`);
  if (char.weight) out.push(`${char.weight} kg`);
  if (char.bust && char.waist && char.hips) out.push(`B${char.bust} W${char.waist} H${char.hips}`);
  if (char.blood_type) out.push(t("Blood type {type}", { type: char.blood_type.toUpperCase() }));
  if (char.birthday) {
    const [month, day] = char.birthday;
    out.push(t("Birthday: {date}", { date: dayMonth(new Date(2000, month - 1, day)) }));
  }
  return out.join("  ·  ");
}

function characterCard(char, role) {
  const picture = el("div", { class: "labs-char-pic" });
  if (char.image?.url) {
    const img = el("img", { src: char.image.url, alt: "", loading: "lazy" });
    if (char.image.sexual >= 1 && !store.settings.allow_nsfw_covers) {
      picture.classList.add("nsfw");
    }
    picture.append(img);
  }

  const info = el(
    "div",
    { class: "labs-char-info" },
    el(
      "div",
      { class: "labs-char-name" },
      el("b", {}, raw(char.name)),
      char.original && el("span", { class: "muted" }, raw(char.original)),
      el("span", { class: `chip ${role === "main" || role === "primary" ? "accent" : "quiet"}` }, ROLES[role]),
    ),
    stats(char) && el("div", { class: "muted small-text" }, raw(stats(char))),
  );
  if (char.description) {
    const text = vndbText(char.description);
    text.classList.add("labs-char-desc");
    text.onclick = () => text.classList.toggle("open");
    info.append(text);
  }
  const icon = svg("svg", { viewBox: "0 0 24 24", "aria-hidden": "true" });
  icon.append(svg("path", { d: "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" }));
  const open = el("button", { class: "labs-char-link", title: "View on VNDB" }, icon);
  open.onclick = () => call("open_url", { url: `https://vndb.org/${char.id}` });
  return el("div", { class: "labs-char" }, picture, info, mascotButton(char), open);
}

function mascotButton(char) {
  const image = char.image;
  if (!image?.url || image.sexual >= 1 || image.violence >= 1) {
    return null;
  }
  const icon = svg("svg", { viewBox: "0 0 24 24", "aria-hidden": "true" });
  icon.append(svg("path", { d: "M22 11v1a10 10 0 1 1-9-10M8 14s1.5 2 4 2 4-2 4-2M9 9h.01M15 9h.01M16 5h6M19 2v6" }));
  const button = el("button", { class: "labs-char-link labs-char-mascot", title: "Add as mascot" }, icon);
  button.onclick = async () => {
    button.disabled = true;
    try {
      if (!(await prepareCutout())) {
        return;
      }
      const aiFailed = await call("add_character_mascot", { name: char.name, url: image.url });
      const done = aiFailed ? "{name} is now a mascot, with the basic cutout" : "{name} is now a mascot";
      toast(t(done, { name: char.name }));
    } finally {
      button.disabled = false;
    }
  };
  return button;
}

async function prepareCutout() {
  if (basicCutout || (await invoke("cutout_ready"))) {
    return true;
  }
  const answer = await openDialog({
    title: "Cleaner mascots with AI",
    body: el(
      "p",
      { class: "muted" },
      "An AI model cuts the character out much more cleanly. It runs on your PC and your pictures " +
        "stay there. It downloads once: about 250 MB, then 185 MB on your disk.",
    ),
    actions: [
      { label: "Not now", kind: "secondary", value: "basic" },
      { label: "Download", value: "download" },
    ],
  });
  if (answer === "basic") {
    basicCutout = true;
    return true;
  }
  return answer === "download" && downloadCutout();
}

async function downloadCutout() {
  const label = el("b", {}, "Downloading the AI model…");
  const fill = el("i", { style: { width: "0%" } });
  let closeDialog;
  const dialog = openDialog({
    title: "Cleaner mascots with AI",
    body: el("div", { class: "reco-loading" }, label, el("div", { class: "bar" }, fill)),
    actions: [{ label: "Cancel", kind: "secondary", value: "cancel" }],
    onOpen: (finish) => (closeDialog = finish),
  });
  const stopListening = await listen("cutout-progress", ({ payload }) => {
    label.textContent = t("Downloading the AI model… {done} / {total} MB", payload);
    fill.style.width = `${payload.percent}%`;
  });
  dialog.then((value) => value !== "done" && invoke("cancel_cutout_download"));
  try {
    const finished = await invoke("download_cutout");
    closeDialog("done");
    return finished;
  } catch (error) {
    closeDialog("done");
    basicCutout = true;
    await openDialog({
      title: "Cleaner mascots with AI",
      body: el("p", { class: "muted" }, String(error)),
    });
    return true;
  } finally {
    stopListening();
  }
}

async function renderCharacters(vnId, box) {
  box.replaceChildren(el("div", { class: "muted small-text" }, "Loading the characters…"));
  let chars;
  try {
    chars = await invoke("vn_characters", { vnId });
  } catch {
    box.replaceChildren(el("div", { class: "muted small-text" }, "Couldn't load the characters."));
    return;
  }
  const shown = chars
    .map((char) => ({ char, link: char.vns.find((vn) => vn.id === vnId) }))
    .filter(({ link }) => link && link.spoiler === 0 && ROLES[link.role])
    .sort((a, b) => ROLE_ORDER.indexOf(a.link.role) - ROLE_ORDER.indexOf(b.link.role));
  box.replaceChildren(
    ...(shown.length
      ? shown.map(({ char, link }) => characterCard(char, link.role))
      : [el("div", { class: "muted small-text" }, "No characters on VNDB.")]),
  );
}

document.addEventListener("game-loaded", (event) => {
  const page = document.getElementById("page-game");
  page?.querySelector(".labs-description-card")?.remove();
  const entry = event.detail.entry;
  const vnId = entry.vndb_id || entry.matched_vndb_id;
  if (!page || !vnId) {
    return;
  }

  const about = el("div", { class: "muted small-text" }, "Loading the description…");
  const people = el("div", { class: "labs-chars" });
  people.hidden = true;
  let peopleLoaded = false;

  const tabs = el(
    "div",
    { class: "seg" },
    el("button", { "data-v": "about", class: "on" }, "Description"),
    el("button", { "data-v": "people" }, "Characters"),
  );
  for (const button of tabs.children) {
    button.onclick = () => {
      for (const other of tabs.children) {
        other.classList.toggle("on", other === button);
      }
      const showPeople = button.dataset.v === "people";
      people.hidden = !showPeople;
      aboutBox.hidden = showPeople;
      if (showPeople && !peopleLoaded) {
        peopleLoaded = true;
        renderCharacters(vnId, people);
      }
    };
  }

  const aboutBox = el("div", {}, about);
  const card = el(
    "div",
    { class: "card labs-description-card" },
    el("div", { class: "card-title" }, tabs),
    aboutBox,
    people,
  );
  page.querySelector(".game-grid")?.after(card);

  const job = ++loading;
  invoke("vn_description", { vnId })
    .then((text) => {
      if (job === loading) {
        about.replaceWith(
          text ? vndbText(text) : el("div", { class: "muted small-text" }, "No description on VNDB."),
        );
      }
    })
    .catch(() => (about.textContent = t("Couldn't load the description.")));
});
