-- Rules an organization sets for its people (shared workspaces): for the whole
-- organization (group_path NULL) or for one department. The id comes from the
-- scope, so every device means the same row.
CREATE TABLE org_policy (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    group_path    TEXT,
    settings_json TEXT NOT NULL DEFAULT '{}',
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL
);

CREATE UNIQUE INDEX org_policy_scope ON org_policy (workspace_id, coalesce(group_path, '')) WHERE deleted_at IS NULL;
