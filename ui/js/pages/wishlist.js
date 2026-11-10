import { $, el, html } from "../dom.js";
import { lengthText } from "../format.js";
import { register, show } from "../router.js";
import { store } from "../store.js";
import { call, setCover } from "../ui.js";
import { t } from "../i18n.js";

const STALE_AFTER_MS = 10 * 60 * 1000;

const TEMPLATE = `
  <button class="back" data-id="back">← Library</button>

  <div class="card wish">
    <div class="cover" data-id="cover"></div>
    <div class="info">
      <div class="eyebrow" data-id="eyebrow">Random pick from your VNDB wishlist</div>
      <h1 data-id="title"></h1>
      <div class="muted" data-id="alt"></div>
      <div class="chips" data-id="chips"></div>
      <div class="subtle small-text" data-id="status"></div>

      <div class="row actions">
        <button class="btn" data-id="next">Another one</button>
        <button class="btn secondary" data-id="vndb">VNDB page</button>
        <span class="subtle small-text">Space: another one</span>
      </div>
    </div>
  </div>
`;

let root;
let order = [];
let position = -1;
let total = 0;
let loadedAt = 0;
let loading = false;
let label = 5;
let labelName = "";
let back = "library";

const part = (id) => $(`[data-id="${id}"]`, root);

function build(container) {
  root = container;
  root.append(html(TEMPLATE));

  part("back").onclick = () => show(back);
  part("next").onclick = next;
  part("vndb").onclick = () =>
    current() && call("open_url", { url: `https://vndb.org/${current().id}` });
  setReady(false);
}

function setReady(ready) {
  part("next").disabled = !ready;
  part("vndb").disabled = !ready;
}

function current() {
  return order[position] || null;
}

function shuffled(wishlist, inLibrary) {
  const skip = new Set(inLibrary);
  const pool = wishlist.filter((vn) => !skip.has(vn.id));
  const list = pool.length ? pool : [...wishlist];

  for (let i = list.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [list[i], list[j]] = [list[j], list[i]];
  }
  return list;
}

async function load(params = {}) {
  const wanted = params.label ?? 5;
  back = params.from || "library";
  part("back").textContent = back === "vndb-lists" ? t("← My VNDB lists") : t("← Library");
  if (wanted !== label) {
    label = wanted;
    order = [];
    position = -1;
  }
  labelName = params.name || labelName;
  part("eyebrow").textContent =
    label === 5
      ? t("Random pick from your VNDB wishlist")
      : t("Random pick from your VNDB list “{name}”", { name: label < 10 ? t(labelName) : labelName });
  if (loading || (order.length && Date.now() - loadedAt < STALE_AFTER_MS)) {
    return;
  }
  loading = true;
  if (!order.length) {
    part("title").textContent = "";
    part("status").textContent = t("Loading your wishlist…");
  }

  try {
    const { vns: wishlist, in_library } = await call("get_vndb_list", { label });
    loadedAt = Date.now();

    if (!wishlist.length) {
      order = [];
      setReady(false);
      part("title").textContent = t("Your wishlist is empty");
      part("status").textContent = t("Add VNs to your Wishlist on vndb.org, then come back.");
      return;
    }

    const shown = current();
    order = shuffled(wishlist, in_library);
    total = wishlist.length;
    setReady(true);

    const keep = shown ? order.findIndex((vn) => vn.id === shown.id) : -1;
    if (keep >= 0) {
      position = keep;
      render();
    } else {
      position = -1;
      next();
    }
  } catch (error) {
    if (!order.length) {
      part("title").textContent = t("Couldn't load your wishlist");
      part("status").textContent = String(error);
    }
  } finally {
    loading = false;
  }
}

function next() {
  if (!order.length) {
    return;
  }
  position += 1;

  if (position >= order.length) {
    const last = order[order.length - 1];
    order = shuffled(order, []);
    if (order.length > 1 && order[0] === last) {
      [order[0], order[1]] = [order[1], order[0]];
    }
    position = 0;
  }
  render();
}

function render() {
  const vn = current();
  part("title").textContent = vn.title;
  part("alt").textContent = vn.alt_title !== vn.title ? vn.alt_title : "";

  const rating = vn.rating > 10 ? vn.rating / 10 : vn.rating;
  const chips = [
    vn.year,
    vn.rating > 0 && t("Rating {rating}", { rating: rating.toFixed(1) }),
    lengthText(vn.length_minutes),
  ];
  part("chips").replaceChildren(
    ...chips.filter(Boolean).map((text) => el("span", { class: "chip quiet" }, text)),
  );

  const skipped = total - order.length;
  part("status").textContent =
    t(label === 5 ? "{position} / {count} on your wishlist" : "{position} / {count} in this list", {
      position: position + 1,
      count: order.length,
    }) +
    (skipped ? "  ·  " + t("{count} already in your Library skipped", { count: skipped }) : "");

  setCover(part("cover"), {
    url: vn.image_url,
    nsfw: vn.nsfw && !store.settings.allow_nsfw_covers,
    label: vn.title,
  });
}

register({
  id: "wishlist",
  nav: "library",
  build,
  show: load,
  onKey: (event) => {
    if (event.key === "Escape") {
      show(back);
    } else if (event.key === " ") {
      event.preventDefault();
      next();
    }
  },
});
