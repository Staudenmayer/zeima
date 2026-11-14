-- A person's timesheet for a period, handed in for approval (shared
-- workspaces). Approving it locks the entries of the period (done by the
-- server, see server/src/modules/sync).
CREATE TABLE timesheet_submission (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspace (id),
    user_id       TEXT NOT NULL REFERENCES app_user (id),
    period_start  TEXT NOT NULL,
    period_end    TEXT NOT NULL,
    tz            TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('submitted', 'approved', 'rejected')),
    note          TEXT NOT NULL DEFAULT '',
    submitted_at  INTEGER NOT NULL,
    decided_by    TEXT,
    decided_at    INTEGER,
    decision_note TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    hlc           TEXT NOT NULL,
    origin_device TEXT NOT NULL,
    CHECK (period_end >= period_start)
);

CREATE INDEX timesheet_submission_user ON timesheet_submission (user_id, period_start) WHERE deleted_at IS NULL;
