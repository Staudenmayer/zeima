-- Activities: what kind of work an entry is (e.g. Meeting, Development,
-- Review). Every new entry needs one; project and tags stay optional.
-- Entries recorded before this migration keep `activity_id = NULL`.

CREATE TABLE activity (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    name          TEXT NOT NULL,
    color         TEXT NOT NULL,
    archived      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX activity_name_unique
    ON activity (workspace_id, lower(name))
    WHERE deleted_at IS NULL;

ALTER TABLE time_entry ADD COLUMN activity_id TEXT REFERENCES activity (id);
