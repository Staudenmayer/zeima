-- Offices people can work from, with room for so many (shared workspaces),
-- and which office a day in the office is at.
CREATE TABLE office (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    name          TEXT NOT NULL,
    capacity      INTEGER CHECK (capacity IS NULL OR capacity > 0),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

ALTER TABLE work_location ADD COLUMN office_id TEXT;
