//! Time representation used in contracts.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch, always UTC.
///
/// Contracts avoid a date-time library to keep `loki-core` dependency-light. Conversion to
/// calendar types and time zones happens at the edges (UI, reminders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
// Exported as `number`, not `bigint`: JSON has no 64-bit integers, and millisecond timestamps stay
// below 2^53 until the year 287396.
#[cfg_attr(feature = "ts", ts(type = "number"))]
pub struct UnixMillis(pub i64);

impl UnixMillis {
    /// Current wall-clock time.
    ///
    /// Code that needs deterministic tests should take a clock as a parameter instead of
    /// calling this directly.
    pub fn now() -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        Self(i64::try_from(millis).unwrap_or(i64::MAX))
    }

    /// Returns `true` if this instant is at or after `deadline`.
    pub const fn has_reached(self, deadline: Self) -> bool {
        self.0 >= deadline.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_is_reached_at_and_after_the_instant() {
        let deadline = UnixMillis(1_000);
        assert!(!UnixMillis(999).has_reached(deadline));
        assert!(UnixMillis(1_000).has_reached(deadline));
        assert!(UnixMillis(1_001).has_reached(deadline));
    }

    #[test]
    fn serializes_as_a_plain_number_and_round_trips() {
        let at = UnixMillis(1_800_000_000_123);
        assert_eq!(serde_json::to_string(&at).unwrap(), "1800000000123");
        assert_eq!(
            serde_json::from_str::<UnixMillis>("1800000000123").unwrap(),
            at
        );
    }

    #[test]
    fn now_is_after_2020() {
        assert!(UnixMillis::now().0 > 1_577_836_800_000);
    }
}
