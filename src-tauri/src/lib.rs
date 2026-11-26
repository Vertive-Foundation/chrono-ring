mod alarm;
mod alarm_alert;
mod sound;
mod store;
mod time;

use std::fs;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use alarm::{Alarm, Status};
use serde::Serialize;
use sound::AlarmSound;
use store::AlarmStore;
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
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

/// Marker file that records the first run has happened. Its presence means
/// the default startup setting was already applied, so it is never applied again.
const FIRST_RUN_MARKER: &str = "first-run-done";

/// Identifiers for the tray menu items. Matched when a menu item is clicked.
const TRAY_SHOW_ID: &str = "show";
const TRAY_AUTOSTART_ID: &str = "autostart";
const TRAY_QUIT_ID: &str = "quit";

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
        show_main_window(app);
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

/// Brings the main window back from the tray or from minimized, then focuses it.
/// Used by the checker, the tray icon, and the tray's Show menu item.
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // If Windows refuses focus, the window is still visible, so ignore the error.
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Turns "Start with Windows" on or off to match the checkbox.
/// The checkbox already flipped when clicked, so we read its new state and apply it.
/// If Windows refuses the change, the checkbox is put back to what it was.
fn toggle_autostart(app: &AppHandle, item: &CheckMenuItem<Wry>) {
    let want_enabled = item.is_checked().unwrap_or(false);
    let manager = app.autolaunch();

    let result = if want_enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    if let Err(err) = result {
        eprintln!("could not change start with Windows: {err}");
        let _ = item.set_checked(!want_enabled);
    }
}

/// On the very first launch, turns on "Start with Windows" so alarms work after a restart.
/// A marker file records that this happened, so the user's later choice is never overridden.
fn apply_first_run_default(app: &AppHandle, data_dir: &std::path::Path) {
    let marker = data_dir.join(FIRST_RUN_MARKER);
    if marker.exists() {
        return;
    }

    if let Err(err) = app.autolaunch().enable() {
        eprintln!("could not enable start with Windows on first run: {err}");
        // Leave the marker absent so the next launch tries again.
        return;
    }

    if let Err(err) = fs::write(&marker, b"done") {
        eprintln!("could not record the first run: {err}");
    }
}

/// Builds the tray icon with its right-click menu.
/// A left click on the icon shows the window again.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // The checkbox starts out matching what Windows currently has.
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);

    let show_item = MenuItem::with_id(app, TRAY_SHOW_ID, "Show ChronoRing", true, None::<&str>)?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        TRAY_AUTOSTART_ID,
        "Start with Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &autostart_item, &quit_item])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    // A clone is cheap and points at the same menu item, so the menu handler can use it.
    let autostart_for_events = autostart_item.clone();

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("ChronoRing")
        .menu(&menu)
        // Left click opens the window directly, so the menu is only for right click.
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            TRAY_SHOW_ID => show_main_window(app),
            TRAY_AUTOSTART_ID => toggle_autostart(app, &autostart_for_events),
            // Quit is the only real exit. Closing the window just hides it.
            TRAY_QUIT_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // React on button release, the normal Windows convention for a click.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Registers the app to start at sign-in when the tray toggle is on.
        // None means no extra arguments are passed at startup.
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(AlarmSound::default())
        .on_window_event(|window, event| {
            // Closing the window hides it, so the checker and the tray keep working.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            // The app's own data folder, created on first run.
            // On Windows this is inside AppData\Roaming\com.vc.chrono-ring.
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;

            apply_first_run_default(app.handle(), &data_dir);

            let store = AlarmStore::open(&data_dir.join(DATABASE_FILE))?;
            app.manage(Mutex::new(store));

            // The checker starts after the store is managed, so its first tick can use it.
            let handle = app.handle().clone();
            thread::spawn(move || run_checker(handle));

            build_tray(app.handle())?;
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