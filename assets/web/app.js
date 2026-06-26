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

// ---- Fuzzy application search (combobox) ----
const MAX_APP_RESULTS = 30;

// Subsequence fuzzy score; -1 when not all query chars match in order.
function fuzzyScore(query, text) {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let qi = 0;
  let score = 0;
  let prev = -2;
  let ti = 0;
  for (; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      score += 1;
      if (ti === prev + 1) score += 4; // consecutive run
      if (ti === 0 || /[\s\-_./]/.test(t[ti - 1])) score += 3; // word boundary
      prev = ti;
      qi++;
    }
  }
  if (qi < q.length) return -1;
  return score - ti * 0.01; // gently prefer shorter/earlier matches
}

function appMatches(query) {
  const q = query.trim();
  if (!q) return state.apps.slice(0, MAX_APP_RESULTS);
  return state.apps
    .map((app) => ({ app, score: fuzzyScore(q, app.name) }))
    .filter((m) => m.score >= 0)
    .sort((a, b) => b.score - a.score)
    .slice(0, MAX_APP_RESULTS)
    .map((m) => m.app);
}

function renderAppResults() {
  const ul = $("appResults");
  ul.innerHTML = "";
  if (state.appMatches.length === 0) {
    const li = document.createElement("li");
    li.className = "combo__empty";
    li.textContent = "No matching applications";
    ul.appendChild(li);
    return;
  }
  state.appMatches.forEach((app, i) => {
    const li = document.createElement("li");
    li.className = "combo__option";
    li.setAttribute("role", "option");
    li.setAttribute("aria-selected", i === state.appActive ? "true" : "false");
    if (app.icon) {
      const img = document.createElement("img");
      img.className = "combo__icon";
      img.src = `/api/icon?path=${encodeURIComponent(app.icon)}`;
      img.alt = "";
      li.appendChild(img);
    }
    const span = document.createElement("span");
    span.className = "combo__name";
    span.textContent = app.name;
    li.appendChild(span);
    // mousedown (not click) so it fires before the input blur closes the list
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      chooseApp(app);
    });
    ul.appendChild(li);
  });
}

function openAppResults() {
  const ul = $("appResults");
  ul.hidden = false;
  $("appQuery").setAttribute("aria-expanded", "true");
}

function closeAppResults() {
  const ul = $("appResults");
  ul.hidden = true;
  state.appActive = -1;
  $("appQuery").setAttribute("aria-expanded", "false");
}

function chooseApp(app) {
  $("appCommand").value = app.command;
  $("appQuery").value = app.name;
  closeAppResults();
  if (!$("label").value.trim()) $("label").value = app.name;
  if (!$("image").value.trim() && app.icon) $("image").value = app.icon;
  onFormChange();
}

function onAppInput(e) {
  e.stopPropagation(); // don't let typing trigger a config apply
  state.appMatches = appMatches($("appQuery").value);
  state.appActive = state.appMatches.length ? 0 : -1;
  renderAppResults();
  openAppResults();
}

function onAppKeydown(e) {
  if ($("appResults").hidden && e.key !== "ArrowDown") return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if ($("appResults").hidden) onAppInput(e);
    const n = state.appMatches.length;
    if (!n) return;
    const dir = e.key === "ArrowDown" ? 1 : -1;
    state.appActive = (state.appActive + dir + n) % n;
    renderAppResults();
  } else if (e.key === "Enter") {
    if (state.appActive >= 0 && state.appMatches[state.appActive]) {
      e.preventDefault();
      chooseApp(state.appMatches[state.appActive]);
    }
  } else if (e.key === "Escape") {
    closeAppResults();
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
  if (act === "openapp") {
    $("appCommand").value = b.run || "";
    const found = state.apps.find((a) => a.command === b.run);
    $("appQuery").value = found ? found.name : b.run || "";
  } else {
    $("appCommand").value = "";
    $("appQuery").value = "";
  }
  closeAppResults();

  let builtin = "brightness_up";
  let bvalue = 70;
  let openTarget = "";
  if (b.builtin) {
    if (b.builtin.startsWith("brightness_set:") || b.builtin.startsWith("brightness:")) {
      builtin = "brightness_set";
      bvalue = parseInt(b.builtin.split(":")[1], 10) || 70;
    } else if (b.builtin.startsWith("open:")) {
      builtin = "open";
      openTarget = b.builtin.slice("open:".length);
    } else {
      builtin = b.builtin;
    }
  }
  $("builtin").value = builtin;
  $("builtinValue").value = bvalue;
  $("openTarget").value = openTarget;

  syncActionVisibility();
}

function currentAction() {
  return document.querySelector('input[name="act"]:checked')?.value || "none";
}

function syncActionVisibility() {
  const act = currentAction();
  $("appSearch").hidden = act !== "openapp";
  if (act !== "openapp") closeAppResults();
  $("run").hidden = act !== "run";
  $("builtinRow").hidden = act !== "builtin";
  $("builtinValue").hidden = !($("builtin").value === "brightness_set");
  $("openTarget").hidden = !($("builtin").value === "open");
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
    const cmd = $("appCommand").value;
    if (cmd) b.run = cmd;
  } else if (act === "run") {
    const run = $("run").value.trim();
    if (run) b.run = run;
  } else if (act === "builtin") {
    const sel = $("builtin").value;
    if (sel === "brightness_set") {
      b.builtin = `brightness_set:${$("builtinValue").value}`;
    } else if (sel === "open") {
      const target = $("openTarget").value.trim();
      if (target) b.builtin = `open:${target}`;
    } else {
      b.builtin = sel;
    }
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

  buildGrid();
  selectKey(0);
  statusMsg(`Ready \u2014 ${state.apps.length} apps. Edits apply live.`, "ok");
}

function wire() {
  $("form").addEventListener("input", onFormChange);
  $("form").addEventListener("change", onFormChange);
  $("builtin").addEventListener("change", syncActionVisibility);
  $("appQuery").addEventListener("input", onAppInput);
  $("appQuery").addEventListener("keydown", onAppKeydown);
  $("appQuery").addEventListener("focus", onAppInput);
  $("appQuery").addEventListener("blur", () => setTimeout(closeAppResults, 120));
  $("clearKey").addEventListener("click", clearKey);
  $("brightness").addEventListener("input", onBrightness);
}

wire();
load().catch((err) => statusMsg(`Failed to load: ${err}`, "err"));
