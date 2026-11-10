export const BALLOON_FONTS = {
  app: "",
  rounded: '"Arial Rounded MT Bold", "Segoe UI", sans-serif',
  serif: 'Georgia, "Yu Mincho", "Times New Roman", serif',
  mono: 'Consolas, "Cascadia Mono", monospace',
  hand: '"Comic Sans MS", "Segoe Print", cursive',
};

export const BALLOON_DEFAULT = {
  bg: null,
  border: null,
  text: null,
  width: 2,
  radius: 16,
  size: 13,
  font: "app",
  tail: false,
  shadow: false,
  bold: false,
  anim: "pop",
  pos: null,
};

export const BALLOON_ANIMS = ["pop", "fade", "bounce", "slide", "type"];

export const BALLOON_PRESETS = [
  ["Theme", {}],
  ["Paper", { bg: "#fffdf6", border: "#2b2b2b", text: "#222222", width: 2, radius: 18, font: "rounded", tail: true, shadow: true }],
  ["Night", { bg: "#15131f", border: "#8b7cff", text: "#ece9ff", width: 2, radius: 14, shadow: true }],
  ["Sakura", { bg: "#fff0f5", border: "#ff8fb1", text: "#5a2a3a", width: 2, radius: 22, font: "rounded", tail: true }],
  ["Comic", { bg: "#ffffff", border: "#000000", text: "#000000", width: 3, radius: 6, font: "hand", tail: true, bold: true }],
  ["Mint", { bg: "#eafff5", border: "#3ccf91", text: "#12432f", width: 2, radius: 12, tail: true }],
  ["Minimal", { bg: "#1e1f24", border: "#1e1f24", text: "#f2f3f5", width: 0, radius: 10, shadow: true }],
  ["Novel", { bg: "#101014", border: "#d8c48a", text: "#f4ecd6", width: 1, radius: 4, font: "serif", shadow: true }],
];

export function balloonStyle(settings, id) {
  return { ...BALLOON_DEFAULT, ...(settings?.mascot_balloons?.[id] || {}) };
}

export function applyBalloon(node, style) {
  const s = { ...BALLOON_DEFAULT, ...(style || {}) };
  const set = (name, value) =>
    value === null || value === "" ? node.style.removeProperty(name) : node.style.setProperty(name, value);
  set("--b-bg", s.bg);
  set("--b-border", s.border);
  set("--b-text", s.text);
  set("--b-width", `${s.width}px`);
  set("--b-radius", `${s.radius}px`);
  set("--b-size", `${s.size}px`);
  set("--b-font", BALLOON_FONTS[s.font] || "");
  node.classList.toggle("tail", !!s.tail);
  node.classList.toggle("shadow", !!s.shadow);
  node.classList.toggle("bold", !!s.bold);
  for (const anim of BALLOON_ANIMS) {
    node.classList.toggle(`anim-${anim}`, s.anim === anim);
  }
}

export function playBalloon(node, words, text, style) {
  node.classList.remove("enter");
  void node.offsetWidth;
  node.classList.add("enter");
  clearInterval(node._typing);
  if ((style?.anim || "pop") !== "type" || !text) {
    words.textContent = text;
    node.style.removeProperty("width");
    node.style.removeProperty("height");
    return;
  }
  words.textContent = text;
  node.style.width = `${node.offsetWidth}px`;
  node.style.height = `${node.offsetHeight}px`;
  const letters = [...text];
  const step = Math.max(12, Math.min(35, 1400 / letters.length));
  let shown = 0;
  words.textContent = "";
  node._typing = setInterval(() => {
    shown += 1;
    words.textContent = letters.slice(0, shown).join("");
    if (shown >= letters.length) {
      clearInterval(node._typing);
    }
  }, step);
}
