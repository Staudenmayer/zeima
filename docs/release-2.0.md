# Zeima 2.0 — Team and Organization

What is new, what changes for existing installations, and what to check
before and after the release. (Written for whoever runs the release; the
user-facing text for the release page is the first section.)

## For the release page

**Zeima 2.0 adds organizations.** Your own time tracking, work time and sync
work as before. New, once you are signed in to a server and belong to an
organization:

- **Organizations**: owners and admins invite people by email, give them roles
  (owner, admin, manager, member), group them into departments and see an
  activity log.
- **Shared workspace**: switch between your own workspace and your
  organization's. Projects, tags, cost centers and offices are shared; your
  entries are yours, and managers see what their role allows. Projects can be
  limited to chosen people and have a project manager.
- **Rules**: organizations (and departments) can require a project or tag on
  every finished entry, limit what managers see of entries, hide who is online,
  set a weekly target and suggest an auto-pause time.
- **Approvals**: submit a timesheet for a period; a manager or admin approves
  or rejects it, which locks the entries. Absences in an organization are
  requests that someone approves. Desktop notifications tell you what waits.
- **Team**: tracked and billable time per person, project, tag and cost
  center, per department, with people far from the weekly target flagged.
- **Online**: see who is online, tracking, in a meeting or away (you choose
  whether your project is shown).
- **Locations**: where everyone works each day (office, home, travel), with
  offices that have room for so many, a usual week and a month view.
- **Meetings**: schedule with colleagues, see who is busy, answer yes/maybe/no,
  repeat daily/weekly/monthly, save as `.ics`; meetings appear in the
  timesheet and offer to be tracked when they end.
- **Import** from Toggl and Clockify CSV exports (Settings → Data).
- **ICS calendar addon**: shows the events of any calendar feed in the
  timesheet.

## For the person running a server

- **Upgrade**: the API migrates its database on start (migrations 0006–0007).
  The app migrates its local database on start (0012–0020). Back up first
  (the app does so automatically before upgrades; for the server use
  `pg_dump` of the `zeima` database).
- **Keycloak**: apply the realm file again (`deploy/keycloak/apply.sh`): the
  API's service account needs `manage-organizations` and `view-organizations`.
  Without `KEYCLOAK_ADMIN_CLIENT_SECRET` the organization routes answer 503.
  Keycloak needs working SMTP to send invitations. With Organizations in use,
  the sign-in page asks for the address first and the password second.
- **New settings**: `KEYCLOAK_ADMIN_CLIENT_SECRET` (the compose files set it
  from `API_ADMIN_SECRET`), `KEYCLOAK_WEBHOOK_SECRET` (optional),
  `RECONCILE_INTERVAL_HOURS` (default 24). See `server/README.md` and
  `docs/keycloak.md`.
- **Plans**: on the cloud the organization routes need a plan with the `org`
  feature (Team, Enterprise). Self-hosted installs rely on the license.
- **Compatibility**: apps 1.x still sync their personal workspace with a 2.0
  server. Organization features need the 2.0 app. A 2.0 app against an older
  server works as before for the personal workspace; organization pages ask
  for a newer server.
- **One API process** for now: presence is kept in memory.

## Checks before tagging

1. `cargo test --workspace --all-features`, `cargo clippy … -D warnings`,
   `bun run --cwd server test`, `bunx vitest run`, `bun run build` — all
   green (CI does this).
2. Against a real stack: `deploy/e2e/smoke.mjs` and `deploy/e2e/org-smoke.mjs`.
3. By hand with two apps (ideally Windows or macOS too) on one organization:
   invite and join, switch workspace, track time in both, submit and approve a
   timesheet (entries lock), request and approve an absence, create a meeting
   and answer it from the other app, look at Online, Locations and Team.
4. Release pipeline: the updater key password secret matches the key
   (`node scripts/verify-updater-key.mjs`), then tag `v2.0.0-rc.1` first.

## Known limits

See "State for 2.0" in `docs/PLAN.md`: no calendar write-through for meetings
(`.ics` instead), no shipped Keycloak event listener (nightly check instead),
presence on one API instance, auto-pause per department is a suggestion.
