const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const $ = (id) => document.getElementById(id);
const THEME_KEY = "chronoring-theme";
const NOTICE_KEY = "chronoring-startup-notice-seen";

const els = {
  newBtn: $("new-btn"),
  emptyNewBtn: $("empty-new-btn"),
  upcoming: $("upcoming"),
  past: $("past"),
  upcomingSection: $("upcoming-section"),
  pastSection: $("past-section"),
  emptyState: $("empty-state"),
  loadError: $("load-error"),
  startupNotice: $("startup-notice"),
  noticeDismiss: $("notice-dismiss"),
  backdrop: $("composer-backdrop"),
  form: $("composer"),
  title: $("composer-title"),
  note: $("note"),
  dateSummary: $("date-summary"),
  dateChips: document.querySelectorAll("[data-day]"),
  otherDay: $("other-day"),
  calendarWrap: $("calendar-wrap"),
  calPrev: $("cal-prev"),
  calNext: $("cal-next"),
  calMonth: $("cal-month"),
  calGrid: $("cal-grid"),
  hourGrid: $("hour-grid"),
  minuteGrid: $("minute-grid"),
  timeSummary: $("time-summary"),
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
  confirmBackdrop: $("confirm-backdrop"),
  confirmCard: document.querySelector(".confirm"),
  confirmText: $("confirm-text"),
  confirmKeep: $("confirm-keep"),
  confirmDelete: $("confirm-delete"),
};

let alarms = [];
let editingId = null; // null means we are creating a new reminder
let lastFocused = null;
let firstRender = true;
const seenIds = new Set();

// The form's date and time. The date is kept as "YYYY-MM-DD" text.
let formDate = "";
// Where the chosen date came from: "quick" for Today/Tomorrow, "other" for the calendar or an edit
let dateSource = "quick";
let calendarOpen = false; // the month calendar is hidden unless needed
let viewYear = 0; // the month the calendar is showing
let viewMonth = 0;
let hour = 12; // 1 to 12
let minute = 0; // 0 to 59
let meridiem = "am";

// The pending answer of the open confirm card, and the element to return focus to.
let confirmResolve = null;
let confirmReturnFocus = null;

// Each hour and minute the grids offer. A minute outside this list only appears when editing.
const HOURS = Array.from({ length: 12 }, (_, i) => i + 1);
const MINUTES = Array.from({ length: 12 }, (_, i) => i * 5);
const STATUS_LABEL = { waiting: "Scheduled", ringing: "Ringing", dismissed: "Done" };
const WEEKDAYS = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

// The minute chip for a minute that is not a multiple of five. Built once, shown on demand.
let offGridMinute = null;

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

/* Startup notice */

// Shows the one-time notice unless it was dismissed on this PC before.
function setupStartupNotice() {
  if (localStorage.getItem(NOTICE_KEY)) return;

  els.startupNotice.hidden = false;
  els.noticeDismiss.addEventListener("click", () => {
    localStorage.setItem(NOTICE_KEY, "1");
    els.startupNotice.hidden = true;
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

// Reads "YYYY-MM-DD" as a local date at midnight.
function parseDateInput(text) {
  const [y, m, d] = text.split("-").map(Number);
  return new Date(y, m - 1, d);
}

function startOfToday() {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d;
}

// The date a given number of days from today, as "YYYY-MM-DD".
function dateOffsetFromToday(days) {
  const d = startOfToday();
  d.setDate(d.getDate() + days);
  return toDateInput(d);
}

function isQuickDay(dateKey) {
  return dateKey === dateOffsetFromToday(0) || dateKey === dateOffsetFromToday(1);
}

function formatDate(text) {
  return new Intl.DateTimeFormat("en-GB", {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  }).format(parseDateInput(text));
}

/* Relative time */

// The calendar gap between two dates, split into years, months, days, hours and minutes.
// Months are counted by the calendar, so a 31-day month and a 28-day February both work.
function calendarGap(from, to) {
  let years = to.getFullYear() - from.getFullYear();
  let months = to.getMonth() - from.getMonth();
  let days = to.getDate() - from.getDate();
  let hours = to.getHours() - from.getHours();
  let minutes = to.getMinutes() - from.getMinutes();

  // Borrow from the larger unit whenever a smaller one goes negative.
  if (minutes < 0) {
    minutes += 60;
    hours -= 1;
  }
  if (hours < 0) {
    hours += 24;
    days -= 1;
  }
  if (days < 0) {
    // Borrow the length of the month before the end date's month.
    days += new Date(to.getFullYear(), to.getMonth(), 0).getDate();
    months -= 1;
  }
  if (months < 0) {
    months += 12;
    years -= 1;
  }

  return [
    ["yr", years],
    ["mo", months],
    ["d", days],
    ["h", hours],
    ["min", minutes],
  ].filter(([, value]) => value > 0);
}

// "in 1 yr 2 mo 3 d", "in 4 h 5 min", "12 min ago". Shows the three largest units.
function relativeTime(fireAt) {
  const now = new Date();
  const target = new Date(fireAt * 1000);
  const future = target >= now;

  const parts = future ? calendarGap(now, target) : calendarGap(target, now);
  const text = parts.length
    ? parts.slice(0, 3).map(([label, value]) => `${value} ${label}`).join(" ")
    : "under a minute";

  return future ? `in ${text}` : `${text} ago`;
}

// Keeps Tab and Shift+Tab inside a dialog while it is open.
function trapFocus(event, container) {
  const focusables = [...container.querySelectorAll("button:not([disabled]), input, select, textarea")];
  const first = focusables[0];
  const last = focusables[focusables.length - 1];

  if (!container.contains(document.activeElement)) {
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

/* Confirm card */

// Shows the in-app confirmation and resolves to true (Delete) or false (Keep).
// Calling it again while a confirmation is open cancels the old one first.
function askConfirm(message) {
  settleConfirm(false);

  confirmReturnFocus = document.activeElement;
  els.confirmText.textContent = message;
  els.confirmBackdrop.hidden = false;
  els.confirmKeep.focus();

  return new Promise((resolve) => {
    confirmResolve = resolve;
  });
}

// Closes the confirm card and resolves the pending question, once.
function settleConfirm(result) {
  if (!confirmResolve) return;

  const resolve = confirmResolve;
  confirmResolve = null;
  els.confirmBackdrop.hidden = true;
  resolve(result);

  if (confirmReturnFocus && typeof confirmReturnFocus.focus === "function") {
    confirmReturnFocus.focus();
  }
  confirmReturnFocus = null;
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
    action("Delete", "ghost small danger", async () => {
      const confirmed = await askConfirm(`"${alarm.note}" will be removed for good.`);
      if (!confirmed) return;
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

// Updates only the relative labels, without rebuilding the list.
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

/* Composer: date and time */

// Default to one hour from now, rounded up to the next 5 minutes.
function defaultDate() {
  const d = new Date(Date.now() + 60 * 60 * 1000);
  d.setMinutes(Math.ceil(d.getMinutes() / 5) * 5, 0, 0);
  return d;
}

// Makes the calendar show the month of the given date key.
function showMonthOf(dateKey) {
  const d = parseDateInput(dateKey);
  viewYear = d.getFullYear();
  viewMonth = d.getMonth();
}

// Chooses a day, then redraws. The calendar and the quick chips both use this.
function setFormDate(dateKey) {
  formDate = dateKey;
  showMonthOf(dateKey);
  renderWhen();
}

// Today or Tomorrow. Closes the calendar, since the quick chips cover these days.
function chooseQuickDay(days) {
  calendarOpen = false;
  dateSource = "quick";
  setFormDate(dateOffsetFromToday(days));
}

function toggleCalendar() {
  calendarOpen = !calendarOpen;
  renderWhen();
}

// Moves the calendar one month forward or back, without changing the chosen day.
function shiftMonth(delta) {
  const d = new Date(viewYear, viewMonth + delta, 1);
  viewYear = d.getFullYear();
  viewMonth = d.getMonth();
  renderCalendar();
}

// Copies a JavaScript Date into the form's state. Opens the calendar only for other days.
function fillWhen(d) {
  const h24 = d.getHours();
  formDate = toDateInput(d);
  showMonthOf(formDate);
  calendarOpen = !isQuickDay(formDate);
  dateSource = isQuickDay(formDate) ? "quick" : "other";
  hour = h24 % 12 === 0 ? 12 : h24 % 12;
  minute = d.getMinutes();
  meridiem = h24 >= 12 ? "pm" : "am";
  renderWhen();
}

function setHour(value) {
  hour = value;
  renderWhen();
}

function setMinute(value) {
  minute = value;
  renderWhen();
}

function setMeridiem(value) {
  meridiem = value;
  renderWhen();
}

// The form's current time as a Unix timestamp, in local time.
function formTimestamp() {
  const day = parseDateInput(formDate);
  const h = (hour % 12) + (meridiem === "pm" ? 12 : 0);
  return Math.floor(new Date(day.getFullYear(), day.getMonth(), day.getDate(), h, minute).getTime() / 1000);
}

function numChip(label, value) {
  const btn = el("button", "num-chip", label);
  btn.type = "button";
  btn.dataset.value = String(value);
  btn.setAttribute("aria-pressed", "false");
  return btn;
}

// Builds the hour and minute chips once. Only their pressed state changes later.
function buildTimeGrids() {
  const hourButtons = HOURS.map((value) => {
    const btn = numChip(String(value), value);
    btn.addEventListener("click", () => setHour(value));
    return btn;
  });
  els.hourGrid.replaceChildren(...hourButtons);

  const minuteButtons = MINUTES.map((value) => {
    const btn = numChip(pad(value), value);
    btn.addEventListener("click", () => setMinute(value));
    return btn;
  });

  offGridMinute = numChip("", 0);
  offGridMinute.hidden = true;
  offGridMinute.addEventListener("click", () => setMinute(Number(offGridMinute.dataset.value)));
  minuteButtons.push(offGridMinute);

  els.minuteGrid.replaceChildren(...minuteButtons);
}

// Draws the hour and minute chips and the AM/PM choice from the current state.
function renderTime() {
  els.hourGrid.querySelectorAll(".num-chip").forEach((btn) => {
    btn.setAttribute("aria-pressed", String(Number(btn.dataset.value) === hour));
  });

  const isOffGrid = minute % 5 !== 0;
  offGridMinute.hidden = !isOffGrid;
  offGridMinute.textContent = pad(minute);
  offGridMinute.dataset.value = String(minute);

  els.minuteGrid.querySelectorAll(".num-chip").forEach((btn) => {
    btn.setAttribute("aria-pressed", String(Number(btn.dataset.value) === minute));
  });

  els.meridiemButtons.forEach((btn) => {
    btn.setAttribute("aria-pressed", String(btn.dataset.meridiem === meridiem));
  });

  els.timeSummary.textContent = `${hour}:${pad(minute)} ${meridiem.toUpperCase()}`;
}

// Draws the month calendar. Past days are disabled, so a reminder can't go backwards.
function renderCalendar() {
  const first = new Date(viewYear, viewMonth, 1);
  const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate();
  const leadingBlanks = (first.getDay() + 6) % 7; // weeks start on Monday
  const today = startOfToday();
  const todayKey = toDateInput(today);

  const cells = WEEKDAYS.map((name) => el("span", "cal-weekday", name));
  for (let i = 0; i < leadingBlanks; i++) {
    cells.push(el("span", "cal-blank"));
  }

  for (let day = 1; day <= daysInMonth; day++) {
    const date = new Date(viewYear, viewMonth, day);
    const key = toDateInput(date);
    const btn = el("button", "cal-day", String(day));
    btn.type = "button";
    btn.disabled = date < today;
    btn.classList.toggle("is-today", key === todayKey);
    btn.setAttribute("aria-pressed", String(key === formDate));
    btn.addEventListener("click", () => {
      dateSource = "other";
      setFormDate(key);
    });
    cells.push(btn);
  }

  els.calGrid.replaceChildren(...cells);
  els.calMonth.textContent = new Intl.DateTimeFormat("en-GB", {
    month: "long",
    year: "numeric",
  }).format(first);
}

// Draws every part of the date and time controls from the current state.
function renderWhen() {
  els.dateSummary.textContent = formatDate(formDate);

  els.dateChips.forEach((chip) => {
    const isPressed =
      dateSource === "quick" && formDate === dateOffsetFromToday(Number(chip.dataset.day));
    chip.setAttribute("aria-pressed", String(isPressed));
  });

  els.otherDay.setAttribute("aria-expanded", String(calendarOpen));
  els.calendarWrap.hidden = !calendarOpen;
  if (calendarOpen) renderCalendar();

  renderTime();
  updateClashHint();
}

// Warns, but does not block, when another reminder has the same time.
function updateClashHint() {
  const ts = formTimestamp();
  const other = alarms.find(
    (a) => a.status !== "dismissed" && a.id !== editingId && a.fire_at === ts
  );
  els.clash.textContent = other
    ? `Heads up: "${other.note}" is set for this same time. Both will ring.`
    : "";
  els.clash.hidden = !other;
}

/* Composer */

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
    calendarOpen = false; // a new reminder starts with the quick chips only
    renderWhen();
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

async function saveAlarm(event) {
  event.preventDefault();

  const note = els.note.value.trim();
  if (!note) return showComposerError("Write something to remember.");

  const day = parseDateInput(formDate);
  const when = {
    year: day.getFullYear(),
    month: day.getMonth() + 1,
    day: day.getDate(),
    hour,
    minute,
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

buildTimeGrids();
applyTheme(localStorage.getItem(THEME_KEY) || "system");
setupStartupNotice();

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

els.dateChips.forEach((chip) =>
  chip.addEventListener("click", () => chooseQuickDay(Number(chip.dataset.day)))
);
els.otherDay.addEventListener("click", toggleCalendar);
els.calPrev.addEventListener("click", () => shiftMonth(-1));
els.calNext.addEventListener("click", () => shiftMonth(1));

els.meridiemButtons.forEach((btn) =>
  btn.addEventListener("click", () => setMeridiem(btn.dataset.meridiem))
);

els.confirmKeep.addEventListener("click", () => settleConfirm(false));
els.confirmDelete.addEventListener("click", () => settleConfirm(true));

// Clicking the dimmed background closes whichever dialog is open, but not clicks inside it.
els.backdrop.addEventListener("mousedown", (event) => {
  if (event.target === els.backdrop) closeComposer();
});
els.confirmBackdrop.addEventListener("mousedown", (event) => {
  if (event.target === els.confirmBackdrop) settleConfirm(false);
});

els.note.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
    els.form.requestSubmit();
  }
});

document.addEventListener("keydown", (event) => {
  // The confirm card sits on top, so it gets the keys first and blocks the rest.
  if (!els.confirmBackdrop.hidden) {
    if (event.key === "Tab") {
      trapFocus(event, els.confirmCard);
    } else if (event.key === "Escape") {
      settleConfirm(false);
    }
    return;
  }

  const composerOpen = !els.backdrop.hidden;

  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") {
    event.preventDefault();
    openComposer();
    return;
  }
  if (composerOpen && event.key === "Tab") {
    trapFocus(event, els.form);
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