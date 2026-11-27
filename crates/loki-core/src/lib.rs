//! # loki-core
//!
//! Domain types that every other Loki crate shares: identifiers, the tool contract, policy
//! decisions, task states, conversation messages and platform capability status.
//!
//! ## Rules for this crate
//!
//! - **No** UI, OS, network, database or async-runtime dependencies. If a type needs one of
//!   those, it belongs in another crate.
//! - Only plain data and pure functions. Everything is (de)serializable, so it can cross the
//!   Tauri IPC boundary and be stored in the audit log.
//! - Changing a serialized shape is a **contract change**. Update `docs/CONTRACTS.md` and
//!   regenerate `packages/contracts` (P1-T03) in the same commit.
//!
//! See `docs/CONTRACTS.md` for how these types are used end to end.

pub mod capability;
pub mod decision;
pub mod ids;
pub mod message;
pub mod task;
pub mod time;
pub mod tool;

pub use capability::{Capability, CapabilityStatus};
pub use decision::Decision;
pub use ids::{ConversationId, MessageId, RequestId, SessionId, ToolName};
pub use message::{Conversation, Message, Role};
pub use task::TaskState;
pub use time::UnixMillis;
pub use tool::{ErrorCode, Risk, ToolRequest, ToolResult, ToolSpec, ToolStatus};
