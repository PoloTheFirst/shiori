import { invoke, listen } from "../tauri.js";
import { followTheme } from "./theme.js";
import { applyBalloon, balloonStyle, playBalloon } from "../balloon-style.js";

const balloon = document.getElementById("balloon");
const words = document.getElementById("balloon-text");
let saying = 0;
let settings = {};
let speaker = null;

async function say({ text, mascot } = {}) {
  const mine = ++saying;
  speaker = mascot || null;
  const look = balloonStyle(settings, speaker);
  applyBalloon(balloon, look);
  balloon.classList.remove("shown", "to-right", "enter");
  balloon.style.removeProperty("width");
  balloon.style.removeProperty("height");
  clearInterval(balloon._typing);
  words.textContent = text || "";
  if (!text) {
    return;
  }
  await document.fonts.ready;

  const style = getComputedStyle(balloon);
  const marginX = parseFloat(style.marginLeft) + parseFloat(style.marginRight);
  const marginY = parseFloat(style.marginTop) + parseFloat(style.marginBottom);
  const scale = window.devicePixelRatio || 1;
  const width = Math.ceil((balloon.offsetWidth + marginX + 4) * scale);
  const height = Math.ceil((balloon.offsetHeight + marginY + 4) * scale);

  const anchor = Array.isArray(look.pos) ? look.pos : null;
  const side = await invoke("place_balloon", { width, height, anchor });
  if (mine !== saying) {
    return;
  }
  document.body.classList.toggle("right", side === "right");
  balloon.classList.toggle("to-right", side === "right");
  requestAnimationFrame(() => {
    balloon.classList.add("shown");
    playBalloon(balloon, words, text, look);
  });
}

function hush() {
  saying += 1;
  balloon.classList.remove("shown");
}

balloon.addEventListener("click", () => invoke("hide_balloon"));

async function start() {
  await followTheme();
  settings = (await invoke("get_state")).settings || {};
  await listen("settings-changed", (event) => {
    settings = event.payload || {};
    applyBalloon(balloon, balloonStyle(settings, speaker));
  });
  await listen("say", (event) => say(event.payload));
  await listen("hush", hush);
  say(await invoke("balloon_text"));
}

start();
