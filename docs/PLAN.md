# Time — Implementation Plan

> Companion to [SPEC.md](./SPEC.md). Status: Draft v0.1 · 2026-10-04
> Each phase ends in a usable, shippable state. Phases 0–5 are the **Free/local** product. Phases 6–8 add the paid tiers.

## Current State (baseline)

| Area | State |
|---|---|
| Shell | Tauri 2 tray app: show/hide/quit menu, `Ctrl+Shift+Space` toggle, close→hide, window-state plugin |
| Frontend | Vue 3, Pinia, vue-router auto-routes, shadcn-vue (sidebar, card, chart…), Tailwind 4, unovis, vue-i18n (de/en) |
| Data | `tauri-plugin-sql` with **postgres** feature (no schema yet) |
| Pages | `index.vue` demo (counter + BarChart) |
| Cleanup needed | `src/layouts/default copy.vue`, product name `tray-app`, window title "My App", `csp: null`, no git repo |

---

## Phase 0 — Foundation (≈ 1 week)

Goal: the project skeleton every later phase builds on.

- [ ] `git init`, first commit of the current state; add `.editorconfig`, rustfmt and clippy config
- [ ] Rename the product: `productName`, `identifier`, window title, `package.json` name, Cargo package
- [ ] Convert to a **Cargo workspace** (root `Cargo.toml` with members `src-tauri`, `crates/time-core`) and **bun workspaces** (root app, later `server/`, `packages/*`)
- [ ] `crates/time-core`: `sqlx` (sqlite, runtime-tokio), `uuid` (v7), `chrono` + `chrono-tz`, `thiserror`, `serde`
  - [ ] Keep pure logic (models, work-time calc, country profiles, rules) free of I/O; put `sqlx`/tokio behind a `db` cargo feature so the crate compiles to WASM for the server later
  - [ ] DB bootstrap: open `app_data_dir/time.db`, WAL mode, `foreign_keys=ON`, `sqlx::migrate!`
  - [ ] Common columns helper + `ChangeLog` writes inside a repository transaction wrapper (`Tx::upsert`, `Tx::soft_delete`)
  - [ ] HLC implementation + per-install `device_id`
- [ ] Remove `tauri-plugin-sql` (Rust + JS + capability `sql:default`)
- [ ] Add **tauri-specta** and generate `src/bindings.ts` on debug build; one `ping` command end to end
- [ ] Turn on the CSP and tighten `capabilities/default.json`
- [ ] Frontend scaffolding: `src/stores/`, `src/composables/`, sidebar nav entries per SPEC §8.1 (placeholder pages), i18n keys
- [ ] CI: GitHub Actions running `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `vue-tsc`, `vitest`
- [ ] `docs/adr/0001-local-first-sqlite-rust-owned-db.md`, `docs/adr/0002-fastify-openapi-keycloak-server.md`

**Done when:** the app starts, creates `time.db` with migrations applied, and the frontend calls a typed command.

---

## Phase 1 — Core Tracking MVP (≈ 2–3 weeks)

Goal: track time on projects with tags from the tray. A daily-driver for one user.

**Backend (`time-core`)**
- [ ] Migration `0001_core`: `workspace`, `user`, `project`, `tag`, `time_entry`, `time_entry_tag`, `pause`, `setting`, `change_log`
- [ ] Bootstrap a local personal workspace and local user on first run
- [ ] Repositories + validation: Project (unique alias), Tag (global/project-scoped), TimeEntry
- [ ] **Timer engine** (`TimerService`): start / stop / switch / pause / resume. Enforce the single running entry. Write a heartbeat every 30 s; recover the running entry after a crash.
- [ ] Commands: `project_*`, `tag_*`, `entry_*`, `timer_*`; events `timer://tick` (1 s, only while the window is visible), `entry://changed`

**Shell (`src-tauri`)**
- [ ] Dynamic tray: state icon (idle/tracking/paused), tooltip with elapsed time, current activity, recent 5 projects, pause/resume, stop
- [ ] **Quick-switch window** + `Ctrl+Shift+T`; configurable shortcuts stored in `setting`
- [ ] Autostart on login (`tauri-plugin-autostart`), single instance (`tauri-plugin-single-instance`), notifications (`tauri-plugin-notification`)

**Frontend**
- [ ] Stores: `timer`, `projects`, `tags`, `entries`
- [ ] Pages: `projects/` (list, create/edit with alias, color, budget, project tags), `tags/`, `timesheet/` (day list + week view, manual add/edit/delete, split/duplicate)
- [ ] Quick-switch palette: fuzzy search, `#alias @tag note` parser (unit tested)
- [ ] Header: running-timer pill with stop/pause

**Done when:** one user tracks a full workday from the tray and quick switch, edits entries, and data survives restart and crash.

---

## Phase 2 — Work Hours & Auto-Pause (≈ 2–3 weeks)

- [ ] Migration `0002_worktime`: `work_schedule`, `auto_pause_rule`, `absence_type`, `absence`, `holiday`, `balance_adjustment`, `daily_summary`
- [ ] **Work-time calculator** in `time-core` (pure functions; heavy unit and property tests): interval merge, day clipping in the local tz, DST, target from schedule/holidays/absences, balance
- [ ] `daily_summary` recompute on change (dirty-day queue)
- [ ] **Country profile engine** (SPEC §5.13): TOML profile format + JSON Schema, `extends` / region inheritance, rule-based holiday generator (fixed, Easter offset, nth weekday, substitutes, validity ranges), profile versioning
- [ ] Profiles `AT.toml` (primary), `DE.toml`, `CH.toml`, each with a `*.fixtures.toml` (3 years of holidays + break/balance sample days); a test that loads every profile and checks its fixtures
- [ ] Migration adds `jurisdiction_selection`; onboarding step "Country & region" (suggested from the OS locale); settings page to change it
- [ ] Custom holidays on top of profile holidays
- [ ] `docs/country-profiles.md`: how to add a country (copy template → fill → fixtures → PR)
- [ ] **OS watchers** in `src-tauri/src/os/`: idle seconds (`user-idle`), lock/unlock, suspend/resume. One module per platform behind a trait; Linux via logind D-Bus (`zbus`).
- [ ] **Auto-pause rule engine** (simulated clock for tests): lunch fixed/detected, idle prompt, lock/suspend, night stop, max-day / min-rest warning. **All thresholds are read from the resolved country profile + overrides; none are hardcoded.**
- [ ] Prompt window (idle return, forgotten timer, morning check-in) with Undo actions
- [ ] Pages: `settings/work-hours` (schedule editor with validity ranges), `settings/auto-pause`, `absences/` (calendar + list, vacation remaining)

**Done when:** balance and overtime match a hand-computed reference month (fixture test), and every auto-pause can be seen and undone.

---

## Phase 3 — Dashboard, Reports, Cost Centers (≈ 2 weeks)

- [ ] Migration `0003_cost_centers`: `cost_center`, `cost_center_assignment`; resolution logic project → user/group default
- [ ] Report query engine in `time-core`: filter + group-by (project, tag, cost center, day/week/month, billable) over `time_entry` + `daily_summary`
- [ ] **Dashboard** (`pages/index.vue`, replaces the demo): Today, Week (rework `BarChart.vue`), Balance, Projects/budget burn, Cost centers. Widget layout is persisted.
- [ ] Project budget warnings (notification at warn % and 100 %)
- [ ] Pages: `reports/`, `cost-centers/`
- [ ] Exports: CSV, XLSX (`rust_xlsxwriter`), monthly PDF timesheet
- [ ] Full data export (JSON) + automatic daily DB backup (keep 14)

**Done when:** a month can be exported to CSV/XLSX/PDF and the dashboard loads in < 300 ms for one year of seeded data.

---

## Phase 4 — Calendar Integration (personal) (≈ 2 weeks)

- [ ] `keyring` secret store abstraction
- [ ] OAuth 2 PKCE with loopback redirect (`tauri-plugin-oauth` or a small local listener) for **Microsoft Graph** and **Google**
- [ ] Migration `0004_calendar`: `calendar_account`, `calendar_event`, `calendar_mapping`
- [ ] Incremental sync worker (delta / syncToken), 5-min interval + on focus
- [ ] ICS URL and CalDAV (read-only)
- [ ] Timeline shows calendar events; "track this meeting"; mapping rules → auto-entries (opt-in)
- [ ] Absence → OOF event (opt-in)

**Done when:** Outlook events appear in the day view and can become entries in one click.

---

## Phase 5 — Addon System v1 (≈ 2 weeks)

- [ ] Internal **event bus** in `time-core` (all domain events from Phases 1–4)
- [ ] `crates/time-addon-sdk` (Rust) + manifest schema (`addon.toml`)
- [ ] Extism host in `src-tauri`: load, permission check against the manifest, per-addon KV store, `http_fetch` domain allowlist, time and memory limits
- [ ] Hooks: `entry.*`, `day.closed`, `report.export`, `quickswitch.suggest`, `schedule.tick`
- [ ] Declarative settings form (JSON schema → shadcn form)
- [ ] Page: `addons/` (install from file, enable/disable, permissions consent dialog)
- [ ] First-party example addons: **configurable CSV export format** (proves the `report.export` hook), **GitHub/Jira issue linker**
- [ ] `docs/addons.md` developer guide

**Done when:** a third party can build an export-format addon from the docs alone.

> 🎯 **Release 1.0 (Free)**: Phases 0–5, plus signed builds and the auto-updater (`tauri-plugin-updater`).

---

## Phase 6 — Server, Accounts, Sync, Licensing (≈ 5–7 weeks)

> Optional early spike (in parallel with Phase 2–3): steps 6a + 6b only, to validate the desktop OIDC login flow early.

**6a. Keycloak setup**
- [ ] `deploy/keycloak/realm-time.json`: realm `time`, clients `time-desktop`, `time-api`, `time-api-admin`, `time-swagger` (SPEC §7.2), audience mapper, `orgs` claim mapper, password/MFA/brute-force policy, Organizations feature on
- [ ] Self-registration + Google/Microsoft social login (cloud)
- [ ] Event-listener extension → HMAC-signed webhook to the API
- [ ] Login theme (de/en, app branding)
- [ ] `keycloak-config-cli` step in CI/CD to apply realm changes

**6b. Fastify API skeleton**
- [ ] `server/` package: Fastify 5, TypeBox type provider, `@fastify/env` config, helmet/cors/rate-limit/under-pressure, Pino, problem+json error handler
- [ ] `@fastify/swagger` + `@fastify/swagger-ui` at `/docs` with the Keycloak OAuth2 security scheme; `bun run openapi:export` writes `packages/api-contract/openapi.json`
- [ ] Auth plugin: `jose` JWKS verification (`iss`, `aud`, `exp`), `request.user` with `orgs` claim, `X-Org-Id` handling
- [ ] Authz plugin: `can()` driven by `packages/shared-fixtures/permissions.yaml` + a matrix test
- [ ] Drizzle schema + migrations mirroring the client schema + `organization`, `device`, `entitlement`, `kc_user`, `kc_group`, `kc_group_member`, `kc_org_member`, `audit_log`; RLS policies
- [ ] `/healthz`, `/readyz`, `/metrics`; Dockerfile (Node 24 slim, non-root)
- [ ] CI: lint, typecheck, Vitest, Testcontainers (Postgres + Keycloak), `oasdiff breaking`, route schema lint

**6c. Shared logic & contracts**
- [ ] `crates/time-core-wasm` (wasm-bindgen, `--target nodejs`) → `@time/core-wasm`: work-time calc, balances, holidays, country profiles
- [ ] `packages/shared-fixtures/sync/*.json` test vectors; Rust and TS merge implementations both pass them
- [ ] `crates/time-api-client` generated with `progenitor`; Vue types via `openapi-typescript` + `openapi-fetch` (fetch = `@tauri-apps/plugin-http`)

**6d. Desktop auth**
- [ ] OIDC Auth Code + PKCE in the system browser, loopback redirect (deep-link fallback); `offline_access` refresh token stored in the keychain
- [ ] `auth_login`, `auth_logout`, `auth_access_token` commands; silent refresh; "Signed in as…" in `NavUser.vue`
- [ ] Server URL setting + `.well-known/time-server` discovery (API URL + issuer) + installer preset

**6e. Sync**
- [ ] `/v1/sync/push`, `/v1/sync/pull` (cursor), field-level LWW by HLC, server rule overrides
- [ ] Client sync worker in Rust: push the outbox, pull, apply, conflict cards UI
- [ ] WebSocket "changes available" channel
- [ ] Deterministic multi-device convergence tests

**6f. Licensing, billing, deployment**
- [ ] **Entitlements**: signed token, offline grace, `has_feature()` gating in the client and server
- [ ] Billing (cloud only): Paddle / Lemon Squeezy (or Stripe) checkout + webhooks → entitlements; account page in app
- [ ] `DEPLOYMENT_MODE = cloud | self_hosted`; billing/signup modules disabled in self-hosted mode; offline license file import in the admin UI
- [ ] `deploy/docker-compose.yml` (api + keycloak + postgres + caddy). A CI job boots it and runs login + sync E2E against it, so self-hosting can't silently break.
- [ ] Cloud deployment: EU hosting, managed Postgres for both DBs, Keycloak in HA mode (2+ nodes), backups, monitoring (OpenTelemetry → Grafana or Sentry)
- [ ] API versioning (`/v1`) + client/server compatibility check

> 🎯 **Release 1.1 (Pro)**: multi-device sync for individuals.

---

## Phase 7 — Organization Features (≈ 6–8 weeks)

Order is by dependency:

1. **User management**: org creation (Keycloak Organization + role groups), invitations, role assignment, hierarchical department groups. All of it goes through API admin endpoints that call the Keycloak Admin API, then the mirror tables. Admin pages (`admin/users`, `admin/groups`, `admin/policies`), seat counting against the entitlement, audit log.
2. **Shared data**: org projects (visibility, members, project manager role), org cost centers/tags, org work schedules and auto-pause policies per group
3. **Approvals**: timesheet submission/approval → `locked` entries; absence requests/approval
4. **Online users**: server WebSocket presence hub, client presence reporter (based on timer state and the idle/lock watchers), privacy opt-in for project sharing, `team/online` panel + dashboard widget
5. **Work location**: `office`, `work_location` tables; weekly planner; morning tray prompt; `team/locations` grid (users × days) with office capacity
6. **Meetings**: scheduling UI with free/busy grid, write-through to the organizer's calendar (Graph/Google), RSVP sync, "track this meeting" prompt per attendee, RRULE recurrence
7. Manager dashboards: group totals, **project-level breakdown per member** (projects, tags, cost centers, entries), overtime outliers, pending approvals

> 🎯 **Release 2.0 (Team/Org)**: launches in the cloud first. The self-hosted Docker release follows once it has run on the cloud for one stable cycle (same version, same binary).

---

## Phase 8 — Enterprise & Hardening (ongoing)

- SSO per org: Keycloak identity providers (Entra ID, Google Workspace, generic SAML/OIDC) linked to the org's email domain; LDAP/AD user federation for self-hosted; SCIM provisioning via a Keycloak SCIM extension; self-service SSO setup page in the admin UI
- Self-hosted polish: Helm chart, air-gapped install bundle, admin docs, upgrade guide, mountable extra country profiles
- Addon registry with signing; org addon allowlist
- Timeular BLE tracker addon; office Wi-Fi location addon
- Data import (Toggl, Clockify, Timeular CSV)
- Security review / penetration test; GDPR DPA, works-council documentation pack
- Mobile companion (Tauri mobile) exploration

---

## Cross-Cutting Rules (apply in every phase)

- Every write goes through `time-core` repositories, which write the `change_log` in the same transaction. No exceptions.
- Every synced table gets the common columns (SPEC §4) and soft delete from its first migration. Do not retrofit them later.
- Feature gates use `has_feature(...)`. In Free, org-only UI is hidden, not shown as broken.
- New user-facing strings go into **both** `de.ts` and `en.ts`.
- Work-time math changes need a fixture test.
- **No country-specific values in code.** Legal numbers, holidays and defaults belong in a country profile.
- **No cloud-only dependencies in the server.** Every feature must work with Postgres + Keycloak + config alone, or be explicitly cloud-only behind `DEPLOYMENT_MODE`.
- **Schema-first API.** Every Fastify route has TypeBox request and response schemas. `openapi.json` is regenerated and committed with the change; breaking changes need a new API version.
- **Identity data is written to Keycloak first.** The API never creates users or groups only in its own DB.
- An ADR in `docs/adr/` for every decision that is hard to reverse.

## Risks

| Risk | Mitigation |
|---|---|
| Idle/lock detection on Wayland is inconsistent | Abstract behind a trait; fall back to logind lock events; document limits |
| Country profiles wrong or outdated (legal liability) | Profiles are versioned, cite sources, ship with fixtures, and carry a not-legal-advice disclaimer; orgs can override them |
| Self-hosted drifts from cloud | Same images and realm file, self-hosted compose stack runs in CI, API compatibility range |
| Keycloak operational complexity (upgrades, HA, theme breakage) | Pin a supported version range, realm-as-code with `keycloak-config-cli`, Testcontainers tests on every Keycloak bump, compose E2E in CI |
| Mirror tables drift from Keycloak | Three update paths (token upsert, event webhook, nightly reconcile) + a drift metric/alert |
| Logic drift between Rust client and TS server | Domain math in one place (`time-core` → WASM); shared JSON test vectors for sync merge and permissions |
| Sync bugs corrupt data | Tombstones, change log, server-side history table, convergence tests, local backups |
| Scope creep before revenue | Ship 1.0 Free early; validate demand for Org before Phase 7 |
| Works-council or privacy objections | Privacy-first defaults (SPEC §12); documented visibility model |

## Immediate Next Steps

1. Answer the open questions in SPEC §14 (the missing feature bullet).
2. Phase 0, tasks 1–5: git init, rename, Cargo workspace, `time-core` with SQLite + migrations, remove `tauri-plugin-sql`.
