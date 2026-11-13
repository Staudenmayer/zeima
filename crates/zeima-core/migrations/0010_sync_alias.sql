-- Rows that every device creates for itself (work settings, the country
-- selection, built-in absence types, a schedule for a given start date) get
-- different random ids on each device. Sync names them by an id derived from
-- what identifies them (`canonical_id`, see repo/sync_ids.rs); this table
-- remembers which local row stands for which canonical id.
CREATE TABLE sync_alias (
    table_name   TEXT NOT NULL,
    local_id     TEXT NOT NULL,
    canonical_id TEXT NOT NULL,
    PRIMARY KEY (table_name, local_id),
    UNIQUE (table_name, canonical_id)
);
