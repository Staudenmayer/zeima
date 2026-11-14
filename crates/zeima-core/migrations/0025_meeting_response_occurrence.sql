-- An answer to one date of a repeating meeting: `occurrence_at` is the start of
-- that date (unix ms). Without it the answer is for the whole series. A person has
-- one live answer per meeting and date, and one for the series.
ALTER TABLE meeting_response ADD COLUMN occurrence_at INTEGER;

DROP INDEX meeting_response_person;
CREATE UNIQUE INDEX meeting_response_person
    ON meeting_response (workspace_id, meeting_id, user_id, COALESCE(occurrence_at, -1))
    WHERE deleted_at IS NULL;
