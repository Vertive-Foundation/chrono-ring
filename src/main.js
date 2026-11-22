const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);

const els = {
  newBtn: $("new-btn"),
  upcoming: $("upcoming"),
  past: $("past"),
  upcomingSection: $("upcoming-section"),
  pastSection: $("past-section"),
  empty: $("empty"),
  loadError: $("load-error"),
  backdrop: $("composer-backdrop"),
  form: $("composer"),
  note: $("note"),
  date: $("date"),
  hour: $("hour"),
  minute: $("minute"),
  meridiemButtons: document.querySelectorAll("[data-meridiem]"),
  error: $("composer-error"),
  save: $("save-btn"),
  cancel: $("cancel-btn"),
  close: $("close-btn"),
};

let alarms = [];
let meridiem = "am";

const STATUS_LABEL = { waiting: "Scheduled", ringing: "Ringing", dismissed: "Done" };

/* Helpers */

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function pad(n) {
  return String(n).padStart(2, "0");
}

function relativeTime(fireAt) {
  const diff = fireAt - Math.floor(Date.now() / 1000);
  const mins = Math.round(Math.abs(diff) / 60);

  let text;
  if (mins < 1) {
    text = "less than a minute";
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
  const li = el("li", `card card-${alarm.status}`);
  const body = el("div", "card-body");

  body.append(el("p", "card-note", alarm.note));

  const meta = el("div", "card-meta");
  meta.append(el("span", `pill pill-${alarm.status}`, STATUS_LABEL[alarm.status]));
  meta.append(el("span", "card-time", alarm.display_time));
  if (alarm.status !== "dismissed") {
    meta.append(el("span", "card-relative", relativeTime(alarm.fire_at)));
  }
  body.append(meta);

  const actions = el("div", "card-actions");

  if (alarm.status !== "dismissed") {
    const stop = el("button", "ghost small", alarm.status === "ringing" ? "Stop" : "Dismiss");
    stop.addEventListener("click", () =>
      run(() => invoke("dismiss_alarm", { id: alarm.id }))
    );
    actions.append(stop);
  }

  const del = el("button", "ghost small danger", "Delete");
  del.addEventListener("click", () => {
    if (!window.confirm(`Delete this reminder?\n\n"${alarm.note}"`)) return;
    run(() => invoke("delete_alarm", { id: alarm.id }));
  });
  actions.append(del);

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
  els.empty.hidden = alarms.length > 0;
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
function defaultWhen() {
  const d = new Date(Date.now() + 60 * 60 * 1000);
  d.setMinutes(Math.ceil(d.getMinutes() / 5) * 5, 0, 0);

  const h24 = d.getHours();
  const h12 = h24 % 12 === 0 ? 12 : h24 % 12;

  return {
    date: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
    hour: h12,
    minute: d.getMinutes(),
    meridiem: h24 >= 12 ? "pm" : "am",
  };
}

function setMeridiem(value) {
  meridiem = value;
  els.meridiemButtons.forEach((btn) => {
    btn.setAttribute("aria-pressed", String(btn.dataset.meridiem === value));
  });
}

function showComposerError(message) {
  els.error.textContent = message;
  els.error.hidden = false;
}

function openComposer() {
  const d = defaultWhen();
  els.note.value = "";
  els.date.value = d.date;
  els.hour.value = String(d.hour);
  els.minute.value = String(d.minute);
  setMeridiem(d.meridiem);
  els.error.hidden = true;
  els.backdrop.hidden = false;
  els.note.focus();
}

function closeComposer() {
  els.backdrop.hidden = true;
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
    await invoke("add_alarm", { note, when });
    closeComposer();
    await refresh();
  } catch (err) {
    showComposerError(String(err));
  } finally {
    els.save.disabled = false;
    els.save.textContent = "Save reminder";
  }
}

/* Wiring */

fillTimeOptions();

els.newBtn.addEventListener("click", openComposer);
els.cancel.addEventListener("click", closeComposer);
els.close.addEventListener("click", closeComposer);
els.form.addEventListener("submit", saveAlarm);

els.meridiemButtons.forEach((btn) =>
  btn.addEventListener("click", () => setMeridiem(btn.dataset.meridiem))
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
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") {
    event.preventDefault();
    openComposer();
  }
  if (event.key === "Escape" && !els.backdrop.hidden) {
    closeComposer();
  }
});

refresh();
setInterval(refresh, 5000);