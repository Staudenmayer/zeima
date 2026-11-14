-- Meetings (shared workspaces): who calls them, when, who is asked, and each
-- person's answer. An answer's id comes from the meeting and the person, so a
-- person has one answer per meeting on every device.
CREATE TABLE meeting (
    id               TEXT PRIMARY KEY NOT NULL,
    workspace_id     TEXT NOT NULL REFERENCES workspace (id),
    user_id          TEXT NOT NULL REFERENCES app_user (id),
    title            TEXT NOT NULL,
    start_at         INTEGER NOT NULL,
    end_at           INTEGER NOT NULL,
    tz               TEXT NOT NULL,
    location         TEXT,
    online_url       TEXT,
    project_id       TEXT,
    note             TEXT NOT NULL DEFAULT '',
    rrule            TEXT,
    attendee_ids_json TEXT NOT NULL DEFAULT '[]',
    cancelled        INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    deleted_at       INTEGER,
    hlc              TEXT NOT NULL,
    origin_device    TEXT NOT NULL,
    CHECK (end_at > start_at)
);

CREATE INDEX meeting_start ON meeting (workspace_id, start_at) WHERE deleted_at IS NULL;

CREATE TABLE meeting_response (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    meeting_id    TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('accepted', 'declined', 'tentative')),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX meeting_response_person ON meeting_response (workspace_id, meeting_id, user_id) WHERE deleted_at IS NULL;
