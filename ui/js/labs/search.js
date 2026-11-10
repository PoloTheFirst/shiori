import { $$, el } from "../dom.js";
import { show } from "../router.js";
import { invoke } from "../tauri.js";
import { t } from "../i18n.js";
import { raw } from "./util.js";

let overlay = null;

function close() {
  overlay?.remove();
  overlay = null;
}

export async function openSearch() {
  return open();
}

async function open() {
  close();
  const input = el("input", { type: "search", placeholder: "Search a VN or a page…" });
  const list = el("div", { class: "labs-search-list" });
  overlay = el(
    "div",
    { class: "labs-search-overlay" },
    el("div", { class: "card labs-search" }, input, list),
  );
  overlay.onclick = (event) => event.target === overlay && close();
  document.body.append(overlay);
  input.focus();

  const pages = $$("nav button").map((button) => ({
    label: button.textContent.trim(),
    hint: t("Page"),
    go: () => show(button.dataset.page),
  }));
  const games = (await invoke("get_library").catch(() => [])).map((game) => ({
    label: game.name,
    hint: "VN",
    go: () => show("game", { key: game.key }),
  }));
  const items = [...pages, ...games];
  let results = [];
  let selected = 0;

  const render = () => {
    const query = input.value.trim().toLowerCase();
    results = items.filter((item) => item.label.toLowerCase().includes(query)).slice(0, 12);
    selected = Math.min(selected, Math.max(0, results.length - 1));
    list.replaceChildren(
      ...results.map((item, i) => {
        const row = el(
          "button",
          { class: `labs-search-item ${i === selected ? "on" : ""}` },
          el("span", {}, raw(item.label)),
          el("span", { class: "muted small-text" }, raw(item.hint)),
        );
        row.onclick = () => {
          close();
          item.go();
        };
        return row;
      }),
    );
  };

  input.oninput = () => {
    selected = 0;
    render();
  };
  input.onkeydown = (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      selected = (selected + (event.key === "ArrowDown" ? 1 : -1) + results.length) % Math.max(1, results.length);
      render();
    } else if (event.key === "Enter" && results[selected]) {
      const item = results[selected];
      close();
      item.go();
    } else if (event.key === "Escape") {
      close();
    }
  };
  render();
}

document.addEventListener("keydown", (event) => {
  if (event.ctrlKey && event.key.toLowerCase() === "k") {
    event.preventDefault();
    if (overlay) {
      close();
    } else {
      open();
    }
  }
});
