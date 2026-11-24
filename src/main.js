const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const $ = (id) => document.getElementById(id);
const THEME_KEY = "chronoring-theme";

const els = {
  newBtn: $("new-btn"),
  emptyNewBtn: $("empty-new-btn"),
  upcoming: $("upcoming"),
  past: $("past"),
  upcomingSection: $("upcoming-section"),
  pastSection: $("past-section"),
  emptyState: $("empty-state"),
  loadError: $("load-error"),
  backdrop: $("composer-backdrop"),
  form: $("composer"),
  title: $("composer-title"),
  note: $("note"),
  date: $("date"),
  hour: $("hour"),
  minute: $("minute"),
  meridiemButtons: document.querySelectorAll("[data-meridiem]"),
  themeButtons: document.querySelectorAll("[data-theme-choice]"),
  winMin: $("win-min"),
  winMax: $("win-max"),
  winClose: $("win-close"),
  clash: $("clash-hint"),
  error: $("composer-error"),
  save: $("save-btn"),
  cancel: $("cancel-btn"),
  close: $("close-btn"),
};

let alarms = [];
let meridiem = "am";
let editingId = null; // null means we are creating a new reminder
let lastFocused = null;
let firstRender = true;
const seenIds = new Set();

const STATUS_LABEL = { waiting: "Scheduled", ringing: "Ringing", dismissed: "Done" };

/* Window controls */

// Keeps the maximize button's icon and label in step with the window state.
async function syncMaximizeButton() {
  const maximized = await appWindow.isMaximized();
  els.winMax.dataset.maximized = String(maximized);
  els.winMax.setAttribute("aria-label", maximized ? "Restore" : "Maximize");
  els.winMax.title = maximized ? "Restore" : "Maximize";
}

/* Theme */

function applyTheme(choice) {
  document.documentElement.dataset.theme = choice;
  localStorage.setItem(THEME_KEY, choice);
  els.themeButtons.forEach((btn) => {
    btn.setAttribute("aria-pressed", String(btn.dataset.themeChoice === choice));
  });
}

/* Helpers */

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function action(label, className, onClick) {
  const btn = el("button", className, label);
  btn.type = "button";
  btn.addEventListener("click", onClick);
  return btn;
}

function pad(n) {
  return String(n).padStart(2, "0");
}

function toDateInput(d) {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function relativeTime(fireAt) {
  const diff = fireAt - Math.floor(Date.now() / 1000);
  const mins = Math.round(Math.abs(diff) / 60);

  let text;
  if (mins < 1) {
    text = "under a minute";
  } else if (mins < 60) {
    text = `${mins} min`;
  } else {
    const h = Math.floor(mins / 60);
    const m = mins % 60;
    text = m ? `${h} h ${m} min` : `${h} h`;
  }
  return diff >= 0 ? `in ${text}` : `${text} ago`;
}

/* Rendering */

function buildCard(alarm) {
  const isNew = !firstRender && !seenIds.has(alarm.id);
  const li = el("li", `card card-${alarm.status}${isNew ? " card-enter" : ""}`);
  const body = el("div", "card-body");

  body.append(el("p", "card-note", alarm.note));

  const meta = el("div", "card-meta");
  meta.append(el("span", "card-time", alarm.display_time));
  meta.append(el("span", `pill pill-${alarm.status}`, STATUS_LABEL[alarm.status]));
  if (alarm.status !== "dismissed") {
    const rel = el("span", "card-relative", relativeTime(alarm.fire_at));
    rel.dataset.fireAt = String(alarm.fire_at);
    meta.append(rel);
  }
  body.append(meta);

  const actions = el("div", "card-actions");

  if (alarm.status === "ringing") {
    actions.append(
      action("Stop", "stop-btn", () => run(() => invoke("dismiss_alarm", { id: alarm.id })))
    );
  } else if (alarm.status === "waiting") {
    actions.append(action("Edit", "ghost small", () => openComposer(alarm)));
    actions.append(
      action("Dismiss", "ghost small", () => run(() => invoke("dismiss_alarm", { id: alarm.id })))
    );
  } else {
    // Finished reminders can be brought back with a new time.
    actions.append(action("Reschedule", "ghost small", () => openComposer(alarm)));
  }

  actions.append(
    action("Delete", "ghost small danger", () => {
      if (!window.confirm(`Delete this reminder?\n\n"${alarm.note}"`)) return;
      run(() => invoke("delete_alarm", { id: alarm.id }));
    })
  );

  li.append(body, actions);
  return li;
}

function render() {
  const active = alarms.filter((a) => a.status !== "dismissed");
  const done = alarms.filter((a) => a.status === "dismissed");

  // Ringing alarms go to the top, then the rest by time.
  active.sort((a, b) => {
    if (a.status === "ringing" && b.status !== "ringing") return -1;
    if (b.status === "ringing" && a.status !== "ringing") return 1;
    return a.fire_at - b.fire_at;
  });

  els.upcoming.replaceChildren(...active.map(buildCard));
  els.past.replaceChildren(...done.map(buildCard));

  els.upcomingSection.hidden = active.length === 0;
  els.pastSection.hidden = done.length === 0;
  els.emptyState.hidden = alarms.length > 0;
  els.newBtn.hidden = alarms.length === 0;

  alarms.forEach((a) => seenIds.add(a.id));
  firstRender = false;
}

// Updates only the "in 5 min" labels, without rebuilding the list.
function updateRelativeTimes() {
  document.querySelectorAll("[data-fire-at]").forEach((node) => {
    node.textContent = relativeTime(Number(node.dataset.fireAt));
  });
}

function showLoadError(message) {
  els.loadError.textContent = message;
  els.loadError.hidden = false;
}

/* Data */

async function refresh() {
  try {
    alarms = await invoke("list_alarms");
    els.loadError.hidden = true;
    render();
  } catch (err) {
    showLoadError(`Could not load reminders: ${err}`);
  }
}

// Runs an action that changes data, then reloads the list.
async function run(action) {
  try {
    await action();
    await refresh();
  } catch (err) {
    showLoadError(String(err));
  }
}

/* Composer */

function fillTimeOptions() {
  for (let h = 1; h <= 12; h++) {
    els.hour.append(new Option(String(h), String(h)));
  }
  for (let m = 0; m < 60; m++) {
    els.minute.append(new Option(pad(m), String(m)));
  }
}

// Default to one hour from now, rounded up to the next 5 minutes.
function defaultDate() {
  const d = new Date(Date.now() + 60 * 60 * 1000);
  d.setMinutes(Math.ceil(d.getMinutes() / 5) * 5, 0, 0);
  return d;
}

// Fills the form's date and time fields from a JavaScript Date.
function fillWhen(d) {
  const h24 = d.getHours();
  els.date.value = toDateInput(d);
  els.hour.value = String(h24 % 12 === 0 ? 12 : h24 % 12);
  els.minute.value = String(d.getMinutes());
  setMeridiem(h24 >= 12 ? "pm" : "am");
}

function setMeridiem(value) {
  meridiem = value;
  els.meridiemButtons.forEach((btn) => {
    btn.setAttribute("aria-pressed", String(btn.dataset.meridiem === value));
  });
  updateClashHint();
}

// The form's current time as a Unix timestamp, in local time.
function formTimestamp() {
  const [y, m, d] = els.date.value.split("-").map(Number);
  const h = (Number(els.hour.value) % 12) + (meridiem === "pm" ? 12 : 0);
  return Math.floor(new Date(y, m - 1, d, h, Number(els.minute.value)).getTime() / 1000);
}

// Warns, but does not block, when another reminder has the same time.
function updateClashHint() {
  if (!els.date.value) {
    els.clash.hidden = true;
    return;
  }
  const ts = formTimestamp();
  const other = alarms.find(
    (a) => a.status !== "dismissed" && a.id !== editingId && a.fire_at === ts
  );
  els.clash.textContent = other
    ? `Heads up: "${other.note}" is set for this same time. Both will ring.`
    : "";
  els.clash.hidden = !other;
}

function showComposerError(message) {
  els.error.textContent = message;
  els.error.hidden = false;
}

// Opens the form. Pass an alarm to edit or reschedule it.
function openComposer(alarm = null) {
  lastFocused = document.activeElement;
  editingId = alarm ? alarm.id : null;

  els.title.textContent = alarm ? "Edit reminder" : "New reminder";
  els.save.textContent = alarm ? "Save changes" : "Save reminder";
  els.error.hidden = true;

  if (alarm) {
    els.note.value = alarm.note;
    fillWhen(new Date(alarm.fire_at * 1000));
  } else {
    els.note.value = "";
    fillWhen(defaultDate());
  }

  els.backdrop.hidden = false;
  els.note.focus();
}

function closeComposer() {
  if (els.backdrop.hidden) return;
  els.backdrop.hidden = true;
  if (lastFocused && typeof lastFocused.focus === "function") {
    lastFocused.focus();
  }
}

// Keeps Tab and Shift+Tab inside the open form.
function trapFocus(event) {
  const focusables = [
    ...els.form.querySelectorAll("button:not([disabled]), input, select, textarea"),
  ];
  const first = focusables[0];
  const last = focusables[focusables.length - 1];

  if (!els.form.contains(document.activeElement)) {
    event.preventDefault();
    first.focus();
  } else if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

async function saveAlarm(event) {
  event.preventDefault();

  const note = els.note.value.trim();
  if (!note) return showComposerError("Write something to remember.");
  if (!els.date.value) return showComposerError("Pick a date.");

  const [year, month, day] = els.date.value.split("-").map(Number);
  const when = {
    year,
    month,
    day,
    hour: Number(els.hour.value),
    minute: Number(els.minute.value),
    meridiem,
  };

  els.save.disabled = true;
  els.save.textContent = "Saving…";
  try {
    if (editingId === null) {
      await invoke("add_alarm", { note, when });
    } else {
      await invoke("update_alarm", { id: editingId, note, when });
    }
    closeComposer();
    await refresh();
  } catch (err) {
    showComposerError(String(err));
  } finally {
    els.save.disabled = false;
    els.save.textContent = editingId === null ? "Save reminder" : "Save changes";
  }
}

/* Wiring */

fillTimeOptions();
applyTheme(localStorage.getItem(THEME_KEY) || "system");

els.winMin.addEventListener("click", () => appWindow.minimize());
els.winMax.addEventListener("click", () => appWindow.toggleMaximize());
els.winClose.addEventListener("click", () => appWindow.close());
appWindow.onResized(syncMaximizeButton);
syncMaximizeButton();

els.themeButtons.forEach((btn) =>
  btn.addEventListener("click", () => applyTheme(btn.dataset.themeChoice))
);

els.newBtn.addEventListener("click", () => openComposer());
els.emptyNewBtn.addEventListener("click", () => openComposer());
els.cancel.addEventListener("click", closeComposer);
els.close.addEventListener("click", closeComposer);
els.form.addEventListener("submit", saveAlarm);

els.meridiemButtons.forEach((btn) =>
  btn.addEventListener("click", () => setMeridiem(btn.dataset.meridiem))
);
[els.date, els.hour, els.minute].forEach((input) =>
  input.addEventListener("change", updateClashHint)
);

// Clicking the dimmed background closes the composer, but not clicks inside it.
els.backdrop.addEventListener("mousedown", (event) => {
  if (event.target === els.backdrop) closeComposer();
});

els.note.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
    els.form.requestSubmit();
  }
});

document.addEventListener("keydown", (event) => {
  const composerOpen = !els.backdrop.hidden;

  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") {
    event.preventDefault();
    openComposer();
    return;
  }
  if (composerOpen && event.key === "Tab") {
    trapFocus(event);
    return;
  }
  if (composerOpen && event.key === "Escape") {
    closeComposer();
  }
});

// The Rust checker tells us the moment an alarm starts ringing, so the list
// updates immediately instead of waiting for the next poll.
listen("alarms-ringing", () => refresh());

refresh();
setInterval(refresh, 5000);
setInterval(updateRelativeTimes, 30000);