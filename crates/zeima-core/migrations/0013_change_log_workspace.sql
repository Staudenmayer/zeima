-- Which workspace a change belongs to, so each workspace is pushed to its
-- own place on the server. Existing changes are the personal workspace's.
ALTER TABLE change_log ADD COLUMN workspace_id TEXT;
UPDATE change_log SET workspace_id = (SELECT value FROM local_meta WHERE key = 'workspace_id');
