mod alarm;
mod alarm_alert;
mod sound;
mod store;
mod time;

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use alarm::{Alarm, Status};
use serde::Serialize;
use sound::AlarmSound;
use store::AlarmStore;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use time::DateInput;

/// The store, shared by every command and by the checker thread.
/// The Mutex lets only one of them change it at a time.
type SharedStore = Mutex<AlarmStore>;

/// How often the checker looks at the clock. Checking a short list is cheap,
/// so one second keeps alarms close to their exact time.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Name of the event the screen listens for when alarms start ringing.
const ALARMS_RINGING_EVENT: &str = "alarms-ringing";

/// File name of the database, inside the app's data folder.
const DATABASE_FILE: &str = "alarms.db";

/// What the screen receives for each alarm. It adds a ready-to-show time string.
#[derive(Clone, Serialize)]
pub struct AlarmView {
    id: u64,
    note: String,
    fire_at: i64,
    status: Status,
    display_time: String,
}

impl From<Alarm> for AlarmView {
    fn from(alarm: Alarm) -> Self {
        let display_time =
            time::format_timestamp(alarm.fire_at).unwrap_or_else(|| "unknown time".to_string());
        Self {
            id: alarm.id,
            note: alarm.note,
            fire_at: alarm.fire_at,
            status: alarm.status,
            display_time,
        }
    }
}

/// Returns every alarm, earliest first.
#[tauri::command]
fn list_alarms(store: State<'_, SharedStore>) -> Result<Vec<AlarmView>, String> {
    let guard = store.lock().map_err(|e| e.to_string())?;
    let alarms = guard.list().map_err(|e| e.to_string())?;
    Ok(alarms.into_iter().map(AlarmView::from).collect())
}

/// Validates the form's date and time, then saves the alarm.
#[tauri::command]
fn add_alarm(
    note: String,
    when: DateInput,
    store: State<'_, SharedStore>,
) -> Result<AlarmView, String> {
    let fire_at = time::to_timestamp(&when).map_err(|e| e.to_string())?;
    time::ensure_in_future(fire_at, time::now()).map_err(|e| e.to_string())?;

    let mut guard = store.lock().map_err(|e| e.to_string())?;
    let alarm = guard.add(&note, fire_at).map_err(|e| e.to_string())?;
    Ok(alarm.into())
}

/// Changes an existing alarm and makes it waiting again.
#[tauri::command]
fn update_alarm(
    id: u64,
    note: String,
    when: DateInput,
    store: State<'_, SharedStore>,
) -> Result<AlarmView, String> {
    let fire_at = time::to_timestamp(&when).map_err(|e| e.to_string())?;
    time::ensure_in_future(fire_at, time::now()).map_err(|e| e.to_string())?;

    let mut guard = store.lock().map_err(|e| e.to_string())?;
    let alarm = guard.update(id, &note, fire_at).map_err(|e| e.to_string())?;
    Ok(alarm.into())
}

/// Silences an alarm but keeps it in the list. Stops the tone if nothing else is ringing.
#[tauri::command]
fn dismiss_alarm(
    id: u64,
    store: State<'_, SharedStore>,
    sound: State<'_, AlarmSound>,
) -> Result<(), String> {
    let mut guard = store.lock().map_err(|e| e.to_string())?;
    guard.dismiss(id).map_err(|e| e.to_string())?;
    silence_if_nothing_ringing(&guard, &sound);
    Ok(())
}

/// Removes an alarm from the list. Stops the tone if nothing else is ringing.
#[tauri::command]
fn delete_alarm(
    id: u64,
    store: State<'_, SharedStore>,
    sound: State<'_, AlarmSound>,
) -> Result<(), String> {
    let mut guard = store.lock().map_err(|e| e.to_string())?;
    guard.remove(id).map_err(|e| e.to_string())?;
    silence_if_nothing_ringing(&guard, &sound);
    Ok(())
}

/// Stops the tone once no alarm is still ringing. Other alarms keep it going.
fn silence_if_nothing_ringing(store: &AlarmStore, sound: &AlarmSound) {
    // If the database cannot be read, this counts as "nothing ringing",
    // so the tone is never left running by mistake.
    let any_ringing = store
        .list()
        .is_ok_and(|alarms| alarms.iter().any(|alarm| alarm.status == Status::Ringing));
    if !any_ringing {
        sound.stop();
    }
}

/// Runs forever on its own thread, checking once per interval.
fn run_checker(app: AppHandle) {
    loop {
        thread::sleep(CHECK_INTERVAL);
        check_once(&app);
    }
}

/// One checker tick: take the alarms that are due. If there are any, start the
/// tone, decide how to alert the user, and tell the screen.
fn check_once(app: &AppHandle) {
    let store = app.state::<SharedStore>();

    // The lock is released at the end of this block, before any other work.
    let due = {
        // A command panicked while holding the lock. Try again on the next tick.
        let Ok(mut guard) = store.lock() else {
            return;
        };
        match guard.take_due(time::now()) {
            Ok(due) => due,
            Err(err) => {
                eprintln!("could not check for due alarms: {err}");
                return;
            }
        }
    };
    if due.is_empty() {
        return;
    }

    // The sound starts regardless of the notification decision, so it is the guaranteed signal.
    app.state::<AlarmSound>().start();

    let views: Vec<AlarmView> = due.into_iter().map(AlarmView::from).collect();

    let plan = alarm_alert::plan(alarm_alert::current_state());
    if plan.sends_toast() {
        for view in &views {
            send_toast(app, view);
        }
    }
    if plan.shows_window() {
        bring_to_front(app);
    }

    if let Err(err) = app.emit(ALARMS_RINGING_EVENT, views) {
        eprintln!("could not notify the screen about ringing alarms: {err}");
    }
}

/// Shows a Windows notification with the reminder's note and time.
fn send_toast(app: &AppHandle, alarm: &AlarmView) {
    let result = app
        .notification()
        .builder()
        .title("ChronoRing reminder")
        .body(format!("{}\n{}", alarm.note, alarm.display_time))
        .show();

    if let Err(err) = result {
        eprintln!("could not show the notification: {err}");
    }
}

/// Restores the main window if it is hidden or minimized, then focuses it.
fn bring_to_front(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // If Windows refuses focus, the alarm still rings on screen, so ignore the error.
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AlarmSound::default())
        .setup(|app| {
            // The app's own data folder, created on first run.
            // On Windows this is inside AppData\Roaming\com.vc.chrono-ring.
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = AlarmStore::open(&data_dir.join(DATABASE_FILE))?;
            app.manage(Mutex::new(store));

            // The checker starts after the store is managed, so its first tick can use it.
            let handle = app.handle().clone();
            thread::spawn(move || run_checker(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_alarms,
            add_alarm,
            update_alarm,
            dismiss_alarm,
            delete_alarm
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}