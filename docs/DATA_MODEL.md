# Persistence and personal memory

Storage is SQLite via `rusqlite`, in the per-user data directory. Migrations live in `/migrations`
as numbered SQL files (`0001_init.sql`, ...). They are **forward-only**, each runs inside a
`BEGIN IMMEDIATE` transaction together with its `PRAGMA user_version` bump, and they are embedded
in the binary with `include_str!` (ADR-0007). A database with a newer `user_version` than the build
knows is refused and left untouched.

## Connection settings

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
```

## Target schema

Timestamps are **UTC** Unix milliseconds (`INTEGER`). IDs are UUID v7 text (ADR-0004). The tables
in the first section exist (`migrations/0001_init.sql`, P1-T05); the others are added by the phase
shown. `settings` is created but not used yet: the user config lives in `config.json` (ADR-0008).
Appending a message also bumps `conversations.updated_at` in the same transaction.

```sql
-- P1-T05
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
  created_at       INTEGER NOT NULL,
  stopped          INTEGER NOT NULL DEFAULT 0 CHECK (stopped IN (0, 1))   -- 0002, P2-T02
);
CREATE INDEX idx_messages_conversation ON messages(conversation_id, created_at);
CREATE TABLE settings (
  key         TEXT PRIMARY KEY,
  value_json  TEXT NOT NULL,
  version     INTEGER NOT NULL
);

-- P3-T03 / P3-T04: tasks, audit, approvals, permissions. IMPLEMENTED in migrations/0003_*.sql
-- (kept separate from conversational memory). Approvals are consumed with one conditional
-- `UPDATE ... WHERE used_at IS NULL AND expires_at > now AND request_hash = ? AND decision =
-- 'approved' RETURNING ...`, so reuse is impossible even under races (SEC-05).
-- Startup marks queued/running/awaiting_approval tasks `interrupted`; nothing is replayed.
CREATE TABLE tasks (
  id            TEXT PRIMARY KEY,
  state         TEXT NOT NULL,          -- TaskState, snake_case
  request_json  TEXT NOT NULL,          -- the ToolRequest (arguments redacted where required)
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE INDEX idx_tasks_state ON tasks(state);
CREATE TABLE tool_runs (
  id               TEXT PRIMARY KEY,
  task_id          TEXT NOT NULL REFERENCES tasks(id),
  tool             TEXT NOT NULL,
  status           TEXT NOT NULL,       -- ToolStatus
  redacted_result  TEXT,                -- summary only, never raw file contents or secrets
  duration_ms      INTEGER NOT NULL,
  created_at       INTEGER NOT NULL
);
CREATE TABLE approvals (
  id            TEXT PRIMARY KEY,
  request_id    TEXT NOT NULL,
  request_hash  TEXT NOT NULL,          -- sha256(canonical_json({tool, arguments}))
  decision      TEXT NOT NULL CHECK (decision IN ('approved','rejected')),
  expires_at    INTEGER NOT NULL,
  used_at       INTEGER                 -- single use: set when consumed
);
CREATE TABLE permissions (
  id          TEXT PRIMARY KEY,
  capability  TEXT NOT NULL,            -- Capability, snake_case
  scope       TEXT NOT NULL,            -- e.g. allowed root id, or '*'
  granted_at  INTEGER NOT NULL,
  revoked_at  INTEGER
);

-- P7-T03 / P7-T05: memories (FTS5) and reminders
CREATE TABLE memories (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,            -- fact | preference | note
  content     TEXT NOT NULL,
  source_id   TEXT,                     -- provenance: message or document it came from
  updated_at  INTEGER NOT NULL
);
CREATE VIRTUAL TABLE memories_fts USING fts5(content, content='memories', content_rowid='rowid');
CREATE TABLE reminders (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  due_at      INTEGER NOT NULL,         -- UTC
  timezone    TEXT NOT NULL,            -- original IANA zone, for display and DST-correct repeats
  state       TEXT NOT NULL,            -- scheduled | fired | missed | dismissed
  created_at  INTEGER NOT NULL
);

-- P10-T01: paired devices
CREATE TABLE devices (
  id          TEXT PRIMARY KEY,
  public_key  TEXT NOT NULL,
  label       TEXT NOT NULL,
  scopes      TEXT NOT NULL,            -- JSON array of granted capabilities
  paired_at   INTEGER NOT NULL,
  revoked_at  INTEGER
);
```

Allowed roots are referred to by `root_id`. The root ID to absolute path mapping lives in config,
or a `roots` table if needed, and the model never sees absolute paths. Removing a root, or pointing its ID
at a different folder, revokes every grant with that scope (P5-T05, ADR-0014).

## Rules

- Use **parameterized SQL only.** Never format values into SQL strings.
- **Never store tokens, passwords or raw credentials** in SQLite.
- Keep audit records (`tasks`, `tool_runs`, `approvals`) **separate from** conversational memory.
- Don't hold a connection or transaction across `.await`. Run DB work on a blocking thread.
- Every migration gets a test that applies all migrations to an empty DB and to a DB at the previous version.

## Memory retrieval

1. Start with recent turns plus FTS5 search over **user-approved** memories.
2. Add embeddings or vector search only when retrieval-quality measurements justify it, with an ADR.
3. Bound the context by token budget. Every retrieved fact carries its provenance (`source_id`).
4. Never memorize secrets or sensitive documents automatically. The user can correct, export and delete memories.

## Data lifecycle

P5-T06 implements `Database::export_data` (one transaction/read snapshot), `clear_history`, and
`apply_retention`. Export schema v1 includes conversation metadata and every message, including
stopped replies, and excludes audit/config/grants. The runtime writes JSON directly to a new file
selected by the desktop native save dialog. It never overwrites an existing file or creates an
exported temporary copy. JSON exports are unencrypted and remain after clearing history.

Retention deletes conversations whose **last activity** (`updated_at`) is strictly older than
the configured cutoff. `None` keeps history; the desktop accepts 1–36500 days. Enforcement occurs
at startup and when settings are saved, not on a background timer. Saving a changed finite
retention period asks for confirmation because it deletes expired history immediately.

Bulk deletion cascades messages, runs with `secure_delete = ON`, then `VACUUM` and a checked
`wal_checkpoint(TRUNCATE)`. Audit/approvals/grants remain. A busy WAL reader is reported as an
incomplete cleanup **after logical deletion**; close the other reader and clear again. A config
save followed by retention failure still leaves the saved policy active; the UI reports the error
and reloads effective settings. Successful bulk changes and partial failures marked
`DataControlError.state_changed` also discard the desktop's cached chat and refresh provider status.

**No personal memory or FTS tables exist yet.** Their export/delete controls report unavailable,
and the export distinguishes unavailable memory from an empty supported store. P7 must extend
export, clear/delete and retention to derived indexes; the current data-control test deliberately
fails if memory/FTS tables appear without updating it. The P5-T06 FTS acceptance remains open.

- History retention is configurable. Raw audio is off. Audit payloads are redacted.
- **Deletion removes derived data too**: FTS rows, caches and exported temp files. Document the
  limitations, for example OS backups (Time Machine, File History) and WAL checkpoints.
  Run `VACUUM` after bulk deletes.
- Consider an encrypted database (SQLCipher) for sensitive deployments. That needs an ADR.
- Recovery: before migrating an existing DB, `Database::open` copies it to `loki.db.bak-v<old version>`
  with `VACUUM INTO` (tested: the backup holds the pre-migration data at the old version). Restoring is
  manual for now: replace `loki.db` with the backup while Loki is closed.
