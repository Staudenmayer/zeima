-- Row-level security: the tenant boundary.
--
-- Migrations and server-side jobs run as the table owner, which RLS does not
-- apply to (no FORCE). Request handlers run `SET LOCAL ROLE zeima_app` and set
-- `app.user_id` / `app.org_ids` from the verified token (src/db/index.ts), so
-- a query can only see what the caller's identity reaches. Finer rules
-- (a manager sees their group, a member only their own entries) are the
-- `can()` policy in src/plugins/authz.ts; this layer stops cross-tenant leaks
-- even when a handler forgets to check.
--
-- zeima_app may only read identity data (organizations, the Keycloak mirror,
-- entitlements); those are written by the owner role.

-- Roles belong to the whole cluster, so two databases (or two API replicas)
-- migrating at the same moment can both find the role missing and both create
-- it. The loser's error is expected and harmless.
DO $$
BEGIN
  CREATE ROLE zeima_app NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
  NULL;
END $$;
--> statement-breakpoint
-- Lets the owner `SET ROLE zeima_app`. A superuser needs no membership; for
-- others the grant is cluster-wide too, so a collision is retried once.
DO $$
BEGIN
  IF NOT pg_has_role(CURRENT_USER, 'zeima_app', 'MEMBER') THEN
    BEGIN
      GRANT zeima_app TO CURRENT_USER;
    EXCEPTION WHEN OTHERS THEN
      PERFORM pg_sleep(0.2);
      IF NOT pg_has_role(CURRENT_USER, 'zeima_app', 'MEMBER') THEN
        GRANT zeima_app TO CURRENT_USER;
      END IF;
    END;
  END IF;
END $$;
--> statement-breakpoint
GRANT USAGE ON SCHEMA public TO zeima_app;
--> statement-breakpoint

CREATE FUNCTION app_user_id() RETURNS uuid
  LANGUAGE sql STABLE
  AS $$ SELECT nullif(current_setting('app.user_id', true), '')::uuid $$;
--> statement-breakpoint
CREATE FUNCTION app_org_ids() RETURNS uuid[]
  LANGUAGE sql STABLE
  AS $$ SELECT coalesce(string_to_array(nullif(current_setting('app.org_ids', true), ''), ',')::uuid[], '{}') $$;
--> statement-breakpoint

-- Identity data: read only for requests.
GRANT SELECT ON organization, kc_user, kc_group, kc_group_member, kc_org_member, entitlement TO zeima_app;
--> statement-breakpoint
ALTER TABLE organization ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE kc_user ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE kc_group ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE kc_group_member ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE kc_org_member ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE entitlement ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
CREATE POLICY org_read ON organization FOR SELECT TO zeima_app USING (id = ANY (app_org_ids()));
--> statement-breakpoint
CREATE POLICY org_member_read ON kc_org_member FOR SELECT TO zeima_app USING (org_id = ANY (app_org_ids()));
--> statement-breakpoint
CREATE POLICY group_read ON kc_group FOR SELECT TO zeima_app USING (org_id = ANY (app_org_ids()));
--> statement-breakpoint
CREATE POLICY group_member_read ON kc_group_member FOR SELECT TO zeima_app
  USING (group_id IN (SELECT id FROM kc_group));
--> statement-breakpoint
CREATE POLICY user_read ON kc_user FOR SELECT TO zeima_app
  USING (id = app_user_id() OR id IN (SELECT user_id FROM kc_org_member));
--> statement-breakpoint
CREATE POLICY entitlement_read ON entitlement FOR SELECT TO zeima_app
  USING (user_id = app_user_id() OR org_id = ANY (app_org_ids()));
--> statement-breakpoint

-- A user's devices.
GRANT SELECT, INSERT, UPDATE, DELETE ON device TO zeima_app;
--> statement-breakpoint
ALTER TABLE device ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
CREATE POLICY device_own ON device TO zeima_app
  USING (user_id = app_user_id()) WITH CHECK (user_id = app_user_id());
--> statement-breakpoint

-- Audit log: readable per org, append only.
GRANT SELECT, INSERT ON audit_log TO zeima_app;
--> statement-breakpoint
GRANT USAGE ON SEQUENCE audit_log_id_seq TO zeima_app;
--> statement-breakpoint
ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
CREATE POLICY audit_read ON audit_log FOR SELECT TO zeima_app USING (org_id = ANY (app_org_ids()));
--> statement-breakpoint
CREATE POLICY audit_append ON audit_log FOR INSERT TO zeima_app WITH CHECK (org_id = ANY (app_org_ids()));
--> statement-breakpoint

-- Synced data: a workspace belongs to one user or one organization.
GRANT SELECT, INSERT, UPDATE, DELETE ON workspace, project, tag, time_entry, time_entry_tag, pause TO zeima_app;
--> statement-breakpoint
ALTER TABLE workspace ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE project ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE tag ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE time_entry ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE time_entry_tag ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
ALTER TABLE pause ENABLE ROW LEVEL SECURITY;
--> statement-breakpoint
CREATE POLICY workspace_access ON workspace TO zeima_app
  USING (owner_id = app_user_id() OR org_id = ANY (app_org_ids()))
  WITH CHECK (owner_id = app_user_id() OR org_id = ANY (app_org_ids()));
--> statement-breakpoint
CREATE POLICY project_access ON project TO zeima_app
  USING (workspace_id IN (SELECT id FROM workspace))
  WITH CHECK (workspace_id IN (SELECT id FROM workspace));
--> statement-breakpoint
CREATE POLICY tag_access ON tag TO zeima_app
  USING (workspace_id IN (SELECT id FROM workspace))
  WITH CHECK (workspace_id IN (SELECT id FROM workspace));
--> statement-breakpoint
CREATE POLICY time_entry_access ON time_entry TO zeima_app
  USING (workspace_id IN (SELECT id FROM workspace))
  WITH CHECK (workspace_id IN (SELECT id FROM workspace));
--> statement-breakpoint
CREATE POLICY time_entry_tag_access ON time_entry_tag TO zeima_app
  USING (entry_id IN (SELECT id FROM time_entry))
  WITH CHECK (entry_id IN (SELECT id FROM time_entry));
--> statement-breakpoint
CREATE POLICY pause_access ON pause TO zeima_app
  USING (workspace_id IN (SELECT id FROM workspace))
  WITH CHECK (workspace_id IN (SELECT id FROM workspace));
