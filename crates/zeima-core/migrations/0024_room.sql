-- Rooms (places) meetings are held in, kept by the organization. A room is
-- booked by the meetings that name it; two meetings may not overlap in one.
CREATE TABLE room (
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
CREATE UNIQUE INDEX room_name_unique ON room (workspace_id, lower(name)) WHERE deleted_at IS NULL;

ALTER TABLE meeting ADD COLUMN room_id TEXT;
