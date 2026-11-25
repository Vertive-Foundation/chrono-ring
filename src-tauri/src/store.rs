use std::fmt;
use std::path::Path;

use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::alarm::{Alarm, AlarmError, Status};

/// Everything that can go wrong when working with the store.
#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    NotFound(u64),
    Invalid(AlarmError),
    /// The database itself failed. Stored as text so the error stays comparable in tests.
    Database(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound(id) => write!(f, "no alarm with id {id}"),
            StoreError::Invalid(err) => write!(f, "{err}"),
            StoreError::Database(msg) => write!(f, "database error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Lets `?` turn an `AlarmError` into a `StoreError` automatically.
impl From<AlarmError> for StoreError {
    fn from(err: AlarmError) -> Self {
        StoreError::Invalid(err)
    }
}

/// Lets `?` turn any database error into a `StoreError` automatically.
impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        StoreError::Database(err.to_string())
    }
}

/// The table layout. `AUTOINCREMENT` means an id is never reused, even after a delete.
const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS alarms (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        note    TEXT    NOT NULL,
        fire_at INTEGER NOT NULL,
        status  TEXT    NOT NULL CHECK (status IN ('waiting', 'ringing', 'dismissed'))
    );";

/// Every column, in the order `row_to_alarm` reads them.
const SELECT_ALL: &str = "SELECT id, note, fire_at, status FROM alarms";

/// Saved alarms in a SQLite database file. Each change is written straight away.
#[derive(Debug)]
pub struct AlarmStore {
    conn: Connection,
}

impl AlarmStore {
    /// Opens (or creates) the database file at `path`.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::initialize(Connection::open(path)?)
    }

    /// Opens a temporary database in memory. Used by the tests.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    /// Creates the table if needed, and recovers alarms interrupted by a closed app.
    fn initialize(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA)?;
        // An alarm that was ringing when the app closed has not been acknowledged.
        // Put it back to waiting, so it rings again as soon as the app starts.
        conn.execute("UPDATE alarms SET status = 'waiting' WHERE status = 'ringing'", [])?;
        Ok(Self { conn })
    }

    /// Creates an alarm and returns it with the id SQLite assigned.
    pub fn add(&mut self, note: &str, fire_at: i64) -> Result<Alarm, StoreError> {
        // The id is a placeholder here. SQLite assigns the real one on insert.
        let mut alarm = Alarm::new(0, note, fire_at)?;
        self.conn.execute(
            "INSERT INTO alarms (note, fire_at, status) VALUES (?1, ?2, ?3)",
            params![alarm.note, alarm.fire_at, status_to_str(alarm.status)],
        )?;
        alarm.id = self.conn.last_insert_rowid() as u64;
        Ok(alarm)
    }

    /// All alarms, earliest first.
    pub fn list(&self) -> Result<Vec<Alarm>, StoreError> {
        query_alarms(&self.conn, &format!("{SELECT_ALL} ORDER BY fire_at, id"))
    }

    /// Removes an alarm entirely.
    pub fn remove(&mut self, id: u64) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM alarms WHERE id = ?1", params![id as i64])?;
        if changed == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    /// Silences an alarm without removing it.
    pub fn dismiss(&mut self, id: u64) -> Result<(), StoreError> {
        self.modify(id, Alarm::dismiss)?;
        Ok(())
    }

    /// Changes an alarm's note and time, and makes it waiting again.
    /// This is how an accidentally dismissed alarm is brought back.
    pub fn update(&mut self, id: u64, note: &str, fire_at: i64) -> Result<Alarm, StoreError> {
        // Validate first, so a bad edit never reaches the database.
        let checked = Alarm::new(id, note, fire_at)?;
        self.modify(id, |alarm| {
            alarm.note = checked.note;
            alarm.fire_at = fire_at;
            alarm.status = Status::Waiting;
        })
    }

    /// Finds every alarm that is due at `now`, marks each as ringing, and returns them.
    /// It runs in one transaction, so a failure part-way leaves nothing half-changed.
    pub fn take_due(&mut self, now: i64) -> Result<Vec<Alarm>, StoreError> {
        let tx = self.conn.transaction()?;
        let waiting = query_alarms(&tx, &format!("{SELECT_ALL} WHERE status = 'waiting'"))?;

        let mut due = Vec::new();
        for mut alarm in waiting.into_iter().filter(|alarm| alarm.is_due(now)) {
            alarm.start_ringing();
            save_alarm(&tx, &alarm)?;
            due.push(alarm);
        }
        tx.commit()?;
        Ok(due)
    }

    /// Loads one alarm, applies a change to it, and saves the result.
    /// Every single-alarm change goes through here, so they all behave the same way.
    fn modify(&self, id: u64, change: impl FnOnce(&mut Alarm)) -> Result<Alarm, StoreError> {
        let mut alarm = load_alarm(&self.conn, id)?;
        change(&mut alarm);
        save_alarm(&self.conn, &alarm)?;
        Ok(alarm)
    }
}

/// Runs a query that returns alarms and collects the rows.
fn query_alarms(conn: &Connection, sql: &str) -> Result<Vec<Alarm>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let alarms = statement
        .query_map([], row_to_alarm)?
        .collect::<rusqlite::Result<Vec<Alarm>>>()?;
    Ok(alarms)
}

/// Reads one alarm by id.
fn load_alarm(conn: &Connection, id: u64) -> Result<Alarm, StoreError> {
    conn.query_row(&format!("{SELECT_ALL} WHERE id = ?1"), params![id as i64], row_to_alarm)
        .optional()?
        .ok_or(StoreError::NotFound(id))
}

/// Writes an alarm's changeable fields back to its row.
fn save_alarm(conn: &Connection, alarm: &Alarm) -> Result<(), StoreError> {
    let changed = conn.execute(
        "UPDATE alarms SET note = ?1, fire_at = ?2, status = ?3 WHERE id = ?4",
        params![alarm.note, alarm.fire_at, status_to_str(alarm.status), alarm.id as i64],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(alarm.id));
    }
    Ok(())
}

/// Turns one database row into an `Alarm`. Columns are read by position.
fn row_to_alarm(row: &Row<'_>) -> rusqlite::Result<Alarm> {
    let raw_id: i64 = row.get(0)?;
    let raw_status: String = row.get(3)?;

    let status = status_from_str(&raw_status).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            Type::Text,
            format!("unknown status {raw_status}").into(),
        )
    })?;

    Ok(Alarm {
        id: u64::try_from(raw_id).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, raw_id))?,
        note: row.get(1)?,
        fire_at: row.get(2)?,
        status,
    })
}

/// The status as it is written in the database.
fn status_to_str(status: Status) -> &'static str {
    match status {
        Status::Waiting => "waiting",
        Status::Ringing => "ringing",
        Status::Dismissed => "dismissed",
    }
}

/// Reads a status back from the database. `None` means an unknown value.
fn status_from_str(text: &str) -> Option<Status> {
    match text {
        "waiting" => Some(Status::Waiting),
        "ringing" => Some(Status::Ringing),
        "dismissed" => Some(Status::Dismissed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn memory_store() -> AlarmStore {
        AlarmStore::open_in_memory().unwrap()
    }

    /// A database file in the temp folder. Removes any leftover from an earlier run.
    fn temp_db(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("chronoring-{name}-{}.db", std::process::id()));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn ids_increase_for_each_new_alarm() {
        let mut store = memory_store();
        let first = store.add("Gym", 1000).unwrap();
        let second = store.add("Call mum", 2000).unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
    }

    #[test]
    fn blank_note_is_rejected_and_not_stored() {
        let mut store = memory_store();
        let result = store.add("  ", 1000);
        assert_eq!(result, Err(StoreError::Invalid(AlarmError::EmptyNote)));
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn list_is_sorted_by_fire_time() {
        let mut store = memory_store();
        store.add("Late", 3000).unwrap();
        store.add("Early", 1000).unwrap();
        let notes: Vec<String> = store.list().unwrap().into_iter().map(|a| a.note).collect();
        assert_eq!(notes, vec!["Early", "Late"]);
    }

    #[test]
    fn remove_deletes_and_missing_id_is_an_error() {
        let mut store = memory_store();
        let alarm = store.add("Gym", 1000).unwrap();
        assert_eq!(store.remove(alarm.id), Ok(()));
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.remove(alarm.id), Err(StoreError::NotFound(alarm.id)));
    }

    #[test]
    fn dismiss_missing_id_is_an_error() {
        let mut store = memory_store();
        assert_eq!(store.dismiss(99), Err(StoreError::NotFound(99)));
    }

    #[test]
    fn due_alarm_rings_once_then_is_not_returned_again() {
        let mut store = memory_store();
        store.add("Gym", 1000).unwrap();

        let first = store.take_due(1000).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].status, Status::Ringing);

        let second = store.take_due(2000).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn update_reschedules_and_brings_back_a_dismissed_alarm() {
        let mut store = memory_store();
        let alarm = store.add("Gym", 1000).unwrap();
        store.dismiss(alarm.id).unwrap();

        let updated = store.update(alarm.id, "Gym later", 5000).unwrap();
        assert_eq!(updated.status, Status::Waiting);
        assert_eq!(updated.fire_at, 5000);
        assert_eq!(updated.note, "Gym later");
    }

    #[test]
    fn alarms_survive_reopening_the_database() {
        let path = temp_db("survive");
        {
            let mut store = AlarmStore::open(&path).unwrap();
            store.add("Gym", 1000).unwrap();
        }

        let store = AlarmStore::open(&path).unwrap();
        let alarms = store.list().unwrap();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].note, "Gym");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ringing_alarm_rings_again_after_restart() {
        let path = temp_db("ringing");
        {
            let mut store = AlarmStore::open(&path).unwrap();
            store.add("Gym", 1000).unwrap();
            let rang = store.take_due(1000).unwrap();
            assert_eq!(rang.len(), 1);
        }

        // The app was closed while the alarm was ringing, so it is due again on startup.
        let mut store = AlarmStore::open(&path).unwrap();
        let due = store.take_due(2000).unwrap();
        assert_eq!(due.len(), 1);

        let _ = fs::remove_file(&path);
    }
}