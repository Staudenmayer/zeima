-- Core tracking tables. Every synced table carries the common columns from
-- docs/SPEC.md §4: id (UUIDv7), workspace_id, created_at, updated_at,
-- deleted_at (tombstone), hlc, origin_device. Times are unix ms UTC.

CREATE TABLE workspace (
    id            TEXT PRIMARY KEY NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('personal', 'org')),
    name          TEXT NOT NULL,
    settings_json TEXT NOT NULL DEFAULT '{}',
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE TABLE app_user (
    id            TEXT PRIMARY KEY NOT NULL,
    display_name  TEXT NOT NULL,
    email         TEXT,
    locale        TEXT,
    timezone      TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE TABLE project (
    id               TEXT PRIMARY KEY NOT NULL,
    workspace_id     TEXT NOT NULL REFERENCES workspace (id),
    name             TEXT NOT NULL,
    alias            TEXT,
    color            TEXT NOT NULL,
    parent_id        TEXT REFERENCES project (id),
    client           TEXT,
    budget_minutes   INTEGER,
    budget_period    TEXT NOT NULL DEFAULT 'total' CHECK (budget_period IN ('total', 'monthly', 'weekly')),
    budget_warn_pct  INTEGER NOT NULL DEFAULT 80,
    billable_default INTEGER NOT NULL DEFAULT 0,
    status           TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'archived')),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    deleted_at       INTEGER,
    hlc              TEXT NOT NULL,
    origin_device    TEXT NOT NULL
);

CREATE UNIQUE INDEX project_alias_unique
    ON project (workspace_id, lower(alias))
    WHERE alias IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE tag (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    name          TEXT NOT NULL,
    color         TEXT NOT NULL,
    project_id    TEXT REFERENCES project (id),
    group_name    TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX tag_name_unique
    ON tag (workspace_id, coalesce(project_id, ''), lower(name))
    WHERE deleted_at IS NULL;

CREATE TABLE time_entry (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    project_id    TEXT REFERENCES project (id),
    start_at      INTEGER NOT NULL,
    end_at        INTEGER,
    tz            TEXT NOT NULL,
    note          TEXT NOT NULL DEFAULT '',
    billable      INTEGER NOT NULL DEFAULT 0,
    source        TEXT NOT NULL,
    source_ref    TEXT,
    locked        INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL,
    CHECK (end_at IS NULL OR end_at >= start_at)
);

CREATE INDEX time_entry_range ON time_entry (user_id, start_at) WHERE deleted_at IS NULL;

-- At most one running entry per user.
CREATE UNIQUE INDEX time_entry_one_running
    ON time_entry (user_id)
    WHERE end_at IS NULL AND deleted_at IS NULL;

-- Tags of an entry. Not synced on its own: the entry's change-log payload
-- carries `tagIds`, and sync treats the tag set as one entry field.
CREATE TABLE time_entry_tag (
    entry_id TEXT NOT NULL REFERENCES time_entry (id) ON DELETE CASCADE,
    tag_id   TEXT NOT NULL REFERENCES tag (id),
    PRIMARY KEY (entry_id, tag_id)
);

CREATE TABLE pause (
    id              TEXT PRIMARY KEY NOT NULL,
    workspace_id    TEXT NOT NULL REFERENCES workspace (id),
    user_id         TEXT NOT NULL REFERENCES app_user (id),
    start_at        INTEGER NOT NULL,
    end_at          INTEGER,
    kind            TEXT NOT NULL,
    rule_id         TEXT,
    accepted        INTEGER,
    resume_entry_id TEXT REFERENCES time_entry (id),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    deleted_at      INTEGER,
    hlc             TEXT NOT NULL,
    origin_device   TEXT NOT NULL,
    CHECK (end_at IS NULL OR end_at >= start_at)
);

CREATE INDEX pause_range ON pause (user_id, start_at) WHERE deleted_at IS NULL;

-- At most one open pause per user.
CREATE UNIQUE INDEX pause_one_open
    ON pause (user_id)
    WHERE end_at IS NULL AND deleted_at IS NULL;
