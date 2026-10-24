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
    setStatus(String(e), true);
  }
}

// Lights

async function scan() {
  $("scan").disabled = true;
  setLink("busy", "Scanning…");
  try {
    const names = await invoke("scan_lights");
    $("devices").replaceChildren(...names.map(deviceButton));
    setStatus(names.length ? "" : "No lights found. Close the vendor app and scan again.");
  } catch (e) {
    setStatus(String(e), true);
  } finally {
    $("scan").disabled = false;
    if ($("link").dataset.state === "busy") setLink("off", "Not connected");
  }
}

function deviceButton(name) {
  const li = document.createElement("li");
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = name;
  button.addEventListener("click", () => connect(name, button));
  li.append(button);
  return li;
}

async function connect(name, button) {
  setLink("busy", `Connecting to ${name}…`);
  setStatus("");
  $("controls").inert = true;
  $("diy-mode").checked = false;
  $("diy-tools").inert = true;
  try {
    await invoke("connect", { name });
    for (const b of $("devices").querySelectorAll("button")) b.removeAttribute("aria-current");
    button.setAttribute("aria-current", "true");
    $("controls").inert = false;
    setLink("on", name);
  } catch (e) {
    setLink("off", "Not connected");
    setStatus(String(e), true);
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

// Tabs

for (const tab of document.querySelectorAll('[role="tab"]')) {
  tab.addEventListener("click", () => {
    for (const t of document.querySelectorAll('[role="tab"]')) {
      const selected = t === tab;
      t.setAttribute("aria-selected", String(selected));
      $(t.getAttribute("aria-controls")).hidden = !selected;
    }
    previewingWhite = false;
  });
}

// DIY

// Every pixel write must reach the light, so DIY commands run strictly in order.
let diyQueue = Promise.resolve();
const queueDiy = (command, args) => {
  diyQueue = diyQueue.then(() => run(command, args));
  return diyQueue;
};
const diyArgs = () => ({
  rgb: balance($("color").value),
  brightness: Number($("diy-brightness").value),
  leds: Number($("leds").value),
});

function loadLayout() {
  try {
    const saved = JSON.parse(localStorage.getItem("layout") ?? "null");
    if (saved) {
      $("leds").value = saved.leds;
      $("split").value = saved.split;
    }
  } catch {}
}

function saveLayout() {
  try {
    localStorage.setItem("layout", JSON.stringify({ leds: $("leds").value, split: $("split").value }));
  } catch {}
}

function ledCell(index) {
  const cell = document.createElement("button");
  cell.type = "button";
  cell.dataset.index = index;
  cell.title = `LED ${index}`;
  cell.setAttribute("aria-label", `LED ${index}`);
  return cell;
}

function arm(label, indexes) {
  const section = document.createElement("div");
  section.className = "arm";
  const head = document.createElement("div");
  head.className = "arm-head";
  head.innerHTML = `<span class="ctrl">Controller</span><span></span>`;
  head.lastChild.textContent = `${label} · ${indexes[0]} → ${indexes.at(-1)}`;
  const dots = document.createElement("div");
  dots.className = "dots";
  dots.replaceChildren(...indexes.map(ledCell));
  section.append(head, dots);
  return section;
}

// Each arm is drawn starting from the LED next to the controller.
function buildStrip() {
  const leds = Math.max(1, Math.min(1024, Number($("leds").value) || 180));
  const split = Math.min(leds - 1, Math.max(0, Number($("split").value)));
  const sideA = Array.from({ length: split + 1 }, (_, i) => split - i);
  const sideB = Array.from({ length: leds - split - 1 }, (_, i) => split + 1 + i);
  $("strip").replaceChildren(arm("Side A", sideA), ...(sideB.length ? [arm("Side B", sideB)] : []));
}

const stripCells = () => $("strip").querySelectorAll("[data-index]");

function paint(cell) {
  const args = diyArgs();
  const shown = $("color").value;
  const key = `${shown}/${args.brightness}`;
  if (cell.dataset.painted === key) return;
  cell.dataset.painted = key;
  cell.style.setProperty("--cell", shown);
  queueDiy("diy_pixel", { index: Number(cell.dataset.index), ...args });
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

$("diy-mode").addEventListener("change", async () => {
  const on = $("diy-mode").checked;
  await queueDiy("diy", { on });
  $("diy-tools").inert = !on;
});
$("diy-brightness").addEventListener("input", () => {
  $("diy-brightness-value").textContent = `${$("diy-brightness").value}%`;
});
for (const id of ["leds", "split"]) {
  $(id).addEventListener("change", () => { saveLayout(); buildStrip(); });
}
$("fill").addEventListener("click", () => {
  const args = diyArgs();
  const shown = $("color").value;
  for (const cell of stripCells()) {
    cell.style.setProperty("--cell", shown);
    cell.dataset.painted = `${shown}/${args.brightness}`;
  }
  queueDiy("diy_fill", args);
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
loadWb();
showWb();
pickColor($("color").value, false);
loadLayout();
buildStrip();
