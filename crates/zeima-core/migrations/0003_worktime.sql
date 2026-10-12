-- Work time: country profile selection, schedules, absences, custom
-- holidays, balance corrections and per-user work settings. All synced
-- tables carry the common columns (docs/SPEC.md §4).

CREATE TABLE jurisdiction_selection (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    -- NULL = workspace default; set = override for one user (org edition).
    user_id       TEXT REFERENCES app_user (id),
    profile_id    TEXT NOT NULL,
    region        TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX jurisdiction_selection_scope
    ON jurisdiction_selection (workspace_id, coalesce(user_id, ''))
    WHERE deleted_at IS NULL;

CREATE TABLE work_schedule (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    valid_from    TEXT NOT NULL, -- YYYY-MM-DD
    model         TEXT NOT NULL CHECK (model IN ('fixed', 'flextime', 'trust')),
    days_json     TEXT NOT NULL, -- 7 × DayPlan, Monday first
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX work_schedule_valid_from
    ON work_schedule (user_id, valid_from)
    WHERE deleted_at IS NULL;

CREATE TABLE absence_type (
    id             TEXT PRIMARY KEY NOT NULL,
    workspace_id   TEXT NOT NULL REFERENCES workspace (id),
    -- Built-in types have a key (vacation, sick, comp_time, training, other)
    -- and are translated by the UI; custom types only have a name.
    key            TEXT,
    name           TEXT NOT NULL,
    color          TEXT NOT NULL,
    reduces_target INTEGER NOT NULL,
    affects_vacation INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    deleted_at     INTEGER,
    hlc            TEXT NOT NULL,
    origin_device  TEXT NOT NULL
);

CREATE TABLE absence (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    type_id       TEXT NOT NULL REFERENCES absence_type (id),
    start_date    TEXT NOT NULL,
    end_date      TEXT NOT NULL,
    half_day      TEXT CHECK (half_day IN ('am', 'pm')),
    status        TEXT NOT NULL DEFAULT 'approved' CHECK (status IN ('requested', 'approved', 'rejected')),
    note          TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL,
    CHECK (end_date >= start_date),
    CHECK (half_day IS NULL OR start_date = end_date)
);

CREATE INDEX absence_range ON absence (user_id, start_date) WHERE deleted_at IS NULL;

CREATE TABLE custom_holiday (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    date          TEXT NOT NULL,
    name          TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE TABLE balance_adjustment (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    date          TEXT NOT NULL,
    minutes       INTEGER NOT NULL,
    reason        TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

-- One row per user: vacation entitlement, break deduction, auto-pause.
CREATE TABLE work_settings (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    settings_json TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX work_settings_user ON work_settings (user_id) WHERE deleted_at IS NULL;

-- Cache of computed days (worktime::WorkDay as JSON). Derived data: never
-- synced, rebuilt on demand. Rows are deleted when inputs change.
CREATE TABLE daily_summary (
    user_id         TEXT NOT NULL,
    date            TEXT NOT NULL,
    data            TEXT NOT NULL,
    profile_version TEXT NOT NULL,
    computed_at     INTEGER NOT NULL,
    PRIMARY KEY (user_id, date)
);

-- Auto-pause bookkeeping. `applied` = entries were already split around the
-- pause; `accepted` NULL = waiting for the user's decision.
ALTER TABLE pause ADD COLUMN followup_entry_id TEXT REFERENCES time_entry (id);
ALTER TABLE pause ADD COLUMN applied INTEGER NOT NULL DEFAULT 1;
