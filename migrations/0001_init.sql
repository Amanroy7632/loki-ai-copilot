-- 0001_init: conversations, messages and settings (P1-T05, docs/DATA_MODEL.md).
-- Forward-only. Never edit this file once merged; add 0002_*.sql instead.
-- The runner applies it inside a transaction and then sets PRAGMA user_version = 1.

CREATE TABLE conversations (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE messages (
  id               TEXT PRIMARY KEY,
  conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role             TEXT NOT NULL CHECK (role IN ('system','user','assistant','tool')),
  content          TEXT NOT NULL,
  created_at       INTEGER NOT NULL
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, created_at);

CREATE TABLE settings (
  key         TEXT PRIMARY KEY,
  value_json  TEXT NOT NULL,
  version     INTEGER NOT NULL
);
