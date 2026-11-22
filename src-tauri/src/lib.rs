mod alarm;
mod store;
mod time;

use std::sync::Mutex;

use alarm::{Alarm, Status};
use serde::Serialize;
use store::AlarmStore;
use tauri::State;
use time::DateInput;

/// The store, shared by every command. The Mutex lets only one command change it at a time.
type SharedStore = Mutex<AlarmStore>;

/// What the screen receives for each alarm. It adds a ready-to-show time string.
#[derive(Serialize)]
pub struct AlarmView {
    id: u64,
    note: String,
    fire_at: i64,
    status: Status,
    display_time: String,
}

impl From<Alarm> for AlarmView {
    fn from(alarm: Alarm) -> Self {
        let display_time = time::format_timestamp(alarm.fire_at)
            .unwrap_or_else(|| "unknown time".to_string());
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
    Ok(guard.list().into_iter().map(AlarmView::from).collect())
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

/// Silences an alarm but keeps it in the list.
#[tauri::command]
fn dismiss_alarm(id: u64, store: State<'_, SharedStore>) -> Result<(), String> {
    let mut guard = store.lock().map_err(|e| e.to_string())?;
    guard.dismiss(id).map_err(|e| e.to_string())
}

/// Removes an alarm from the list.
#[tauri::command]
fn delete_alarm(id: u64, store: State<'_, SharedStore>) -> Result<(), String> {
    let mut guard = store.lock().map_err(|e| e.to_string())?;
    guard.remove(id).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(AlarmStore::new()))
        .invoke_handler(tauri::generate_handler![
            list_alarms,
            add_alarm,
            dismiss_alarm,
            delete_alarm
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}