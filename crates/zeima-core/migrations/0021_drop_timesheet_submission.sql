-- Timesheets are not handed in for approval any more; only absences are
-- (the entry lock flag stays, managers still change locked entries).
DELETE FROM change_log WHERE table_name = 'timesheet_submission';
DROP TABLE IF EXISTS timesheet_submission;
