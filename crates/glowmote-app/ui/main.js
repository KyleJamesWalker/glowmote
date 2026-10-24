const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

function setStatus(text, isError = false) {
  $("status").textContent = text;
  $("status").classList.toggle("error", isError);
}

async function scan() {
  $("scan").disabled = true;
  setStatus("Scanning…");
  try {
    const names = await invoke("scan_lights");
    $("devices").replaceChildren(...names.map(deviceButton));
    setStatus(names.length ? `Found ${names.length}` : "No lights found. Close the vendor app and scan again.");
  } catch (e) {
    setStatus(String(e), true);
  } finally {
    $("scan").disabled = false;
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
  setStatus(`Connecting to ${name}…`);
  $("controls").inert = true;
  try {
    await invoke("connect", { name });
    for (const b of $("devices").querySelectorAll("button")) b.removeAttribute("aria-current");
    button.setAttribute("aria-current", "true");
    $("controls").inert = false;
    setStatus(`Connected to ${name}`);
  } catch (e) {
    setStatus(String(e), true);
  }
}

async function run(command, args) {
  try {
    await invoke(command, args);
  } catch (e) {
    setStatus(String(e), true);
  }
}

// Sliders and the color picker fire faster than the light acknowledges; only the latest value is sent.
let colorInFlight = false;
let queuedColor = null;
async function sendColor() {
  queuedColor = { rgb: $("color").value, brightness: Number($("brightness").value) };
  if (colorInFlight) return;
  colorInFlight = true;
  while (queuedColor) {
    const args = queuedColor;
    queuedColor = null;
    await run("color", args);
  }
  colorInFlight = false;
}

$("scan").addEventListener("click", scan);
$("on").addEventListener("click", () => run("power", { on: true }));
$("off").addEventListener("click", () => run("power", { on: false }));
$("color").addEventListener("input", sendColor);
$("brightness").addEventListener("input", () => {
  $("brightness-value").textContent = `${$("brightness").value}%`;
  sendColor();
});
for (const swatch of document.querySelectorAll(".swatches button")) {
  swatch.style.setProperty("--c", swatch.dataset.color);
  swatch.addEventListener("click", () => {
    $("color").value = swatch.dataset.color;
    sendColor();
  });
}
