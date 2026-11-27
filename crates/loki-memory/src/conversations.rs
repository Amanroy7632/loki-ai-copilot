//! Repository for `conversations` and `messages`.
//!
//! Every statement is a constant string with `?N` parameters. Values are never formatted into
//! SQL (a test greps this crate for `format!`).

use loki_core::{Conversation, ConversationId, Message, MessageId, Role, UnixMillis};
use rusqlite::{OptionalExtension, Row, params};

use crate::db::Database;
use crate::error::MemoryError;

fn conversation_from_row(row: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: ConversationId::new(row.get::<_, String>(0)?),
        title: row.get(1)?,
        created_at: UnixMillis(row.get(2)?),
        updated_at: UnixMillis(row.get(3)?),
    })
}

impl Database {
    /// Inserts a new, empty conversation. `created_at` doubles as the initial `updated_at`.
    pub fn create_conversation(
        &self,
        id: &ConversationId,
        title: &str,
        created_at: UnixMillis,
    ) -> Result<Conversation, MemoryError> {
        self.conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![id.as_str(), title, created_at.0],
        )?;
        Ok(Conversation {
            id: id.clone(),
            title: title.to_owned(),
            created_at,
            updated_at: created_at,
        })
    }

    /// Looks up one conversation.
    pub fn get_conversation(
        &self,
        id: &ConversationId,
    ) -> Result<Option<Conversation>, MemoryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?1",
                params![id.as_str()],
                conversation_from_row,
            )
            .optional()?)
    }

    /// All conversations, most recently updated first.
    pub fn list_conversations(&self) -> Result<Vec<Conversation>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT id, title, created_at, updated_at FROM conversations \
             ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], conversation_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Deletes a conversation and, through `ON DELETE CASCADE`, all of its messages.
    /// Returns `false` if it didn't exist.
    pub fn delete_conversation(&self, id: &ConversationId) -> Result<bool, MemoryError> {
        let changed = self.conn.execute(
            "DELETE FROM conversations WHERE id = ?1",
            params![id.as_str()],
        )?;
        Ok(changed > 0)
    }

    /// Appends a message and bumps the conversation's `updated_at`, atomically.
    ///
    /// Fails with [`MemoryError::ConversationNotFound`] if the conversation doesn't exist.
    pub fn append_message(&mut self, message: &Message) -> Result<(), MemoryError> {
        let tx = self.conn.transaction()?;
        let updated = tx.execute(
            "UPDATE conversations SET updated_at = MAX(updated_at, ?2) WHERE id = ?1",
            params![message.conversation_id.as_str(), message.created_at.0],
        )?;
        if updated == 0 {
            return Err(MemoryError::ConversationNotFound(
                message.conversation_id.to_string(),
            ));
        }
        tx.execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.id.as_str(),
                message.conversation_id.as_str(),
                message.role.as_str(),
                message.content,
                message.created_at.0
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Messages of one conversation, oldest first. Ties on `created_at` keep insertion order.
    pub fn list_messages(
        &self,
        conversation: &ConversationId,
    ) -> Result<Vec<Message>, MemoryError> {
        let mut statement = self.conn.prepare(
            "SELECT id, conversation_id, role, content, created_at FROM messages \
             WHERE conversation_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = statement.query_map(params![conversation.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (id, conversation_id, role, content, created_at) = row?;
            let role = Role::parse(&role).ok_or_else(|| MemoryError::Corrupt {
                table: "messages",
                detail: String::from("unknown role"),
            })?;
            messages.push(Message {
                id: MessageId::new(id),
                conversation_id: ConversationId::new(conversation_id),
                role,
                content,
                created_at: UnixMillis(created_at),
            });
        }
        Ok(messages)
    }

    /// Deletes one message. Used when the user explicitly asks to regenerate a reply (`/retry`).
    /// Returns `false` if it didn't exist.
    pub fn delete_message(&self, id: &MessageId) -> Result<bool, MemoryError> {
        let changed = self
            .conn
            .execute("DELETE FROM messages WHERE id = ?1", params![id.as_str()])?;
        Ok(changed > 0)
    }
}
