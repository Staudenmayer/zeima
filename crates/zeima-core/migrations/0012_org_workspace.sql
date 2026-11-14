-- The server's organization behind a shared workspace (NULL for the
-- personal workspace).
ALTER TABLE workspace ADD COLUMN org_id TEXT;
