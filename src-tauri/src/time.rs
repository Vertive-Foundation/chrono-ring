use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::fmt;

/// AM or PM, as chosen in the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Meridiem {
    Am,
    Pm,
}

/// The date and time exactly as the form gives them, in 12-hour format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateInput {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,   // 1 to 12
    pub minute: u32, // 0 to 59
    pub meridiem: Meridiem,
}

/// Everything that can be wrong with a date or time from the form.
#[derive(Debug, PartialEq, Eq)]
pub enum TimeError {
    InvalidDate,
    InvalidHour,
    InvalidMinute,
    NonexistentLocalTime,
    InThePast,
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeError::InvalidDate => write!(f, "that date does not exist"),
            TimeError::InvalidHour => write!(f, "the hour must be between 1 and 12"),
            TimeError::InvalidMinute => write!(f, "the minute must be between 0 and 59"),
            TimeError::NonexistentLocalTime => {
                write!(f, "that time does not exist on this day (clocks change here)")
            }
            TimeError::InThePast => write!(f, "that time has already passed"),
        }
    }
}

impl std::error::Error for TimeError {}

/// Converts 12-hour time into 24-hour time.
/// 12 AM is midnight (0), 12 PM is noon (12).
fn to_hour24(hour: u32, meridiem: Meridiem) -> Result<u32, TimeError> {
    if !(1..=12).contains(&hour) {
        return Err(TimeError::InvalidHour);
    }
    let h = hour % 12; // turns 12 into 0
    Ok(match meridiem {
        Meridiem::Am => h,
        Meridiem::Pm => h + 12,
    })
}

/// Turns the form's date and time into a Unix timestamp (seconds, UTC).
pub fn to_timestamp(input: &DateInput) -> Result<i64, TimeError> {
    let date = NaiveDate::from_ymd_opt(input.year, input.month, input.day)
        .ok_or(TimeError::InvalidDate)?;
    let hour24 = to_hour24(input.hour, input.meridiem)?;
    let time = NaiveTime::from_hms_opt(hour24, input.minute, 0)
        .ok_or(TimeError::InvalidMinute)?;

    let local = NaiveDateTime::new(date, time)
        .and_local_timezone(Local)
        .single()
        .ok_or(TimeError::NonexistentLocalTime)?;

    Ok(local.timestamp())
}

/// Returns an error if `fire_at` is not after `now`.
pub fn ensure_in_future(fire_at: i64, now: i64) -> Result<(), TimeError> {
    if fire_at > now {
        Ok(())
    } else {
        Err(TimeError::InThePast)
    }
}

/// The current time as a Unix timestamp. Only the app calls this, not the tests.
pub fn now() -> i64 {
    Local::now().timestamp()
}

/// Formats a timestamp for display, e.g. "Fri, 4 Dec 2026, 7:30 PM".
/// Returns None if the timestamp is outside the range the clock can represent.
pub fn format_timestamp(timestamp: i64) -> Option<String> {
    let local: DateTime<Local> = Local.timestamp_opt(timestamp, 0).single()?;
    Some(local.format("%a, %-d %b %Y, %-I:%M %p").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(year: i32, month: u32, day: u32, hour: u32, minute: u32, meridiem: Meridiem) -> DateInput {
        DateInput { year, month, day, hour, minute, meridiem }
    }

    #[test]
    fn twelve_am_is_midnight_and_twelve_pm_is_noon() {
        assert_eq!(to_hour24(12, Meridiem::Am), Ok(0));
        assert_eq!(to_hour24(12, Meridiem::Pm), Ok(12));
        assert_eq!(to_hour24(1, Meridiem::Pm), Ok(13));
        assert_eq!(to_hour24(11, Meridiem::Am), Ok(11));
    }

    #[test]
    fn hour_outside_one_to_twelve_is_rejected() {
        assert_eq!(to_hour24(0, Meridiem::Am), Err(TimeError::InvalidHour));
        assert_eq!(to_hour24(13, Meridiem::Pm), Err(TimeError::InvalidHour));
    }

    #[test]
    fn impossible_date_is_rejected() {
        let result = to_timestamp(&input(2026, 2, 30, 7, 30, Meridiem::Am));
        assert_eq!(result, Err(TimeError::InvalidDate));
    }

    #[test]
    fn form_time_formats_back_the_same_way() {
        let ts = to_timestamp(&input(2026, 12, 4, 7, 30, Meridiem::Pm)).unwrap();
        assert_eq!(format_timestamp(ts), Some("Fri, 4 Dec 2026, 7:30 PM".to_string()));
    }

    #[test]
    fn past_time_is_rejected() {
        assert_eq!(ensure_in_future(100, 200), Err(TimeError::InThePast));
        assert_eq!(ensure_in_future(300, 200), Ok(()));
    }
}