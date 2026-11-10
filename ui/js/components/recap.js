import { el } from "../dom.js";
import { invoke } from "../tauri.js";
import { store } from "../store.js";
import { call, openDialog } from "../ui.js";
import { t } from "../i18n.js";

export function recapYear(today = new Date()) {
  return today.getMonth() === 0 ? today.getFullYear() - 1 : today.getFullYear();
}

function inSeason(today) {
  const month = today.getMonth();
  const day = today.getDate();
  return (month === 11 && day >= 20) || (month === 0 && day <= 10);
}

export async function openRecap(year = recapYear()) {
  const url = await invoke("recap_link", { year });
  await call("open_url", { url });
}

export async function offerRecap(today = new Date()) {
  if (!inSeason(today)) {
    return false;
  }
  const year = recapYear(today);
  if (Number(store.settings.recap_offered) >= year) {
    return false;
  }
  const recap = await invoke("year_recap", { year }).catch(() => null);
  if (!recap || recap.seconds < 3600) {
    return false;
  }
  await invoke("update_settings", { changes: { recap_offered: year } });

  const hours = Math.round(recap.seconds / 3600);
  const open = await openDialog({
    title: t("Your {year} in visual novels", { year }),
    body: el(
      "p",
      { class: "muted" },
      t("{hours} h of reading, your top VNs and more. It opens in your browser, and your data stays in the link.", {
        hours,
      }),
    ),
    actions: [
      { label: t("Later"), kind: "secondary", value: false },
      { label: t("Open my recap"), value: true },
    ],
  });
  if (open) {
    await openRecap(year);
  }
  return true;
}
