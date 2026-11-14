-- Shared workspaces: who sees a project and who runs it.
ALTER TABLE project ADD COLUMN visibility TEXT NOT NULL DEFAULT 'org' CHECK (visibility IN ('org', 'members'));
ALTER TABLE project ADD COLUMN manager_id TEXT;
ALTER TABLE project ADD COLUMN member_ids_json TEXT NOT NULL DEFAULT '[]';
