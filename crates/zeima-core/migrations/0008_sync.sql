-- What this device knows about the server's version of each row: per field
-- the value and the HLC that set it, and the latest delete (see sync.rs).
-- Changes are pushed as the fields that differ from this state.
CREATE TABLE sync_state (
    table_name TEXT NOT NULL,
    row_id     TEXT NOT NULL,
    state      TEXT NOT NULL,
    PRIMARY KEY (table_name, row_id)
);
