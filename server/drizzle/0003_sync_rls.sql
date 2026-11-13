-- Sync state follows the workspace: a caller reaches the rows of the
-- workspaces they can see (see 0001_rls.sql).
GRANT SELECT, INSERT, UPDATE ON sync_row TO zeima_app;
--> statement-breakpoint
GRANT USAGE ON SEQUENCE sync_seq TO zeima_app;
--> statement-breakpoint
ALTER TABLE sync_row ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
CREATE POLICY sync_row_access ON sync_row TO zeima_app
  USING (workspace_id IN (SELECT id FROM workspace))
  WITH CHECK (workspace_id IN (SELECT id FROM workspace));
