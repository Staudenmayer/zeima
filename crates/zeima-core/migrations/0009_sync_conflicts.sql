-- Remote changes that could not be applied locally (for example a second
-- project with the same alias). The server's state is still remembered in
-- sync_state; the user resolves these in the conflict list.
CREATE TABLE sync_conflict (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT NOT NULL,
    row_id     TEXT NOT NULL,
    message    TEXT NOT NULL,
    at         INTEGER NOT NULL,
    UNIQUE (table_name, row_id)
);
