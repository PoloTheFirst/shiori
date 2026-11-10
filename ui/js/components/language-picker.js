import { el } from "../dom.js";
import { t } from "../i18n.js";
import { store } from "../store.js";
import { call } from "../ui.js";

const flag = (code) => el("img", { class: "flag", src: `assets/flags/${code}.png`, alt: "" });

export function languagePicker(
  onPick,
  { current = store.settings.language || "auto", first = { code: "auto", name: "Same as Windows", flag: "__" }, right = false } = {},
) {
  const label = el("span");
  const button = el("button", { class: "btn secondary lang-button" }, label, " ▾");
  const list = el("div", { class: `menu lang-menu ${right ? "right" : ""}`, role: "menu", hidden: true });
  const picker = el("div", { class: "lang-picker" }, button, list);

  button.onclick = () => (list.hidden = !list.hidden);
  document.addEventListener("click", (event) => {
    if (!picker.contains(event.target)) {
      list.hidden = true;
    }
  });

  call("languages").then((languages) => {
    const choices = [{ ...first, name: t(first.name) }, ...languages];
    const items = choices.map((choice) => {
      const item = el("button", { role: "menuitemradio" }, flag(choice.flag), choice.name);
      item.onclick = () => {
        list.hidden = true;
        select(choice.code);
        onPick(choice.code);
      };
      return item;
    });
    const select = (code) => {
      const index = Math.max(0, choices.findIndex((choice) => choice.code === code));
      items.forEach((item, i) => item.classList.toggle("on", i === index));
      label.replaceChildren(flag(choices[index].flag), choices[index].name);
    };
    list.append(...items);
    select(current);
  });

  return picker;
}
