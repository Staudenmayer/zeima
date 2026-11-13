-- The part of the app settings that follows the user between devices:
-- language, date/time/number formats, week start, duration format and time
-- zone. (Shortcuts and autostart belong to one machine and stay in
-- local_meta.) One row per user, like work_settings; the settings are one
-- JSON object that sync merges field by field.
CREATE TABLE display_settings (
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

CREATE UNIQUE INDEX display_settings_user ON display_settings (user_id) WHERE deleted_at IS NULL;
