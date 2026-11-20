-- Organization workspaces on one server share the account's user id, so a
-- unique index keyed on the user alone made a second workspace collide with
-- the first. The same names, now scoped to the workspace as well.
DROP INDEX display_settings_user;
CREATE UNIQUE INDEX display_settings_user
    ON display_settings (workspace_id, user_id) WHERE deleted_at IS NULL;

DROP INDEX work_settings_user;
CREATE UNIQUE INDEX work_settings_user
    ON work_settings (workspace_id, user_id) WHERE deleted_at IS NULL;

DROP INDEX work_schedule_valid_from;
CREATE UNIQUE INDEX work_schedule_valid_from
    ON work_schedule (workspace_id, user_id, valid_from) WHERE deleted_at IS NULL;

-- The day cache is derived data: rebuilt on demand, so it starts empty with
-- the workspace in its key.
DROP TABLE daily_summary;
CREATE TABLE daily_summary (
    workspace_id    TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    date            TEXT NOT NULL,
    data            TEXT NOT NULL,
    profile_version TEXT NOT NULL,
    computed_at     INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, user_id, date)
);
