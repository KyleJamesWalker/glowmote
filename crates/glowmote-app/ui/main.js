const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

const PALETTE = [
  "#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#00c7be", "#30b0ff",
  "#007aff", "#5856d6", "#af52de", "#ff2d92", "#ffd8a8", "#ffffff",
];

function setStatus(text, isError = false) {
  $("status").textContent = text;
  $("status").classList.toggle("error", isError);
}

function setLink(state, text) {
  $("link").dataset.state = state;
  $("link-text").textContent = text;
}

async function run(command, args) {
  try {
    return await invoke(command, args);
  } catch (e) {
    const message = String(e);
    setStatus(message, true);
    if (message === "not connected") showDisconnected("Connection lost");
  }
}

// Lights

async function scan() {
  const before = [$("link").dataset.state, $("link-text").textContent];
  $("scan").disabled = true;
  setLink("busy", "Scanning…");
  try {
    const names = await invoke("scan_lights");
    $("devices").replaceChildren(...names.map(deviceButton));
    if (before[0] === "on") markCurrent(before[1]);
    setStatus(names.length ? "" : "No lights found. Close the vendor app and scan again.");
  } catch (e) {
    setStatus(String(e), true);
  } finally {
    $("scan").disabled = false;
    if ($("link").dataset.state === "busy") setLink(...before);
  }
}

function deviceButton(name) {
  const li = document.createElement("li");
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = name;
  button.addEventListener("click", () => connect(name));
  li.append(button);
  return li;
}

function remember(name) {
  try {
    if (name) localStorage.setItem("lastLight", name);
    else localStorage.removeItem("lastLight");
  } catch {}
}

function lastLight() {
  try {
    return localStorage.getItem("lastLight");
  } catch {
    return null;
  }
}

function markCurrent(name) {
  for (const b of $("devices").querySelectorAll("button")) {
    if (b.textContent === name) b.setAttribute("aria-current", "true");
    else b.removeAttribute("aria-current");
  }
}

async function showConnected(name) {
  markCurrent(name);
  $("controls").inert = false;
  $("disconnect").hidden = false;
  setLink("on", name);
  const info = await run("info");
  $("info-model").textContent = info ? info.model ?? "unavailable on this platform" : "–";
  $("info-firmware").textContent = info?.firmware?.filter(Boolean).join(" · ") || "–";
  $("link").title = info ? [info.model, ...info.firmware].filter(Boolean).join(" · ") : "";
}

function showDisconnected(text = "Not connected") {
  markCurrent(null);
  $("controls").inert = true;
  $("disconnect").hidden = true;
  $("diy-mode").checked = false;
  $("diy-tools").inert = true;
  $("diy-leave").hidden = true;
  $("info-model").textContent = "–";
  $("info-firmware").textContent = "–";
  $("link").title = "";
  setLink("off", text);
}

async function connect(name) {
  setLink("busy", `Connecting to ${name}…`);
  setStatus("");
  $("controls").inert = true;
  $("diy-mode").checked = false;
  $("diy-tools").inert = true;
  try {
    await invoke("connect", { name });
    remember(name);
    await showConnected(name);
  } catch (e) {
    showDisconnected();
    setStatus(String(e), true);
  }
}

async function disconnect() {
  await run("disconnect");
  remember(null);
  showDisconnected();
}

// Adopts a connection the backend already holds, otherwise reconnects to the last light used.
async function resume() {
  const current = await run("status");
  if (current) {
    $("devices").replaceChildren(deviceButton(current));
    await showConnected(current);
    return;
  }
  const last = lastLight();
  if (!last) return;
  setStatus(`Looking for ${last}…`);
  await scan();
  if ([...$("devices").querySelectorAll("button")].some((b) => b.textContent === last)) {
    await connect(last);
  } else {
    setStatus(`${last} not found. Close the vendor app, then scan.`);
  }
}

// Power

function showPower(on) {
  $("on").setAttribute("aria-pressed", String(on));
  $("off").setAttribute("aria-pressed", String(!on));
  document.body.classList.toggle("off", !on);
}

$("on").addEventListener("click", () => { showPower(true); run("power", { on: true }); });
$("off").addEventListener("click", () => { showPower(false); run("power", { on: false }); });

// Color

// White balance

const WB_DEFAULT = { r: 100, g: 55, b: 40, temp: 0, tint: 0 };
const WB_KEYS = ["r", "g", "b", "temp", "tint"];
let wb = { ...WB_DEFAULT };

function loadWb() {
  try {
    wb = { ...WB_DEFAULT, ...JSON.parse(localStorage.getItem("whiteBalance") ?? "{}") };
  } catch {}
}

function saveWb() {
  try {
    localStorage.setItem("whiteBalance", JSON.stringify(wb));
  } catch {}
}

const signed = (v, low, high) => (v === 0 ? "0" : `${Math.abs(v)} ${v < 0 ? low : high}`);

function showWb() {
  for (const key of WB_KEYS) $(`wb-${key}`).value = wb[key];
  for (const ch of ["r", "g", "b"]) $(`wb-${ch}-value`).textContent = `${wb[ch]}%`;
  $("wb-temp-value").textContent = signed(wb.temp, "cool", "warm");
  $("wb-tint-value").textContent = signed(wb.tint, "green", "magenta");
}

// Gains only ever reduce a channel, so a balanced color never exceeds what was picked.
function gains() {
  let r = wb.r / 100, g = wb.g / 100, b = wb.b / 100;
  if (wb.temp > 0) b *= 1 - wb.temp / 100;
  if (wb.temp < 0) r *= 1 + wb.temp / 100;
  if (wb.tint > 0) g *= 1 - wb.tint / 100;
  if (wb.tint < 0) { r *= 1 + wb.tint / 100; b *= 1 + wb.tint / 100; }
  return [r, g, b];
}

function balance(hex) {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = gains();
  const ch = (v, gain) => Math.round(v * gain).toString(16).padStart(2, "0");
  return `#${ch(n >> 16, r)}${ch((n >> 8) & 0xff, g)}${ch(n & 0xff, b)}`;
}

let previewingWhite = false;

// The picker and slider fire faster than the light acknowledges; only the latest value is sent.
let colorInFlight = false;
let queuedColor = null;
async function sendColor(picked = $("color").value) {
  const brightness = Number($("brightness").value);
  queuedColor = colorEffect === 0 || picked === "#ffffff" && previewingWhite
    ? ["color", { rgb: balance(picked), brightness }]
    : ["effect", {
      mode: colorEffect,
      first: balance(picked),
      second: balance($("effect-second").value),
      speed: Number($("effect-speed").value),
      brightness,
      reverse: $("effect-reverse").checked,
    }];
  if (colorInFlight) return;
  colorInFlight = true;
  while (queuedColor) {
    const [command, args] = queuedColor;
    queuedColor = null;
    await run(command, args);
  }
  colorInFlight = false;
}

function showColor(hex) {
  document.documentElement.style.setProperty("--c", hex);
  for (const swatch of $("palette").children) {
    swatch.setAttribute("aria-pressed", String(swatch.dataset.color === hex));
  }
}

function pickColor(hex, send) {
  $("color").value = hex;
  showColor(hex);
  if (send) {
    showPower(true);
    sendColor();
  }
}

const onColorTab = () => !$("panel-color").hidden;

$("palette").replaceChildren(...PALETTE.map((hex) => {
  const swatch = document.createElement("button");
  swatch.type = "button";
  swatch.dataset.color = hex;
  swatch.setAttribute("aria-label", hex);
  swatch.style.setProperty("--swatch", hex);
  swatch.addEventListener("click", () => pickColor(hex, onColorTab()));
  return swatch;
}));

$("color").addEventListener("input", () => pickColor($("color").value, onColorTab()));
$("brightness").addEventListener("input", () => {
  $("brightness-value").textContent = `${$("brightness").value}%`;
  sendColor();
});

// Effects: names come from the firmware's renderers; only captured modes are enabled.

const COLOR_EFFECTS = [
  "Static", "Running bar", "Stacking", "Meteor", "Pulse wave",
  "Theater chase", "Running dot", "Bar sweep", "Breathe", "Strobe",
];
const COLOR_CAPTURED = new Set([0, 1]);
const SCENE_EFFECTS = [
  "Scrolling bands", "Twinkle", "Color wipe", "Curtain", "Chasing dots", "Bar pass",
  "Stacking fill", "Meteor tails", "Pulse waves", "Gradient flow", "Jump", "Breathe",
];
const SCENE_CAPTURED = new Set([0, 1, 7, 8, 9]);

function effectChips(container, names, captured, selected, onPick) {
  const chips = names.map((name, mode) => {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.setAttribute("role", "radio");
    chip.setAttribute("aria-checked", String(mode === selected));
    chip.textContent = name;
    chip.classList.toggle("predicted", !captured.has(mode));
    chip.title = captured.has(mode) ? `Mode ${mode}` : `Mode ${mode}: predicted from the firmware, not captured yet`;
    chip.addEventListener("click", () => {
      for (const c of chips) c.setAttribute("aria-checked", String(c === chip));
      onPick?.(mode);
    });
    return chip;
  });
  $(container).replaceChildren(...chips);
}

let sceneEffect = 7;
// The vendor app's default background per mode for this light (LightColorCore, type 3).
const EFFECT_BACKGROUNDS = [
  "#ff0000", "#0000ff", "#ffffff", "#000000", "#000000",
  "#0000ff", "#00ff00", "#ff00ff", "#00ff00", "#0000ff",
];

let colorEffect = 0;
effectChips("color-effects", COLOR_EFFECTS, COLOR_CAPTURED, colorEffect, (mode) => {
  colorEffect = mode;
  $("effect-second").value = EFFECT_BACKGROUNDS[mode];
  $("effect-controls").hidden = mode === 0;
  if (onColorTab()) {
    showPower(true);
    sendColor();
  }
});
effectChips("scene-effects", SCENE_EFFECTS, SCENE_CAPTURED, sceneEffect, (mode) => {
  sceneEffect = mode;
  resendScene();
});

$("effect-speed").addEventListener("input", () => {
  $("effect-speed-value").textContent = `${$("effect-speed").value}%`;
  sendColor();
});
for (const id of ["effect-second", "effect-reverse"]) $(id).addEventListener("input", () => sendColor());

// Music: this computer's microphone streams levels to the light, or the light listens itself.
// Effect names are not known yet; 0-2 are captured.

let musicEffect = 0;
let musicSource = "here";

function musicArgs() {
  return { effect: musicEffect, sensitivity: Number($("sensitivity").value) };
}

async function setMusic(on) {
  if (on) showPower(true);
  if (musicSource === "here") {
    await run("listen", { on, ...musicArgs() });
  } else {
    await run("music", { on, effect: musicEffect });
  }
  if (!on) $("music-meter").style.setProperty("--level", "0%");
}

effectChips("music-effects", Array.from({ length: 10 }, (_, i) => `Effect ${i + 1}`), new Set([0, 1, 2]), 0, (mode) => {
  musicEffect = mode;
  if ($("music-on").checked) setMusic(true);
});
$("music-on").addEventListener("change", () => setMusic($("music-on").checked));
$("sensitivity").addEventListener("input", () => {
  $("sensitivity-value").textContent = `${$("sensitivity").value}%`;
  if ($("music-on").checked && musicSource === "here") run("listen", { on: true, ...musicArgs() });
});
for (const [id, source] of [["source-here", "here"], ["source-light", "light"]]) {
  $(id).addEventListener("click", async () => {
    if (source === musicSource) return;
    const wasOn = $("music-on").checked;
    if (wasOn) await setMusic(false);
    musicSource = source;
    $("source-here").setAttribute("aria-checked", String(source === "here"));
    $("source-light").setAttribute("aria-checked", String(source === "light"));
    $("here-controls").hidden = source !== "here";
    if (wasOn) await setMusic(true);
  });
}
window.__TAURI__.event.listen("music-level", ({ payload }) => {
  $("music-meter").style.setProperty("--level", `${payload}%`);
});
window.__TAURI__.event.listen("music-error", ({ payload }) => {
  $("music-on").checked = false;
  setStatus(payload, true);
});

// Tabs

for (const tab of document.querySelectorAll('[role="tab"]')) {
  tab.addEventListener("click", () => {
    for (const t of document.querySelectorAll('[role="tab"]')) {
      const selected = t === tab;
      t.setAttribute("aria-selected", String(selected));
      $(t.getAttribute("aria-controls")).hidden = !selected;
    }
    previewingWhite = false;
    scenePlaying = false;
    if (tab.id === "tab-diy") selectSlot(currentSlot);
  });
}

// Scenes

const RAINBOW = ["#ff0000", "#ffa200", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff"];
const SCENE_PRESETS = [
  ["Rainbow", RAINBOW],
  ["Holiday", ["#ff3b30", "#34c759"]],
  ["Candy cane", ["#ff3b30", "#ffffff"]],
  ["Ocean", ["#007aff", "#00c7be", "#ffffff"]],
  ["Sunset", ["#ff9500", "#ff2d92", "#af52de"]],
  ["Spooky", ["#ff9500", "#af52de", "#34c759"]],
  ["Ice", ["#ffffff", "#30b0ff"]],
];
const MAX_SCENE_COLORS = 7;

function sceneColors() {
  return [...$("scene-colors").querySelectorAll("input")].map((input) => input.value);
}

function setSceneColors(colors) {
  $("scene-colors").replaceChildren(...colors.map((hex, i) => {
    const well = document.createElement("label");
    well.className = "well";
    const input = document.createElement("input");
    input.type = "color";
    input.value = hex;
    input.setAttribute("aria-label", `Color ${i + 1}`);
    input.addEventListener("input", resendScene);
    well.append(input);
    return well;
  }));
  $("scene-add").disabled = colors.length >= MAX_SCENE_COLORS;
  $("scene-remove").disabled = colors.length <= 1;
}

let scenePlaying = false;
let sceneInFlight = false;
let sceneQueued = false;
async function sendScene() {
  sceneQueued = true;
  if (sceneInFlight) return;
  sceneInFlight = true;
  while (sceneQueued) {
    sceneQueued = false;
    await run("scene_effect", {
      effect: sceneEffect,
      colors: sceneColors().map(balance),
      speed: Number($("scene-speed").value),
      brightness: Number($("scene-brightness").value),
      reverse: $("scene-reverse").checked,
    });
  }
  sceneInFlight = false;
}

function playScene() {
  scenePlaying = true;
  showPower(true);
  sendScene();
}

function resendScene() {
  if (scenePlaying) sendScene();
}

const stops = (colors) => colors.map((c, i) =>
  `${c} ${(i * 100) / colors.length}% ${((i + 1) * 100) / colors.length}%`).join(", ");

$("scene-presets").replaceChildren(...SCENE_PRESETS.map(([name, colors]) => {
  const button = document.createElement("button");
  button.type = "button";
  const duo = document.createElement("span");
  duo.className = "duo";
  duo.style.setProperty("--stops", stops(colors));
  button.append(duo, name);
  button.addEventListener("click", () => {
    setSceneColors(colors);
    playScene();
  });
  return button;
}));

for (const [id, unit] of [["scene-speed", "%"], ["scene-brightness", "%"]]) {
  $(id).addEventListener("input", () => {
    $(`${id}-value`).textContent = `${$(id).value}${unit}`;
    resendScene();
  });
}
$("scene-reverse").addEventListener("input", resendScene);
$("scene-add").addEventListener("click", () => {
  const colors = sceneColors();
  setSceneColors([...colors, colors.at(-1)]);
  resendScene();
});
$("scene-remove").addEventListener("click", () => {
  setSceneColors(sceneColors().slice(0, -1));
  resendScene();
});
setSceneColors(SCENE_PRESETS[1][1]);
$("scene-play").addEventListener("click", playScene);

// Timers: the light cannot report its timers, so the last saved ones are remembered here.

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function timerField(id) {
  const box = $(id);
  return {
    enabled: box.querySelector(".timer-enabled"),
    time: box.querySelector(".timer-time"),
    days: box.querySelector(".timer-days"),
  };
}

function buildDays(field) {
  field.days.replaceChildren(...WEEKDAYS.map((name) => {
    const day = document.createElement("button");
    day.type = "button";
    day.textContent = name;
    day.setAttribute("aria-pressed", "false");
    day.addEventListener("click", () => {
      day.setAttribute("aria-pressed", String(day.getAttribute("aria-pressed") !== "true"));
    });
    return day;
  }));
}

function readTimer(field) {
  if (!field.enabled.checked) return null;
  const [hour, minute] = field.time.value.split(":").map(Number);
  const days = [...field.days.children]
    .reduce((mask, day, bit) => mask | (day.getAttribute("aria-pressed") === "true" ? 1 << bit : 0), 0);
  return { hour, minute, days };
}

function showTimer(field, timer) {
  field.enabled.checked = timer !== null;
  if (!timer) return;
  field.time.value = `${String(timer.hour).padStart(2, "0")}:${String(timer.minute).padStart(2, "0")}`;
  [...field.days.children].forEach((day, bit) =>
    day.setAttribute("aria-pressed", String(Boolean(timer.days & (1 << bit)))));
}

const timerOn = timerField("timer-on");
const timerOff = timerField("timer-off");
buildDays(timerOn);
buildDays(timerOff);
try {
  const saved = JSON.parse(localStorage.getItem("timers") ?? "null");
  if (saved) {
    showTimer(timerOn, saved.on);
    showTimer(timerOff, saved.off);
  }
} catch {}

$("timers-save").addEventListener("click", async () => {
  const d = new Date();
  const now = {
    weekday: d.getDay() || 7,
    hour: d.getHours(),
    minute: d.getMinutes(),
    second: d.getSeconds(),
  };
  const on = readTimer(timerOn);
  const off = readTimer(timerOff);
  if ((on && Number.isNaN(on.hour)) || (off && Number.isNaN(off.hour))) {
    setStatus("Pick a time for each enabled timer", true);
    return;
  }
  if ((await run("timers", { now, on, off })) !== undefined) {
    try { localStorage.setItem("timers", JSON.stringify({ on, off })); } catch {}
    setStatus("Timers saved to the light");
  }
});

// DIY

// Every pixel write must reach the light, so DIY commands run strictly in order.
let diyQueue = Promise.resolve();
let diyPending = 0;
function showProgress() {
  $("diy-progress").textContent = diyPending > 1 ? `Sending… ${diyPending} left` : "";
}
const queueDiy = (command, args) => {
  diyPending++;
  showProgress();
  diyQueue = diyQueue.then(() => run(command, args)).finally(() => {
    diyPending--;
    showProgress();
  });
  return diyQueue;
};
const diyBrightness = () => Number($("diy-brightness").value);
const ledCount = () => Math.max(1, Math.min(400, Number($("leds").value) || 180));
const splitAt = () => Math.min(ledCount() - 1, Math.max(0, Number($("split").value)));

let hiddenArms = [];

function loadLayout() {
  try {
    const saved = JSON.parse(localStorage.getItem("layout") ?? "null");
    if (saved) {
      $("leds").value = saved.leds;
      $("split").value = saved.split;
      hiddenArms = saved.hidden ?? [];
    }
  } catch {}
}

function saveLayout() {
  try {
    localStorage.setItem("layout", JSON.stringify({ leds: $("leds").value, split: $("split").value, hidden: hiddenArms }));
  } catch {}
}

// Indexes of each arm, ordered outward from the controller.
function arms() {
  const leds = ledCount();
  const split = splitAt();
  return [
    ["Side A", Array.from({ length: split + 1 }, (_, i) => split - i)],
    ["Side B", Array.from({ length: leds - split - 1 }, (_, i) => split + 1 + i)],
  ].filter(([, indexes]) => indexes.length);
}

function mirrorOf(index) {
  const split = splitAt();
  const other = index <= split ? split + 1 + (split - index) : split - (index - split - 1);
  return other >= 0 && other < ledCount() ? other : null;
}

function ledCell(index) {
  const cell = document.createElement("button");
  cell.type = "button";
  cell.dataset.index = index;
  cell.title = `LED ${index}`;
  cell.setAttribute("aria-label", `LED ${index}`);
  return cell;
}

function armView(label, indexes) {
  const section = document.createElement("div");
  section.className = "arm";
  section.classList.toggle("collapsed", hiddenArms.includes(label));
  const head = document.createElement("div");
  head.className = "arm-head";
  const ctrl = document.createElement("span");
  ctrl.className = "ctrl";
  ctrl.textContent = "Controller";
  const text = document.createElement("span");
  text.className = "arm-label";
  text.textContent = `${label} · ${indexes[0]} → ${indexes.at(-1)}`;
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.textContent = section.classList.contains("collapsed") ? "Show" : "Hide";
  toggle.addEventListener("click", () => {
    hiddenArms = hiddenArms.includes(label) ? hiddenArms.filter((l) => l !== label) : [...hiddenArms, label];
    saveLayout();
    buildStrip();
  });
  head.append(ctrl, text, toggle);
  const dots = document.createElement("div");
  dots.className = "dots";
  dots.replaceChildren(...indexes.map(ledCell));
  section.append(head, dots);
  return section;
}

let design = new Map();

function buildStrip() {
  $("strip").replaceChildren(...arms().map(([label, indexes]) => armView(label, indexes)));
  for (const [index, { color }] of design) showPixel(index, color);
}

const cellAt = (index) => $("strip").querySelector(`[data-index="${index}"]`);

function showPixel(index, color) {
  const cell = cellAt(index);
  if (!cell) return;
  cell.style.setProperty("--cell", color);
  cell.dataset.painted = "";
}

function setPixel(index, color, brightness = diyBrightness()) {
  const current = design.get(index);
  if (current && current.color === color && current.brightness === brightness) return;
  design.set(index, { color, brightness });
  showPixel(index, color);
  queueDiy("diy_pixel", { index, leds: ledCount(), rgb: balance(color), brightness });
}

function fillAll(color, brightness = diyBrightness()) {
  design = new Map(Array.from({ length: ledCount() }, (_, i) => [i, { color, brightness }]));
  for (const [index] of design) showPixel(index, color);
  queueDiy("diy_fill", { leds: ledCount(), rgb: balance(color), brightness });
}

// Redraws the whole strip in one bulk upload; LEDs missing from `next` go dark.
function drawAll(next) {
  design = next;
  buildStrip();
  const pixels = Array.from({ length: ledCount() }, (_, i) => {
    const pixel = design.get(i);
    return pixel ? balance(pixel.color) : "#000000";
  });
  queueDiy("diy_upload", { pixels });
}

function paint(cell) {
  const index = Number(cell.dataset.index);
  const color = $("color").value;
  setPixel(index, color);
  const mirror = $("mirror").checked ? mirrorOf(index) : null;
  if (mirror !== null) setPixel(mirror, color);
}

function mix(a, b, t) {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const ch = (shift) => Math.round(((pa >> shift) & 0xff) * (1 - t) + ((pb >> shift) & 0xff) * t);
  return `#${[16, 8, 0].map((s) => ch(s).toString(16).padStart(2, "0")).join("")}`;
}

function applyGradient() {
  const from = $("color").value;
  const to = $("gradient-to").value;
  const next = new Map(design);
  for (const [label, indexes] of arms()) {
    if (hiddenArms.includes(label)) continue;
    indexes.forEach((index, i) => next.set(index, {
      color: mix(from, to, indexes.length > 1 ? i / (indexes.length - 1) : 0),
      brightness: diyBrightness(),
    }));
  }
  drawAll(next);
}

// Paint slots: click to paint with a slot, click the selected slot again to change its color.

const SLOT_DEFAULTS = ["#ff3b30", "#34c759", "#007aff", "#ffd8a8", "#ff9500", "#af52de"];
let slots = [...SLOT_DEFAULTS];
let currentSlot = 0;

function loadSlots() {
  try {
    const saved = JSON.parse(localStorage.getItem("paintSlots") ?? "null");
    if (Array.isArray(saved) && saved.length === SLOT_DEFAULTS.length) slots = saved;
  } catch {}
}

function saveSlots() {
  try {
    localStorage.setItem("paintSlots", JSON.stringify(slots));
  } catch {}
}

function showSlots() {
  $("paint-slots").replaceChildren(...slots.map((hex, i) => {
    const slot = document.createElement("button");
    slot.type = "button";
    slot.setAttribute("role", "radio");
    slot.setAttribute("aria-checked", String(i === currentSlot));
    slot.setAttribute("aria-label", `Paint color ${i + 1}: ${hex}`);
    slot.title = i === currentSlot ? "Click to change this color" : `Paint with ${hex} (key ${i + 1})`;
    slot.style.setProperty("--slot", hex);
    const key = document.createElement("span");
    key.textContent = i + 1;
    slot.append(key);
    slot.addEventListener("click", () => {
      if (i === currentSlot) {
        $("slot-color").value = slots[i];
        $("slot-color").click();
      } else {
        selectSlot(i);
      }
    });
    return slot;
  }));
}

function selectSlot(i) {
  currentSlot = i;
  pickColor(slots[i], false);
  showSlots();
}

$("slot-color").addEventListener("input", () => {
  slots[currentSlot] = $("slot-color").value;
  saveSlots();
  selectSlot(currentSlot);
});

window.addEventListener("keydown", (e) => {
  if ($("panel-diy").hidden || e.target.matches("input, textarea") || e.metaKey || e.ctrlKey || e.altKey) return;
  const i = Number(e.key) - 1;
  if (i >= 0 && i < slots.length) selectSlot(i);
});

let painting = false;
$("strip").addEventListener("pointerdown", (e) => {
  if (e.target.dataset.index === undefined) return;
  painting = true;
  paint(e.target);
});
$("strip").addEventListener("pointerover", (e) => {
  if (painting && e.target.dataset.index !== undefined) paint(e.target);
});
window.addEventListener("pointerup", () => { painting = false; });

// Leaving DIY asks first: DSTOP 01 keeps the drawing showing, DSTOP 00 restores the previous mode.
$("diy-mode").addEventListener("change", async () => {
  if ($("diy-mode").checked) {
    await queueDiy("diy", { on: true });
    $("diy-tools").inert = false;
    return;
  }
  $("diy-mode").checked = true;
  $("diy-leave").hidden = false;
});

async function leaveDiy(keep) {
  $("diy-leave").hidden = true;
  $("diy-mode").checked = false;
  $("diy-tools").inert = true;
  await queueDiy("diy", { on: false, keep });
}

$("diy-keep").addEventListener("click", () => leaveDiy(true));
$("diy-restore").addEventListener("click", () => leaveDiy(false));
$("diy-stay").addEventListener("click", () => { $("diy-leave").hidden = true; });
$("diy-brightness").addEventListener("input", () => {
  $("diy-brightness-value").textContent = `${$("diy-brightness").value}%`;
});
for (const id of ["leds", "split"]) {
  $(id).addEventListener("change", () => { saveLayout(); buildStrip(); });
}
$("fill").addEventListener("click", () => fillAll($("color").value));
$("gradient").addEventListener("click", applyGradient);

// Saved designs

function loadDesigns() {
  try {
    return JSON.parse(localStorage.getItem("designs") ?? "{}");
  } catch {
    return {};
  }
}

function storeDesigns(designs) {
  try {
    localStorage.setItem("designs", JSON.stringify(designs));
  } catch {}
}

function applyDesign(pixels) {
  drawAll(new Map(pixels.filter(([index]) => index < ledCount())));
}

function showDesigns() {
  const designs = loadDesigns();
  $("designs").replaceChildren(...Object.entries(designs).map(([name, pixels]) => {
    const li = document.createElement("li");
    const apply = document.createElement("button");
    apply.type = "button";
    apply.className = "design-apply";
    const swatches = document.createElement("span");
    swatches.className = "swatches";
    for (const color of [...new Set(pixels.map(([, p]) => p.color))].slice(0, 5)) {
      const dot = document.createElement("span");
      dot.style.setProperty("background", color);
      swatches.append(dot);
    }
    apply.append(swatches, name);
    apply.addEventListener("click", () => applyDesign(pixels));
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "design-delete";
    remove.textContent = "✕";
    remove.setAttribute("aria-label", `Delete ${name}`);
    remove.addEventListener("click", () => {
      const all = loadDesigns();
      delete all[name];
      storeDesigns(all);
      showDesigns();
    });
    li.append(apply, remove);
    return li;
  }));
}

$("design-save").addEventListener("click", () => {
  const name = $("design-name").value.trim();
  if (!name || design.size === 0) {
    setStatus(design.size ? "Name the design first." : "Paint something first.", true);
    return;
  }
  storeDesigns({ ...loadDesigns(), [name]: [...design] });
  $("design-name").value = "";
  showDesigns();
});

for (const key of WB_KEYS) {
  $(`wb-${key}`).addEventListener("input", () => {
    wb[key] = Number($(`wb-${key}`).value);
    showWb();
    saveWb();
    if (previewingWhite) sendColor("#ffffff");
  });
}
$("wb-white").addEventListener("click", () => {
  previewingWhite = true;
  showPower(true);
  sendColor("#ffffff");
});
$("wb-reset").addEventListener("click", () => {
  wb = { ...WB_DEFAULT };
  showWb();
  saveWb();
  if (previewingWhite) sendColor("#ffffff");
});

$("scan").addEventListener("click", scan);
$("disconnect").addEventListener("click", disconnect);
loadWb();
showWb();
pickColor($("color").value, false);
loadLayout();
buildStrip();
showDesigns();
loadSlots();
showSlots();
resume();
