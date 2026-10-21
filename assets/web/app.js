"use strict";

const $ = (id) => document.getElementById(id);
const state = {
  model: { keyCount: 15, columns: 5, rows: 3, name: "" },
  brightness: 60,
  pages: [], // [{ name: string|null, buttons: Map<key, button> }]
  currentPage: 0,
  pressFeedback: true,
  apps: [],
  selected: 0,
  appActive: -1,
  appMatches: [],
  palette: [], // [{ name, hex, for }] from /api/v1/style
  iconActive: -1,
  iconMatches: [], // [{ name, tags }] from /api/v1/icons
  iconTotal: 0, // how many icons the latest search matched
  iconSearch: 0, // the latest search, so a slow answer never overwrites a newer one
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
    (!b.image &&
      !b.icon &&
      !b.color &&
      !b.label &&
      !b.run &&
      !b.builtin &&
      !b.hold &&
      !b.text_color &&
      !b.watch &&
      !(b.macro && b.macro.length) &&
      !(b.states && b.states.length))
  );
}

function hasVisual(b) {
  return !!(
    b &&
    (b.image || b.icon || b.color || b.label || b.watch || (b.states && b.states.length))
  );
}

// ---- Toggle states editor ----
function renderStates(states) {
  const list = $("statesList");
  list.innerHTML = "";
  (states && states.length ? states : [{ color: "#40a02b" }]).forEach((s) => addStateRow(s));
}

function addStateRow(s) {
  const row = document.createElement("div");
  row.className = "state-row";
  // Fields this row has no input for (icon, image, builtin, text_color) are kept.
  row.base = { ...(s || {}) };
  const label = document.createElement("input");
  label.type = "text";
  label.className = "state-label";
  label.placeholder = "label";
  label.value = (s && s.label) || "";
  const color = document.createElement("input");
  color.type = "color";
  color.className = "state-color";
  color.value = (s && s.color) || "#1e1e2e";
  const run = document.createElement("input");
  run.type = "text";
  run.className = "state-run";
  run.placeholder = "command (optional)";
  run.value = (s && s.run) || "";
  const del = document.createElement("button");
  del.type = "button";
  del.className = "state-del";
  del.textContent = "\u2715";
  del.title = "Remove state";
  del.addEventListener("click", () => {
    row.remove();
    onFormChange();
  });
  row.append(label, color, run, del);
  $("statesList").appendChild(row);
}

function collectStates() {
  return [...document.querySelectorAll("#statesList .state-row")].map((row) => {
    const state = { ...row.base, color: row.querySelector(".state-color").value };
    delete state.label;
    delete state.run;
    const label = row.querySelector(".state-label").value.trim();
    if (label) state.label = label;
    const run = row.querySelector(".state-run").value.trim();
    if (run) {
      state.run = run;
      delete state.builtin;
    }
    return state;
  });
}

function currentButtons() {
  return state.pages[state.currentPage].buttons;
}

function previewUrl(key) {
  return `/api/preview/${state.currentPage}/${key}.png?ts=${Date.now()}`;
}

function refreshPreview(key) {
  const cell = document.querySelector(`.key[data-key="${key}"]`);
  if (cell) cell.style.backgroundImage = `url(${previewUrl(key)})`;
}

function refreshAllPreviews() {
  for (let k = 0; k < state.model.keyCount; k++) refreshPreview(k);
}

function buildPageTabs() {
  const nav = $("pageTabs");
  nav.innerHTML = "";
  state.pages.forEach((p, i) => {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "tab";
    tab.setAttribute("aria-selected", i === state.currentPage ? "true" : "false");
    tab.textContent = p.name || `Page ${i + 1}`;
    tab.title = "Click to edit, double-click to rename";
    tab.addEventListener("click", () => switchPage(i));
    tab.addEventListener("dblclick", () => renamePage(i));
    nav.appendChild(tab);
  });
  const add = document.createElement("button");
  add.type = "button";
  add.className = "tab tab--ghost";
  add.textContent = "+";
  add.title = "Add page";
  add.addEventListener("click", addPage);
  nav.appendChild(add);
  if (state.pages.length > 1) {
    const del = document.createElement("button");
    del.type = "button";
    del.className = "tab tab--ghost";
    del.textContent = "\u2715";
    del.title = "Delete current page";
    del.addEventListener("click", deletePage);
    nav.appendChild(del);
  }
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

function switchPage(i) {
  state.currentPage = i;
  state.selected = 0;
  buildPageTabs();
  buildGrid();
  selectKey(0);
}

function addPage() {
  state.pages.push({ name: null, buttons: new Map() });
  switchPage(state.pages.length - 1);
  applyNow();
}

function renamePage(i) {
  const name = prompt("Page name", state.pages[i].name || "");
  if (name === null) return;
  state.pages[i].name = name.trim() || null;
  buildPageTabs();
  applyNow();
}

function deletePage() {
  if (state.pages.length <= 1) return;
  state.pages.splice(state.currentPage, 1);
  state.currentPage = Math.min(state.currentPage, state.pages.length - 1);
  switchPage(state.currentPage);
  applyNow();
}

function selectKey(key) {
  state.selected = key;
  document
    .querySelectorAll(".key")
    .forEach((c) =>
      c.setAttribute("aria-pressed", c.dataset.key === String(key) ? "true" : "false"),
    );
  $("selKey").textContent = String(key);
  populateForm(currentButtons().get(key) || { key });
}

function populateForm(b) {
  $("label").value = b.label || "";
  $("useText").checked = !!b.text_color;
  $("textColor").value = b.text_color || "#ffffff";
  $("useColor").checked = !!b.color;
  $("color").value = b.color || "#1e1e2e";
  $("image").value = b.image || "";
  $("icon").value = b.icon || "";
  $("iconQuery").value = b.icon || "";
  closeIconResults();
  $("watch").value = b.watch || "";
  $("interval").value = b.interval || 5;

  let act = "none";
  if (b.run && b.run.includes("gtk-launch ")) act = "openapp";
  else if (b.run) act = "run";
  else if (b.macro && b.macro.length) act = "macro";
  else if (b.states && b.states.length) act = "toggle";
  else if (b.hold) act = "hold";
  else if (b.builtin) act = "builtin";
  document.querySelectorAll('input[name="act"]').forEach((r) => {
    r.checked = r.value === act;
  });
  $("run").value = b.run || "";
  $("hold").value = b.hold || "";
  $("macro").value = (b.macro || []).join("\n");
  renderStates(b.states);
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
    } else if (b.builtin.startsWith("page:")) {
      builtin = "page";
      openTarget = b.builtin.slice("page:".length);
    } else {
      builtin = b.builtin;
    }
  }
  $("builtin").value = builtin;
  $("builtinValue").value = bvalue;
  $("openTarget").value = builtin === "page" ? "" : openTarget;
  $("pageTarget").value = builtin === "page" ? openTarget : "";

  syncActionVisibility();
  syncLook();
}

function currentAction() {
  return document.querySelector('input[name="act"]:checked')?.value || "none";
}

function syncActionVisibility() {
  const act = currentAction();
  $("appSearch").hidden = act !== "openapp";
  if (act !== "openapp") closeAppResults();
  $("run").hidden = act !== "run";
  $("hold").hidden = act !== "hold";
  $("macro").hidden = act !== "macro";
  $("statesEditor").hidden = act !== "toggle";
  $("builtinRow").hidden = act !== "builtin";
  const sel = $("builtin").value;
  $("builtinValue").hidden = !(sel === "brightness_set");
  $("openTarget").hidden = !(sel === "open");
  $("pageTarget").hidden = !(sel === "page");
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
  const icon = $("icon").value;
  if (icon) b.icon = icon;
  const watch = $("watch").value.trim();
  if (watch) {
    b.watch = watch;
    const iv = parseInt($("interval").value, 10);
    if (iv >= 1) b.interval = iv;
  }

  const act = currentAction();
  if (act === "openapp") {
    const cmd = $("appCommand").value;
    if (cmd) b.run = cmd;
  } else if (act === "run") {
    const run = $("run").value.trim();
    if (run) b.run = run;
  } else if (act === "hold") {
    const hold = $("hold").value.trim();
    if (hold) b.hold = hold;
  } else if (act === "macro") {
    const steps = $("macro")
      .value.split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    if (steps.length) b.macro = steps;
  } else if (act === "toggle") {
    const states = collectStates();
    if (states.length) b.states = states;
  } else if (act === "builtin") {
    const sel = $("builtin").value;
    if (sel === "brightness_set") {
      b.builtin = `brightness_set:${$("builtinValue").value}`;
    } else if (sel === "open") {
      const target = $("openTarget").value.trim();
      if (target) b.builtin = `open:${target}`;
    } else if (sel === "page") {
      const target = $("pageTarget").value.trim();
      if (target) b.builtin = `page:${target}`;
    } else {
      b.builtin = sel;
    }
  }
  return b;
}

function onFormChange() {
  const b = readForm();
  if (isEmpty(b)) currentButtons().delete(state.selected);
  else currentButtons().set(state.selected, b);
  syncActionVisibility();
  syncLook();
  scheduleApply();
}

// ---- Icon picker (Lucide, searched on the daemon) and palette swatches ----
const ICON_SEARCH_DELAY_MS = 120;

function glyphUrl(name) {
  return `url("/api/lucide/${encodeURIComponent(name)}.svg")`;
}

function syncLook() {
  const icon = $("icon").value;
  const preview = $("iconPreview");
  preview.style.backgroundColor = $("useColor").checked ? $("color").value : "#000000";
  preview.style.color = $("useText").checked ? $("textColor").value : "#ffffff";
  preview.querySelector(".iconpick__glyph").style.setProperty("--glyph", icon ? glyphUrl(icon) : "none");
  preview.dataset.empty = icon ? "false" : "true";
  $("iconClear").disabled = !icon;
  const color = $("useColor").checked ? $("color").value.toLowerCase() : "";
  document.querySelectorAll("#swatches .swatch").forEach((swatch) => {
    swatch.setAttribute("aria-pressed", swatch.dataset.hex === color ? "true" : "false");
  });
}

function buildSwatches() {
  const group = $("swatches");
  group.innerHTML = "";
  state.palette.forEach((swatch) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "swatch";
    button.dataset.hex = swatch.hex.toLowerCase();
    button.style.backgroundColor = swatch.hex;
    button.textContent = swatch.name;
    button.title = `${swatch.name} ${swatch.hex}: ${swatch.for}`;
    button.setAttribute("aria-label", `${swatch.name}, for ${swatch.for}`);
    button.addEventListener("click", () => {
      $("useColor").checked = true;
      $("color").value = swatch.hex.toLowerCase();
      onFormChange();
    });
    group.appendChild(button);
  });
  group.hidden = state.palette.length === 0;
}

async function searchIcons(query) {
  const search = ++state.iconSearch;
  try {
    const res = await fetch(`/api/v1/icons?q=${encodeURIComponent(query)}`);
    if (!res.ok) throw new Error(`${res.status} ${await res.text()}`);
    const found = await res.json();
    if (search !== state.iconSearch) return;
    state.iconMatches = found.icons;
    state.iconTotal = found.total;
    state.iconActive = found.icons.length ? 0 : -1;
    renderIconResults();
    openIconResults();
  } catch (err) {
    if (search === state.iconSearch) statusMsg(`Icon search failed: ${err}`, "err");
  }
}

function renderIconResults() {
  const total = state.iconTotal;
  const ul = $("iconResults");
  ul.innerHTML = "";
  if (state.iconMatches.length === 0) {
    const li = document.createElement("li");
    li.className = "combo__empty";
    li.textContent = "No matching icons";
    ul.appendChild(li);
    return;
  }
  state.iconMatches.forEach((icon, i) => {
    const li = document.createElement("li");
    li.className = "combo__option";
    li.id = `icon-option-${i}`;
    li.setAttribute("role", "option");
    li.setAttribute("aria-selected", i === state.iconActive ? "true" : "false");
    li.title = icon.tags.join(", ");
    const glyph = document.createElement("span");
    glyph.className = "combo__glyph";
    glyph.style.setProperty("--glyph", glyphUrl(icon.name));
    const name = document.createElement("span");
    name.className = "combo__name";
    name.textContent = icon.name;
    li.append(glyph, name);
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      chooseIcon(icon.name);
    });
    ul.appendChild(li);
  });
  if (total > state.iconMatches.length) {
    const li = document.createElement("li");
    li.className = "combo__empty";
    li.textContent = `${total - state.iconMatches.length} more: add words to narrow`;
    ul.appendChild(li);
  }
  const active = state.iconActive >= 0 ? `icon-option-${state.iconActive}` : "";
  $("iconQuery").setAttribute("aria-activedescendant", active);
}

function openIconResults() {
  $("iconResults").hidden = false;
  $("iconQuery").setAttribute("aria-expanded", "true");
}

function closeIconResults() {
  $("iconResults").hidden = true;
  state.iconActive = -1;
  $("iconQuery").setAttribute("aria-expanded", "false");
  $("iconQuery").removeAttribute("aria-activedescendant");
}

function chooseIcon(name) {
  $("icon").value = name;
  $("iconQuery").value = name;
  closeIconResults();
  if ($("image").value.trim()) {
    $("image").value = "";
    statusMsg("The icon replaces the image: a key shows one picture.", "ok");
  }
  onFormChange();
}

function clearIcon() {
  $("icon").value = "";
  $("iconQuery").value = "";
  closeIconResults();
  onFormChange();
}

let iconTimer = null;
function onIconInput(e) {
  e.stopPropagation();
  clearTimeout(iconTimer);
  const query = $("iconQuery").value;
  iconTimer = setTimeout(() => searchIcons(query), ICON_SEARCH_DELAY_MS);
}

function onIconBlur() {
  setTimeout(() => {
    closeIconResults();
    // Typing only searches; the chosen icon changes on a pick or on emptying the field.
    if (!$("iconQuery").value.trim() && $("icon").value) clearIcon();
    else $("iconQuery").value = $("icon").value;
  }, 120);
}

function onIconKeydown(e) {
  if ($("iconResults").hidden && e.key !== "ArrowDown") return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if ($("iconResults").hidden) {
      searchIcons($("iconQuery").value);
      return;
    }
    const n = state.iconMatches.length;
    if (!n) return;
    const dir = e.key === "ArrowDown" ? 1 : -1;
    state.iconActive = (state.iconActive + dir + n) % n;
    renderIconResults();
    document.getElementById(`icon-option-${state.iconActive}`)?.scrollIntoView({ block: "nearest" });
  } else if (e.key === "Enter") {
    if (state.iconActive >= 0 && state.iconMatches[state.iconActive]) {
      e.preventDefault();
      chooseIcon(state.iconMatches[state.iconActive].name);
    }
  } else if (e.key === "Escape") {
    closeIconResults();
  }
}

// ---- Fuzzy application search (combobox) ----
const MAX_APP_RESULTS = 30;

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
      if (ti === prev + 1) score += 4;
      if (ti === 0 || /[\s\-_./]/.test(t[ti - 1])) score += 3;
      prev = ti;
      qi++;
    }
  }
  if (qi < q.length) return -1;
  return score - ti * 0.01;
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
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      chooseApp(app);
    });
    ul.appendChild(li);
  });
}

function openAppResults() {
  $("appResults").hidden = false;
  $("appQuery").setAttribute("aria-expanded", "true");
}

function closeAppResults() {
  $("appResults").hidden = true;
  state.appActive = -1;
  $("appQuery").setAttribute("aria-expanded", "false");
}

function chooseApp(app) {
  $("appCommand").value = app.command;
  $("appQuery").value = app.name;
  closeAppResults();
  if (!$("label").value.trim()) $("label").value = app.name;
  if (!$("image").value.trim() && !$("icon").value && app.icon) $("image").value = app.icon;
  onFormChange();
}

function onAppInput(e) {
  e.stopPropagation();
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

function pageToObj(p) {
  const buttons = [...p.buttons.values()].filter(hasVisual).sort((a, z) => a.key - z.key);
  const obj = { buttons };
  if (p.name) obj.name = p.name;
  return obj;
}

function collectConfig() {
  const cfg = { brightness: state.brightness };
  if (!state.pressFeedback) cfg.press_feedback = false;
  if (state.pages.length === 1 && !state.pages[0].name) {
    cfg.buttons = [...state.pages[0].buttons.values()]
      .filter(hasVisual)
      .sort((a, z) => a.key - z.key);
  } else {
    cfg.pages = state.pages.map(pageToObj);
  }
  return cfg;
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
  currentButtons().delete(state.selected);
  populateForm({ key: state.selected });
  applyNow();
}

async function load() {
  const [stateRes, appsRes, style] = await Promise.all([
    fetch("/api/state"),
    fetch("/api/apps"),
    fetch("/api/v1/style")
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error(`${res.status}`))))
      .catch((err) => {
        console.error(`palette unavailable: ${err}`);
        return { palette: [] };
      }),
  ]);
  const data = await stateRes.json();
  state.apps = await appsRes.json().catch(() => []);
  state.palette = style.palette;
  buildSwatches();
  state.model = data.model;
  state.brightness = data.brightness ?? 60;
  state.pressFeedback = data.press_feedback !== false;
  $("pressFb").checked = state.pressFeedback;
  const pages = data.pages && data.pages.length ? data.pages : [{ name: null, buttons: [] }];
  state.pages = pages.map((p) => ({
    name: p.name || null,
    buttons: new Map((p.buttons || []).map((b) => [b.key, b])),
  }));
  state.currentPage = 0;

  $("model").textContent = data.model.name ? `\u00b7 ${data.model.name}` : "";
  $("brightness").value = state.brightness;
  $("brightnessOut").textContent = `${state.brightness}%`;

  buildPageTabs();
  buildGrid();
  selectKey(0);
  statusMsg(`Ready \u2014 ${state.apps.length} apps, ${state.pages.length} page(s).`, "ok");
}

function wire() {
  $("form").addEventListener("input", onFormChange);
  $("form").addEventListener("change", onFormChange);
  $("builtin").addEventListener("change", syncActionVisibility);
  $("appQuery").addEventListener("input", onAppInput);
  $("appQuery").addEventListener("keydown", onAppKeydown);
  $("appQuery").addEventListener("focus", onAppInput);
  $("appQuery").addEventListener("blur", () => setTimeout(closeAppResults, 120));
  $("iconQuery").addEventListener("input", onIconInput);
  $("iconQuery").addEventListener("keydown", onIconKeydown);
  $("iconQuery").addEventListener("focus", () => searchIcons($("iconQuery").value));
  $("iconQuery").addEventListener("blur", onIconBlur);
  $("iconClear").addEventListener("click", clearIcon);
  $("clearKey").addEventListener("click", clearKey);
  $("addState").addEventListener("click", () => {
    addStateRow({ color: "#40a02b" });
    onFormChange();
  });
  $("brightness").addEventListener("input", onBrightness);
  $("pressFb").addEventListener("change", () => {
    state.pressFeedback = $("pressFb").checked;
    applyNow();
  });
}

wire();
load().catch((err) => statusMsg(`Failed to load: ${err}`, "err"));
