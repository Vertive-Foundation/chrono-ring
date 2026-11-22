use serde::{Deserialize, Serialize};
use std::fmt;

/// Where an alarm is in its life cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Waiting,
    Ringing,
    Dismissed,
}

/// One alarm. `fire_at` is a Unix timestamp in seconds (UTC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: u64,
    pub note: String,
    pub fire_at: i64,
    pub status: Status,
}

/// Errors that can happen when creating an alarm.
#[derive(Debug, PartialEq, Eq)]
pub enum AlarmError {
    EmptyNote,
}

impl fmt::Display for AlarmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlarmError::EmptyNote => write!(f, "the note cannot be empty"),
        }
    }
}

impl std::error::Error for AlarmError {}

impl Alarm {
    /// Creates a new waiting alarm. Returns an error if the note is blank.
    pub fn new(id: u64, note: &str, fire_at: i64) -> Result<Self, AlarmError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(AlarmError::EmptyNote);
        }
        Ok(Self {
            id,
            note: note.to_string(),
            fire_at,
            status: Status::Waiting,
        })
    }

    /// True if the alarm is still waiting and its time has arrived.
    /// `now` is passed in so tests can control the time.
    pub fn is_due(&self, now: i64) -> bool {
        self.status == Status::Waiting && now >= self.fire_at
    }

    /// Moves a waiting alarm to ringing. Other states stay as they are.
    pub fn start_ringing(&mut self) {
        if self.status == Status::Waiting {
            self.status = Status::Ringing;
        }
    }

    /// Silences the alarm for good.
    pub fn dismiss(&mut self) {
        self.status = Status::Dismissed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_alarm_starts_waiting() {
        let alarm = Alarm::new(1, "Gym", 1000).unwrap();
        assert_eq!(alarm.status, Status::Waiting);
        assert_eq!(alarm.note, "Gym");
    }

    #[test]
    fn blank_note_is_rejected() {
        let result = Alarm::new(1, "   ", 1000);
        assert_eq!(result, Err(AlarmError::EmptyNote));
    }

    #[test]
    fn alarm_is_due_only_at_or_after_its_time() {
        let alarm = Alarm::new(1, "Gym", 1000).unwrap();
        assert!(!alarm.is_due(999));
        assert!(alarm.is_due(1000));
        assert!(alarm.is_due(1500));
    }

    #[test]
    fn dismissed_alarm_is_never_due() {
        let mut alarm = Alarm::new(1, "Gym", 1000).unwrap();
        alarm.dismiss();
        assert!(!alarm.is_due(2000));
    }

    #[test]
    fn only_waiting_alarms_start_ringing() {
        let mut alarm = Alarm::new(1, "Gym", 1000).unwrap();
        alarm.start_ringing();
        assert_eq!(alarm.status, Status::Ringing);

        alarm.dismiss();
        alarm.start_ringing();
        assert_eq!(alarm.status, Status::Dismissed);
    }
}