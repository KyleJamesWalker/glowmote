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
  $("info-model").textContent = info?.model ?? "–";
  $("info-firmware").textContent = info?.firmware?.filter(Boolean).join(" · ") || "–";
  $("link").title = info ? `${info.model} · ${info.firmware.filter(Boolean).join(" · ")}` : "";
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

const WB_DEFAULT = { r: 100, g: 63, b: 44, temp: 0, tint: 0 };
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
  queuedColor = { rgb: balance(picked), brightness: Number($("brightness").value) };
  if (colorInFlight) return;
  colorInFlight = true;
  while (queuedColor) {
    const args = queuedColor;
    queuedColor = null;
    await run("color", args);
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
const COLOR_CAPTURED = new Set([0]);
const SCENE_EFFECTS = [
  "Scrolling bands", "Twinkle", "Color wipe", "Curtain", "Chasing dots", "Bar pass",
  "Stacking fill", "Meteor tails", "Pulse waves", "Gradient flow", "Jump", "Breathe",
];
const SCENE_CAPTURED = new Set([7]);

function effectChips(container, names, captured, selected) {
  $(container).replaceChildren(...names.map((name, mode) => {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.setAttribute("role", "radio");
    chip.setAttribute("aria-checked", String(mode === selected));
    chip.textContent = name;
    chip.disabled = !captured.has(mode);
    chip.title = chip.disabled ? `Mode ${mode}: needs a capture before glowmote can send it` : `Mode ${mode}`;
    return chip;
  }));
}

effectChips("color-effects", COLOR_EFFECTS, COLOR_CAPTURED, 0);
effectChips("scene-effects", SCENE_EFFECTS, SCENE_CAPTURED, 7);

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
  });
}

// Scenes

const SCENE_PRESETS = [
  ["Holiday", "#ff3b30", "#34c759"],
  ["Candy cane", "#ff3b30", "#ffffff"],
  ["Ocean", "#007aff", "#00c7be"],
  ["Sunset", "#ff9500", "#ff2d92"],
  ["Spooky", "#ff9500", "#af52de"],
  ["Ice", "#ffffff", "#30b0ff"],
];

let scenePlaying = false;
let sceneInFlight = false;
let sceneQueued = false;
async function sendScene() {
  sceneQueued = true;
  if (sceneInFlight) return;
  sceneInFlight = true;
  while (sceneQueued) {
    sceneQueued = false;
    await run("scene", {
      first: balance($("scene-first").value),
      second: balance($("scene-second").value),
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

const resendScene = () => { if (scenePlaying) sendScene(); };

$("scene-presets").replaceChildren(...SCENE_PRESETS.map(([name, a, b]) => {
  const button = document.createElement("button");
  button.type = "button";
  const duo = document.createElement("span");
  duo.className = "duo";
  duo.style.setProperty("--a", a);
  duo.style.setProperty("--b", b);
  button.append(duo, name);
  button.addEventListener("click", () => {
    $("scene-first").value = a;
    $("scene-second").value = b;
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
for (const id of ["scene-first", "scene-second", "scene-reverse"]) {
  $(id).addEventListener("input", resendScene);
}
$("scene-swap").addEventListener("click", () => {
  [$("scene-first").value, $("scene-second").value] = [$("scene-second").value, $("scene-first").value];
  resendScene();
});
$("scene-play").addEventListener("click", playScene);

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
  for (const [label, indexes] of arms()) {
    if (hiddenArms.includes(label)) continue;
    indexes.forEach((index, i) => setPixel(index, mix(from, to, indexes.length > 1 ? i / (indexes.length - 1) : 0)));
  }
}

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

// Replays a design as one fill with its most common color, then pixels for the rest.
function applyDesign(pixels) {
  const counts = new Map();
  for (const [, p] of pixels) {
    const key = `${p.color}/${p.brightness}`;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const [base] = [...counts].sort((a, b) => b[1] - a[1])[0] ?? [];
  if (base) {
    const [color, brightness] = base.split("/");
    fillAll(color, Number(brightness));
  }
  for (const [index, p] of pixels) setPixel(index, p.color, p.brightness);
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
resume();
