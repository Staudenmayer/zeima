-- Device-local key/value data that is never synced (device id, current
-- workspace/user, device settings, timer heartbeat).
CREATE TABLE local_meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

-- Outbox of local changes, written in the same transaction as the change.
-- The sync client (paid tiers) pushes rows with pushed_at IS NULL.
CREATE TABLE change_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT NOT NULL,
    row_id     TEXT NOT NULL,
    op         TEXT NOT NULL CHECK (op IN ('upsert', 'delete')),
    hlc        TEXT NOT NULL,
    payload    TEXT,
    pushed_at  INTEGER
);

CREATE INDEX change_log_unpushed ON change_log (seq) WHERE pushed_at IS NULL;
