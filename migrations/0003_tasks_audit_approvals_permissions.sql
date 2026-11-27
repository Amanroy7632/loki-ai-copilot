-- 0003: tasks, audit, approvals and permissions (P3-T03/T04, docs/DATA_MODEL.md).
-- Kept separate from conversational memory. Forward-only.

CREATE TABLE tasks (
  id            TEXT PRIMARY KEY,
  state         TEXT NOT NULL CHECK (state IN ('queued','running','awaiting_approval','succeeded','failed','cancelled','interrupted')),
  request_json  TEXT NOT NULL,          -- the ToolRequest, redacted by the runtime before storing
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE INDEX idx_tasks_state ON tasks(state);

CREATE TABLE tool_runs (
  id               TEXT PRIMARY KEY,
  task_id          TEXT NOT NULL REFERENCES tasks(id),
  tool             TEXT NOT NULL,
  status           TEXT NOT NULL,       -- ToolStatus, snake_case
  redacted_result  TEXT,                -- summary only, never raw file contents or secrets
  duration_ms      INTEGER NOT NULL,
  created_at       INTEGER NOT NULL
);
CREATE INDEX idx_tool_runs_task ON tool_runs(task_id);
CREATE INDEX idx_tool_runs_created ON tool_runs(created_at);

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
  scope       TEXT NOT NULL,            -- an allowed root id, or '*'
  granted_at  INTEGER NOT NULL,
  revoked_at  INTEGER
);
CREATE INDEX idx_permissions_active ON permissions(capability, scope) WHERE revoked_at IS NULL;
