-- Where a person works on a day (shared workspaces): office, home, travel.
-- One row per person and day; its id comes from the three, so two devices
-- that set the same day meet in the same row.
CREATE TABLE work_location (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    date          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('office', 'home', 'travel', 'other')),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX work_location_day ON work_location (workspace_id, user_id, date) WHERE deleted_at IS NULL;
