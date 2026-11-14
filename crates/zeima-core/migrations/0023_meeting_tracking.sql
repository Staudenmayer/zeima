-- What a meeting carries for tracking its time, like a time entry: tags,
-- whether it is billable and a cost center (none: automatic).
ALTER TABLE meeting ADD COLUMN tag_ids_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE meeting ADD COLUMN billable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE meeting ADD COLUMN cost_center_id TEXT;
