use std::time::{Duration, Instant};

use ratatui::widgets::ListState;

/// How long a toast stays on screen.
pub const INFO_TTL: Duration = Duration::from_secs(3);
/// Errors stay longer: they usually need reading.
pub const ERROR_TTL: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

#[derive(Default)]
pub struct UiState {
    pub list: ListState,
    /// Toast in the top right, until `status_until`.
    pub status: Option<Status>,
    pub status_until: Option<Instant>,
    /// Key help overlay open (`?`). Any key closes it.
    pub show_help: bool,
}

impl UiState {
    pub fn error(&mut self, msg: impl Into<String>) {
        self.show(Status::Error(msg.into()), ERROR_TTL);
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.show(Status::Info(msg.into()), INFO_TTL);
    }

    fn show(&mut self, status: Status, ttl: Duration) {
        self.status = Some(status);
        self.status_until = Some(Instant::now() + ttl);
    }

    /// Drop the toast once `now` has reached its expiry. `now` is passed in
    /// so tests don't have to sleep.
    pub fn expire(&mut self, now: Instant) {
        if self.status_until.is_some_and(|until| now >= until) {
            self.status = None;
            self.status_until = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_sets_expiry() {
        let mut s = UiState::default();
        let before = Instant::now();
        s.info("hi");
        assert_eq!(s.status, Some(Status::Info("hi".into())));
        assert!(s.status_until.unwrap() >= before + INFO_TTL);
    }

    #[test]
    fn errors_stay_longer_than_info() {
        let mut s = UiState::default();
        s.info("i");
        let info_until = s.status_until.unwrap();
        s.error("e");
        assert!(s.status_until.unwrap() > info_until);
    }

    #[test]
    fn expire_keeps_toast_until_its_time() {
        let mut s = UiState::default();
        s.info("hi");
        let until = s.status_until.unwrap();

        s.expire(until - Duration::from_millis(1));
        assert!(s.status.is_some());

        s.expire(until);
        assert!(s.status.is_none());
        assert!(s.status_until.is_none());
    }

    #[test]
    fn expire_without_toast_is_noop() {
        let mut s = UiState::default();
        s.expire(Instant::now());
        assert!(s.status.is_none());
    }
}
