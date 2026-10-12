-- Installed addons and their key-value storage. Device-local like
-- `local_meta`: the WASM modules live in the app data folder, so these rows
-- are not synced and do not go through the change log.

CREATE TABLE addon (
    id            TEXT PRIMARY KEY NOT NULL,
    version       TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 0,
    -- Permissions the user granted at install (JSON array).
    granted_json  TEXT NOT NULL DEFAULT '[]',
    -- Settings form values (JSON object of strings).
    settings_json TEXT NOT NULL DEFAULT '{}',
    installed_at  INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE addon_kv (
    addon_id TEXT NOT NULL REFERENCES addon (id) ON DELETE CASCADE,
    key      TEXT NOT NULL,
    value    TEXT NOT NULL,
    PRIMARY KEY (addon_id, key)
);
