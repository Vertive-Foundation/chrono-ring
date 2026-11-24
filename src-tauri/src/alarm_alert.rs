//! Decides how an alarm should reach the user right now.
//!
//! The decision is split in two so it can be tested without Windows:
//! `current_state` asks the operating system, and `plan` is a pure function
//! that turns that answer into an action.

/// What Windows says about notifications at this moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationState {
    /// Notifications can be shown.
    Allowed,
    /// Quiet time, Focus Assist, a fullscreen app, or presentation mode.
    Blocked,
    /// The question could not be answered, or we are not on Windows.
    Unknown,
}

/// The action to take for a due alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertPlan {
    /// Send a Windows notification. The window stays where it is.
    Toast,
    /// Bring the ChronoRing window to the front, with no notification.
    WindowOnly,
    /// Do both. Used when we cannot tell whether notifications are allowed.
    ToastAndWindow,
}

impl AlertPlan {
    pub fn sends_toast(self) -> bool {
        matches!(self, AlertPlan::Toast | AlertPlan::ToastAndWindow)
    }

    pub fn shows_window(self) -> bool {
        matches!(self, AlertPlan::WindowOnly | AlertPlan::ToastAndWindow)
    }
}

/// Pure decision: no Windows calls, no window, no clock.
pub fn plan(state: NotificationState) -> AlertPlan {
    match state {
        NotificationState::Allowed => AlertPlan::Toast,
        NotificationState::Blocked => AlertPlan::WindowOnly,
        NotificationState::Unknown => AlertPlan::ToastAndWindow,
    }
}

/// Asks Windows whether notifications are allowed right now.
#[cfg(windows)]
pub fn current_state() -> NotificationState {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS, QUNS_NOT_PRESENT,
    };

    // SAFETY: the call takes no arguments and only reads system state.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) if state == QUNS_ACCEPTS_NOTIFICATIONS || state == QUNS_NOT_PRESENT => {
            // NOT_PRESENT means nobody is at the keyboard, so a toast is still useful.
            NotificationState::Allowed
        }
        Ok(_) => NotificationState::Blocked,
        Err(_) => NotificationState::Unknown,
    }
}

/// On other systems we have no answer, so the caller covers both options.
#[cfg(not(windows))]
pub fn current_state() -> NotificationState {
    NotificationState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_sends_a_toast_only() {
        let plan = plan(NotificationState::Allowed);
        assert_eq!(plan, AlertPlan::Toast);
        assert!(plan.sends_toast());
        assert!(!plan.shows_window());
    }

    #[test]
    fn blocked_opens_the_window_and_sends_no_toast() {
        let plan = plan(NotificationState::Blocked);
        assert_eq!(plan, AlertPlan::WindowOnly);
        assert!(!plan.sends_toast());
        assert!(plan.shows_window());
    }

    #[test]
    fn unknown_does_both_so_the_alarm_is_never_missed() {
        let plan = plan(NotificationState::Unknown);
        assert_eq!(plan, AlertPlan::ToastAndWindow);
        assert!(plan.sends_toast());
        assert!(plan.shows_window());
    }
}