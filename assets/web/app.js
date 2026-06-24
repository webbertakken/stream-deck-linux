"use strict";

const $ = (id) => document.getElementById(id);
const state = {
  model: { keyCount: 15, columns: 5, rows: 3, name: "" },
  brightness: 60,
  buttons: new Map(), // key -> button object
  apps: [],
  selected: 0,
};

function statusMsg(text, kind) {
  const el = $("status");
  el.textContent = text || "\u00a0";
  if (kind) el.dataset.kind = kind;
  else delete el.dataset.kind;
}

// A button is empty when nothing is set at all.
function isEmpty(b) {
  return (
    !b ||
    (!b.image && !b.color && !b.label && !b.run && !b.builtin && !b.text_color)
  );
}

// A button is persistable/renderable only with a visual (image/colour/label).
function hasVisual(b) {
  return !!(b && (b.image || b.color || b.label));
}

function previewUrl(key) {
  return `/api/preview/${key}.png?ts=${Date.now()}`;
}

function refreshPreview(key) {
  const cell = document.querySelector(`.key[data-key="${key}"]`);
  if (cell) cell.style.backgroundImage = `url(${previewUrl(key)})`;
}

function refreshAllPreviews() {
  for (let k = 0; k < state.model.keyCount; k++) refreshPreview(k);
}

function buildGrid() {
  const grid = $("grid");
  grid.style.setProperty("--cols", state.model.columns);
  grid.innerHTML = "";
  for (let k = 0; k < state.model.keyCount; k++) {
    const cell = document.createElement("button");
    cell.type = "button";
    cell.className = "key";
    cell.dataset.key = String(k);
    cell.setAttribute("aria-pressed", k === state.selected ? "true" : "false");
    cell.setAttribute("aria-label", `Key ${k}`);
    cell.style.backgroundImage = `url(${previewUrl(k)})`;
    cell.innerHTML = `<span class="key__idx">${k}</span>`;
    cell.addEventListener("click", () => selectKey(k));
    grid.appendChild(cell);
  }
}

function buildAppOptions() {
  const sel = $("appSelect");
  sel.innerHTML = '<option value="">Choose an application\u2026</option>';
  for (const app of state.apps) {
    const opt = document.createElement("option");
    opt.value = app.command;
    opt.textContent = app.name;
    opt.dataset.name = app.name;
    if (app.icon) opt.dataset.icon = app.icon;
    sel.appendChild(opt);
  }
}

function selectKey(key) {
  state.selected = key;
  document.querySelectorAll(".key").forEach((c) =>
    c.setAttribute("aria-pressed", c.dataset.key === String(key) ? "true" : "false"),
  );
  $("selKey").textContent = String(key);
  populateForm(state.buttons.get(key) || { key });
}

function populateForm(b) {
  $("label").value = b.label || "";
  $("useText").checked = !!b.text_color;
  $("textColor").value = b.text_color || "#ffffff";
  $("useColor").checked = !!b.color;
  $("color").value = b.color || "#1e1e2e";
  $("image").value = b.image || "";

  let act = "none";
  if (b.run && b.run.startsWith("gtk-launch ")) act = "openapp";
  else if (b.run) act = "run";
  else if (b.builtin) act = "builtin";
  document.querySelectorAll('input[name="act"]').forEach((r) => {
    r.checked = r.value === act;
  });
  $("run").value = b.run || "";
  $("appSelect").value = act === "openapp" ? b.run : "";

  let builtin = "brightness_up";
  let bvalue = 70;
  if (b.builtin) {
    if (b.builtin.startsWith("brightness_set:") || b.builtin.startsWith("brightness:")) {
      builtin = "brightness_set";
      bvalue = parseInt(b.builtin.split(":")[1], 10) || 70;
    } else {
      builtin = b.builtin;
    }
  }
  $("builtin").value = builtin;
  $("builtinValue").value = bvalue;

  syncActionVisibility();
}

function currentAction() {
  return document.querySelector('input[name="act"]:checked')?.value || "none";
}

function syncActionVisibility() {
  const act = currentAction();
  $("appSelect").hidden = act !== "openapp";
  $("run").hidden = act !== "run";
  $("builtinRow").hidden = act !== "builtin";
  $("builtinValue").hidden = !($("builtin").value === "brightness_set");
}

function readForm() {
  const key = state.selected;
  const b = { key };
  const label = $("label").value.trim();
  if (label) b.label = label;
  if ($("useText").checked) b.text_color = $("textColor").value;
  if ($("useColor").checked) b.color = $("color").value;
  const image = $("image").value.trim();
  if (image) b.image = image;

  const act = currentAction();
  if (act === "openapp") {
    const cmd = $("appSelect").value;
    if (cmd) b.run = cmd;
  } else if (act === "run") {
    const run = $("run").value.trim();
    if (run) b.run = run;
  } else if (act === "builtin") {
    const sel = $("builtin").value;
    b.builtin = sel === "brightness_set" ? `brightness_set:${$("builtinValue").value}` : sel;
  }
  return b;
}

function onFormChange() {
  const b = readForm();
  if (isEmpty(b)) state.buttons.delete(state.selected);
  else state.buttons.set(state.selected, b);
  syncActionVisibility();
  scheduleApply();
}

// When an app is chosen, helpfully fill the label and icon if still blank.
function onAppPicked() {
  const opt = $("appSelect").selectedOptions[0];
  if (opt && opt.value) {
    if (!$("label").value.trim() && opt.dataset.name) $("label").value = opt.dataset.name;
    if (!$("image").value.trim() && opt.dataset.icon) $("image").value = opt.dataset.icon;
  }
  onFormChange();
}

function collectConfig() {
  const buttons = [...state.buttons.values()]
    .filter(hasVisual)
    .sort((a, z) => a.key - z.key);
  return { brightness: state.brightness, buttons };
}

let applyTimer = null;
function scheduleApply() {
  clearTimeout(applyTimer);
  applyTimer = setTimeout(applyNow, 250);
}

async function applyNow() {
  try {
    const res = await fetch("/api/state", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(collectConfig()),
    });
    if (res.ok) {
      statusMsg("Applied.", "ok");
      refreshAllPreviews();
    } else {
      statusMsg(`Could not apply: ${await res.text()}`, "err");
    }
  } catch (err) {
    statusMsg(`Network error: ${err}`, "err");
  }
}

let brightnessTimer = null;
function onBrightness() {
  const value = parseInt($("brightness").value, 10);
  state.brightness = value;
  $("brightnessOut").textContent = `${value}%`;
  clearTimeout(brightnessTimer);
  brightnessTimer = setTimeout(() => {
    fetch("/api/brightness", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ value }),
    }).catch(() => {});
  }, 120);
}

function clearKey() {
  state.buttons.delete(state.selected);
  populateForm({ key: state.selected });
  applyNow();
}

async function load() {
  const [stateRes, appsRes] = await Promise.all([
    fetch("/api/state"),
    fetch("/api/apps"),
  ]);
  const data = await stateRes.json();
  state.apps = await appsRes.json().catch(() => []);
  state.model = data.model;
  state.brightness = data.brightness ?? 60;
  state.buttons = new Map();
  for (const b of data.buttons || []) state.buttons.set(b.key, b);

  $("model").textContent = data.model.name ? `\u00b7 ${data.model.name}` : "";
  $("brightness").value = state.brightness;
  $("brightnessOut").textContent = `${state.brightness}%`;

  buildAppOptions();
  buildGrid();
  selectKey(0);
  statusMsg(`Ready \u2014 ${state.apps.length} apps. Edits apply live.`, "ok");
}

function wire() {
  $("form").addEventListener("input", onFormChange);
  $("form").addEventListener("change", onFormChange);
  $("builtin").addEventListener("change", syncActionVisibility);
  $("appSelect").addEventListener("change", onAppPicked);
  $("clearKey").addEventListener("click", clearKey);
  $("brightness").addEventListener("input", onBrightness);
}

wire();
load().catch((err) => statusMsg(`Failed to load: ${err}`, "err"));
