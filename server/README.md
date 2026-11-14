# Zeima API

Sync, organizations and licensing for the desktop app (SPEC §7, PLAN Phase 5). Fastify 5 + TypeBox, Keycloak for sign-in. Node 24 runs the TypeScript sources directly; there is no build step.

```sh
docker compose -f deploy/compose.dev.yml up -d   # Postgres, Keycloak (realm zeima), Mailpit
cp server/.env.example server/.env
bun run --cwd server dev                          # http://localhost:3000/docs
```

| Command | |
|---|---|
| `bun run --cwd server test` | Vitest, incl. the contract check |
| `bun run --cwd server typecheck` | `tsc --noEmit` |
| `bun run --cwd server openapi:export` | Rewrites `packages/api-contract/openapi.json` after route changes |
| `docker build -f server/Dockerfile .` | Production image (from the repository root) |

Configuration is environment variables only; `src/config.ts` lists them with defaults and fails at start-up with every problem listed.

**Plans and sync.** `GET /v1/entitlement` returns the caller's plan and an Ed25519-signed token the desktop app checks offline (SPEC §2). Outside production the server signs with the development key in `packages/shared-fixtures/entitlement/dev-key.json` (public on purpose), and debug builds of the app trust it. For local sync without billing, start the API with `DEFAULT_PLAN=pro`. In production set `ENTITLEMENT_SIGNING_KEY` (cloud) or give self-hosted installs a vendor-signed license token (`entitlement.token`), and build the app with `ZEIMA_ENTITLEMENT_KEYS`. `DEPLOYMENT_MODE=cloud` makes the sync routes require the `sync` feature.

**Organizations.** `src/modules/orgs` lets owners and admins manage an organization from the API (SPEC §5.9; who may do what is `packages/shared-fixtures/permissions.yaml`): create (`POST /v1/orgs`, the caller becomes the owner; needs the `org` feature on the cloud), list members (everyone, the own groups, or only oneself, by role), invite by email, change roles (owners only), deactivate or remove members, withdraw invitations, read the audit log and delete the organization. Keycloak is the source of truth, so every change calls the Keycloak Admin API first (`src/keycloak/admin.ts`, as the service account `zeima-api-admin`) and then updates the mirror tables. Set `KEYCLOAK_ADMIN_CLIENT_SECRET` (the compose files do); without it these routes answer 503 `org_admin_unavailable`. Each organization has a Keycloak Organization plus the groups `/orgs/<id>/roles/<role>` and `/orgs/<id>/groups/…`; the token's `groups` claim gives the role. After an organization is created or an invitation accepted the person needs a fresh token to see it. An invitation holds the role; it is applied when the invitee signs in with a **verified** address (`acceptInvitations`, called from the identity plugin). Invitations count against the plan's seats (`409 seat_limit_reached`). People who are in several organizations cannot be deactivated (that disables the whole account); remove them from the organization instead.

**Shared workspaces, policies, approvals, presence, locations, meetings.** Every organization has one shared workspace that syncs like a personal one (`GET /v1/workspaces`, `/v1/sync/push`, `/v1/sync/pull`). What each role may write and see per table is `src/modules/sync/orgrules.ts`, from `permissions.yaml`: shared data (projects, tags, cost centers, offices, policies) by `*.write` rights; a person's own rows (time entries, absences, schedules, locations, timesheets, meeting answers) by their owner; managers' view of entries follows the owner's policy (`full`, `hide_notes`, `totals_only`); members-only projects and meetings reach only the people they concern. Approving a timesheet locks the person's entries of the period on the server. Policies (`org_policy` rows: project or tag required on finished entries, manager visibility, presence visibility, weekly target, auto-pause suggestion) are the organization's, made stricter by each department. Presence (`PUT/GET /v1/orgs/:id/presence`) is in memory and tied to a connected device, so it works with one API process. Free/busy for planning meetings is `POST /v1/orgs/:id/freebusy`. The member and department copy is checked against Keycloak nightly (`RECONCILE_INTERVAL_HOURS`), on demand and on signed events (`KEYCLOAK_WEBHOOK_SECRET`); see `docs/keycloak.md`. On the cloud every organization route needs a plan with the `org` feature.

**Cloud and self-hosted.** `DEPLOYMENT_MODE=cloud` registers the billing routes and requires the `sync` feature on the sync routes; `self_hosted` (default) registers `PUT/GET /v1/license` for installing a vendor-signed license (role `platform-admin`) and does not enforce plans on the API (the app checks the license). Billing is configured with `BILLING_WEBHOOK_SECRET`, `BILLING_STORE_URL` and `BILLING_PLANS` (JSON list of `{key, plan, variantId, checkoutId}`); point Lemon Squeezy's webhook at `POST /v1/billing/webhook`. `MIN_CLIENT_VERSION` makes older apps ask for an update. The API document includes the routes of both modes.

**Conventions**
- Routes live in `src/modules/<feature>/routes.ts` and declare `tags`, `summary` and response schemas for every status. A test enforces it.
- A route is protected by `security: SECURITY` in its schema; the auth plugin enforces exactly what the docs show. `request.user` holds the user and their org memberships; `X-Org-Id` picks the active org (`request.orgId`).
- Errors are thrown as `HttpProblem(status, code, detail)` and sent as `application/problem+json`. Use `problems(401, 403, …)` for their response schemas.
