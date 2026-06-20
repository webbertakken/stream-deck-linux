"use strict";

const $ = (id) => document.getElementById(id);
const state = {
  model: { keyCount: 15, columns: 5, rows: 3, name: "" },
  brightness: 60,
  buttons: new Map(), // key -> button object
  selected: 0,
};

function statusMsg(text, kind) {
  const el = $("status");
  el.textContent = text || "\u00a0";
  if (kind) el.dataset.kind = kind;
  else delete el.dataset.kind;
}

function isEmpty(b) {
  return (
    !b ||
    (!b.image && !b.color && !b.label && !b.run && !b.builtin && !b.text_color)
  );
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
  if (b.run) act = "run";
  else if (b.builtin) act = "builtin";
  document.querySelectorAll('input[name="act"]').forEach((r) => {
    r.checked = r.value === act;
  });
  $("run").value = b.run || "";

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

function syncActionVisibility() {
  const act = document.querySelector('input[name="act"]:checked')?.value || "none";
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

  const act = document.querySelector('input[name="act"]:checked')?.value || "none";
  if (act === "run") {
    const run = $("run").value.trim();
    if (run) b.run = run;
  } else if (act === "builtin") {
    const sel = $("builtin").value;
    b.builtin = sel === "brightness_set" ? `brightness_set:${$("builtinValue").value}` : sel;
  }
  return b;
}

// Update the in-memory model whenever the form changes.
function onFormChange() {
  const b = readForm();
  if (isEmpty(b)) state.buttons.delete(state.selected);
  else state.buttons.set(state.selected, b);
  syncActionVisibility();
}

function collectConfig() {
  const buttons = [...state.buttons.values()]
    .filter((b) => !isEmpty(b))
    .sort((a, z) => a.key - z.key);
  return { brightness: state.brightness, buttons };
}

async function save() {
  const config = collectConfig();
  statusMsg("Saving…");
  try {
    const res = await fetch("/api/state", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(config),
    });
    if (res.ok) {
      statusMsg("Saved and applied to the device.", "ok");
      refreshAllPreviews();
    } else {
      const text = await res.text();
      statusMsg(`Could not save: ${text}`, "err");
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
  refreshPreview(state.selected);
}

async function load() {
  const res = await fetch("/api/state");
  const data = await res.json();
  state.model = data.model;
  state.brightness = data.brightness ?? 60;
  state.buttons = new Map();
  for (const b of data.buttons || []) state.buttons.set(b.key, b);

  $("model").textContent = data.model.name ? `· ${data.model.name}` : "";
  $("brightness").value = state.brightness;
  $("brightnessOut").textContent = `${state.brightness}%`;

  buildGrid();
  selectKey(0);
  statusMsg("Ready.", "ok");
}

function wire() {
  $("form").addEventListener("input", onFormChange);
  $("form").addEventListener("change", onFormChange);
  $("builtin").addEventListener("change", syncActionVisibility);
  $("save").addEventListener("click", save);
  $("clearKey").addEventListener("click", clearKey);
  $("brightness").addEventListener("input", onBrightness);
}

wire();
load().catch((err) => statusMsg(`Failed to load: ${err}`, "err"));
