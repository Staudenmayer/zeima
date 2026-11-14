-- Who decided on an absence request (shared workspaces) and why.
ALTER TABLE absence ADD COLUMN decided_by TEXT;
ALTER TABLE absence ADD COLUMN decision_note TEXT NOT NULL DEFAULT '';
