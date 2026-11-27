-- 0002_message_stopped: mark assistant replies the user stopped mid-generation (P2-T02).
-- Forward-only. Existing rows were all complete replies, hence DEFAULT 0.

ALTER TABLE messages ADD COLUMN stopped INTEGER NOT NULL DEFAULT 0 CHECK (stopped IN (0, 1));
