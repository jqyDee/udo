//! Short-lived message in the top right. Message and expiry live in one
//! struct, so there is never a message without a deadline or vice versa.

use std::time::{Duration, Instant};

/// How long an info toast stays on screen.
pub const INFO_TTL: Duration = Duration::from_secs(3);
/// Errors stay longer: they usually need reading.
pub const ERROR_TTL: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    pub msg: String,
    pub until: Instant,
}

impl Toast {
    pub fn info(msg: impl Into<String>) -> Self {
        Self::new(ToastKind::Info, msg.into(), INFO_TTL)
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, msg.into(), ERROR_TTL)
    }

    fn new(kind: ToastKind, msg: String, ttl: Duration) -> Self {
        Self {
            kind,
            msg,
            until: Instant::now() + ttl,
        }
    }

    /// `now` is passed in so tests don't have to sleep.
    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.until
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_expires_after_ttl() {
        let before = Instant::now();
        let t = Toast::info("hi");
        assert_eq!(t.kind, ToastKind::Info);
        assert_eq!(t.msg, "hi");
        assert!(t.until >= before + INFO_TTL);
    }

    #[test]
    fn errors_stay_longer_than_info() {
        let info = Toast::info("i");
        let error = Toast::error("e");
        assert_eq!(error.kind, ToastKind::Error);
        assert!(error.until > info.until);
    }

    #[test]
    fn expiry_boundary() {
        let t = Toast::info("hi");
        assert!(!t.is_expired(t.until - Duration::from_millis(1)));
        assert!(t.is_expired(t.until));
    }
}
