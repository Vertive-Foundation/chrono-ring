use crate::alarm::{Alarm, AlarmError, Status};
use std::fmt;

/// Everything that can go wrong when working with the store.
#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    NotFound(u64),
    Invalid(AlarmError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound(id) => write!(f, "no alarm with id {id}"),
            StoreError::Invalid(err) => write!(f, "{err}"),
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

/// Holds all alarms in memory. Ids are handed out by the store, never reused.
#[derive(Debug)]
pub struct AlarmStore {
    alarms: Vec<Alarm>,
    next_id: u64,
}

impl AlarmStore {
    pub fn new() -> Self {
        Self {
            alarms: Vec::new(),
            next_id: 1,
        }
    }

    /// Creates an alarm with the next free id and returns a copy of it.
    pub fn add(&mut self, note: &str, fire_at: i64) -> Result<Alarm, StoreError> {
        let alarm = Alarm::new(self.next_id, note, fire_at)?;
        self.next_id += 1;
        self.alarms.push(alarm.clone());
        Ok(alarm)
    }

    /// All alarms, sorted by fire time (earliest first).
    pub fn list(&self) -> Vec<Alarm> {
        let mut sorted = self.alarms.clone();
        sorted.sort_by_key(|alarm| alarm.fire_at);
        sorted
    }

    /// Removes an alarm entirely.
    pub fn remove(&mut self, id: u64) -> Result<(), StoreError> {
        let index = self
            .alarms
            .iter()
            .position(|alarm| alarm.id == id)
            .ok_or(StoreError::NotFound(id))?;
        self.alarms.remove(index);
        Ok(())
    }

    /// Silences an alarm without removing it.
    pub fn dismiss(&mut self, id: u64) -> Result<(), StoreError> {
        let alarm = self
            .alarms
            .iter_mut()
            .find(|alarm| alarm.id == id)
            .ok_or(StoreError::NotFound(id))?;
        alarm.dismiss();
        Ok(())
    }

    /// Changes an alarm's note and time, and makes it waiting again.
    /// This is how an accidentally dismissed alarm is brought back.
    pub fn update(&mut self, id: u64, note: &str, fire_at: i64) -> Result<Alarm, StoreError> {
        let checked = Alarm::new(id, note, fire_at)?;
        let alarm = self
            .alarms
            .iter_mut()
            .find(|alarm| alarm.id == id)
            .ok_or(StoreError::NotFound(id))?;
        alarm.note = checked.note;
        alarm.fire_at = fire_at;
        alarm.status = Status::Waiting;
        Ok(alarm.clone())
    }

    /// Finds every alarm that is due at `now`, marks each one as ringing,
    /// and returns copies. An alarm is returned only once, because after it
    /// starts ringing it is no longer waiting.
    pub fn take_due(&mut self, now: i64) -> Vec<Alarm> {
        let mut due = Vec::new();
        for alarm in self.alarms.iter_mut() {
            if alarm.is_due(now) {
                alarm.start_ringing();
                due.push(alarm.clone());
            }
        }
        due
    }
}

impl Default for AlarmStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_increase_for_each_new_alarm() {
        let mut store = AlarmStore::new();
        let first = store.add("Gym", 1000).unwrap();
        let second = store.add("Call mum", 2000).unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
    }

    #[test]
    fn blank_note_is_rejected_and_not_stored() {
        let mut store = AlarmStore::new();
        let result = store.add("  ", 1000);
        assert_eq!(result, Err(StoreError::Invalid(AlarmError::EmptyNote)));
        assert!(store.list().is_empty());
    }

    #[test]
    fn list_is_sorted_by_fire_time() {
        let mut store = AlarmStore::new();
        store.add("Late", 3000).unwrap();
        store.add("Early", 1000).unwrap();
        let notes: Vec<String> = store.list().into_iter().map(|a| a.note).collect();
        assert_eq!(notes, vec!["Early", "Late"]);
    }

    #[test]
    fn remove_deletes_and_missing_id_is_an_error() {
        let mut store = AlarmStore::new();
        let alarm = store.add("Gym", 1000).unwrap();
        assert_eq!(store.remove(alarm.id), Ok(()));
        assert!(store.list().is_empty());
        assert_eq!(store.remove(alarm.id), Err(StoreError::NotFound(alarm.id)));
    }

    #[test]
    fn dismiss_missing_id_is_an_error() {
        let mut store = AlarmStore::new();
        assert_eq!(store.dismiss(99), Err(StoreError::NotFound(99)));
    }

    #[test]
    fn due_alarm_rings_once_then_is_not_returned_again() {
        let mut store = AlarmStore::new();
        store.add("Gym", 1000).unwrap();

        let first = store.take_due(1000);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].status, Status::Ringing);

        let second = store.take_due(2000);
        assert!(second.is_empty());
    }

    #[test]
    fn update_reschedules_and_brings_back_a_dismissed_alarm() {
        let mut store = AlarmStore::new();
        let alarm = store.add("Gym", 1000).unwrap();
        store.dismiss(alarm.id).unwrap();

        let updated = store.update(alarm.id, "Gym later", 5000).unwrap();
        assert_eq!(updated.status, Status::Waiting);
        assert_eq!(updated.fire_at, 5000);
        assert_eq!(updated.note, "Gym later");
    }
}