//! ID generation (ADR-0004): UUID v7, rendered as lowercase hyphenated text.
//!
//! v7 IDs start with a millisecond timestamp, so they sort roughly by creation time and keep
//! SQLite's primary-key index append-mostly. Core treats IDs as opaque strings; only the
//! runtime decides the format.

use loki_core::{ConversationId, MessageId, RequestId};
use uuid::Uuid;

/// A new conversation ID.
pub fn new_conversation_id() -> ConversationId {
    ConversationId::new(Uuid::now_v7().to_string())
}

/// A new message ID.
pub fn new_message_id() -> MessageId {
    MessageId::new(Uuid::now_v7().to_string())
}

/// A new tool request ID (used from P3).
pub fn new_request_id() -> RequestId {
    RequestId::new(Uuid::now_v7().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_uuid_v7_and_unique() {
        let first = new_message_id();
        let second = new_message_id();
        assert_ne!(first, second);
        let parsed = Uuid::parse_str(first.as_str()).unwrap();
        assert_eq!(parsed.get_version_num(), 7);
        assert_eq!(first.as_str().len(), 36);
    }

    #[test]
    fn ids_sort_by_creation_time() {
        let earlier = new_conversation_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let later = new_conversation_id();
        assert!(earlier < later);
        assert_eq!(
            Uuid::parse_str(new_request_id().as_str())
                .unwrap()
                .get_version_num(),
            7
        );
    }
}
