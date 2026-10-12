-- Cost centers (Kostenstellen) and their defaults (docs/SPEC.md §4.1, §5.6).
-- An entry's cost center is resolved when it is written: explicit value →
-- project default → user/group default valid on the entry's date.

CREATE TABLE cost_center (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    code          TEXT NOT NULL,
    name          TEXT NOT NULL,
    parent_id     TEXT REFERENCES cost_center (id),
    active        INTEGER NOT NULL DEFAULT 1,
    valid_from    TEXT, -- YYYY-MM-DD, inclusive
    valid_to      TEXT, -- YYYY-MM-DD, inclusive
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL,
    CHECK (valid_to IS NULL OR valid_from IS NULL OR valid_to >= valid_from)
);

CREATE UNIQUE INDEX cost_center_code_unique
    ON cost_center (workspace_id, lower(code))
    WHERE deleted_at IS NULL;

-- Default cost center for a user or (org edition) a group. Exactly one of
-- user_id / group_id is set. Groups arrive with the org edition, so
-- group_id has no foreign key yet.
CREATE TABLE cost_center_assignment (
    id             TEXT PRIMARY KEY NOT NULL,
    workspace_id   TEXT NOT NULL REFERENCES workspace (id),
    user_id        TEXT REFERENCES app_user (id),
    group_id       TEXT,
    cost_center_id TEXT NOT NULL REFERENCES cost_center (id),
    valid_from     TEXT NOT NULL, -- YYYY-MM-DD, inclusive
    valid_to       TEXT,          -- YYYY-MM-DD, inclusive
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    deleted_at     INTEGER,
    hlc            TEXT NOT NULL,
    origin_device  TEXT NOT NULL,
    CHECK ((user_id IS NULL) != (group_id IS NULL)),
    CHECK (valid_to IS NULL OR valid_to >= valid_from)
);

CREATE INDEX cost_center_assignment_user
    ON cost_center_assignment (user_id, valid_from)
    WHERE deleted_at IS NULL;

ALTER TABLE project ADD COLUMN cost_center_id TEXT REFERENCES cost_center (id);
ALTER TABLE time_entry ADD COLUMN cost_center_id TEXT REFERENCES cost_center (id);
