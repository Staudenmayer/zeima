# Zeima — Implementation Plan

> Companion to [SPEC.md](./SPEC.md). Status: Draft v0.1 · 2026-10-04
> Each phase ends in a usable, shippable state. Phases 0–4 are the **Free/local** product. Phases 5–7 add the paid tiers. Integrations such as calendars are addons, not built-in features.

## Current State

Phases 0–2 are implemented (2026-10-04), Phase 3 on 2026-10-10; Phase 4 is in progress (addon system and the calendar host APIs done 2026-10-13; the calendar addons are postponed to the end). Phase 5 started 2026-10-13: Keycloak realm, dev stack and the API skeleton with authentication. Notes on where the implementation differs from the original checklist are in *italics*.

---

## Phase 0 — Foundation ✅

Goal: the project skeleton every later phase builds on.

- [x] `git init`, first commit of the current state; add `.editorconfig`, rustfmt and clippy config
- [x] Rename the product: `productName` (Zeima), `identifier` (`com.zeima.zeima`), window title, `package.json` name, Cargo package (`zeima`)
- [x] Convert to a **Cargo workspace** (root `Cargo.toml` with members `src-tauri`, `crates/zeima-core`)
- [x] **bun workspaces**: *done in Phase 5 (2026-10-13): root app, `server`, `packages/*`*
- [x] `crates/zeima-core`: `sqlx` 0.9 (sqlite, bundled), `uuid` (v7), `chrono` + `chrono-tz`, `thiserror`, `serde`
  - [x] Pure logic free of I/O; `sqlx`/tokio behind the `db` feature (CI checks the crate builds without it)
  - [x] DB bootstrap: `app_data_dir/zeima.db`, WAL mode, `foreign_keys=ON`, `sqlx::migrate!`
  - [x] `Core::begin()` → `WriteTx` stamps rows and writes `change_log` in the same transaction (`log_upsert`, `log_delete`, `soft_delete`)
  - [x] HLC implementation + per-install `device_id`
- [x] Remove `tauri-plugin-sql` (Rust + JS + capability `sql:default`)
- [x] **tauri-specta** generates `src/bindings.ts` on debug start and in `cargo test`; *the full command set replaced the planned `ping` command*
- [x] CSP on, capabilities reduced to `core:default` + `opener:default` for `main` and `quickswitch`; *remote Google Fonts import removed (bundled Roboto instead)*
- [x] Frontend scaffolding: `src/stores/`, `src/composables/`, sidebar nav per SPEC §8.1 with placeholder pages (reports, absences, calendar, addons; *the calendar page was removed again when calendars became addons*), i18n keys (de/en, German typed against English)
- [x] CI: `.github/workflows/ci.yml` (fmt, clippy `-D warnings`, `cargo test` on Linux/Windows/macOS, bindings drift check, `vue-tsc`, `vitest`, vite build)
- [x] `docs/adr/0001-local-first-sqlite-rust-owned-db.md`, `docs/adr/0002-fastify-openapi-keycloak-server.md`

**Done when:** the app starts, creates `zeima.db` with migrations applied, and the frontend calls a typed command. ✅

---

## Phase 1 — Core Tracking MVP ✅

Goal: track time on projects with tags from the tray. A daily-driver for one user.

**Backend (`zeima-core`)**
- [x] Migrations `0001_meta` (`local_meta`, `change_log`) and `0002_core` (`workspace`, `app_user`, `project`, `tag`, `time_entry`, `time_entry_tag`, `pause`). *Device settings live in `local_meta`; a synced `setting` table is added when the first synced setting appears.*
- [x] Bootstrap a local personal workspace and local user on first run
- [x] Repositories + validation: Project (unique alias, case-insensitive; parent cycle check; delete blocked when entries exist → archive), Tag (global/project-scoped, one tag per group), TimeEntry (create/update/delete/split/duplicate, locked entries protected)
- [x] **Timer engine**: start / stop / switch (no gap) / pause / resume. Single running entry and single open pause enforced by unique partial indexes. Heartbeat every 30 s; the running entry survives a crash because it is stored as an open entry. *The "timer ran overnight" prompt that uses the heartbeat comes with Phase 2 prompts.*
- [x] Commands `project_*`, `tag_*`, `entry_*`, `timer_*`, `summaries`, `settings_*`; events `TimerChanged`, `DataChanged`, `QuickSwitchOpened`, `Navigate`. *No `timer://tick` event: the UI computes elapsed time from `startAt`, so no IPC traffic per second.*
- [x] 29 Rust tests (unit + repository/timer integration on in-memory SQLite)

**Shell (`src-tauri`)**
- [x] Dynamic tray: icon with green (tracking) / amber (paused) dot, tooltip and title with elapsed time, current activity, 5 recent projects (one click to switch), pause/resume, stop, quick switch, show/hide, quit; menu texts in de/en
- [x] **Quick-switch window** (frameless, always on top, hides on blur) + configurable global shortcuts (toggle window, quick switch, pause/resume), validated before saving
- [x] Autostart on login (starts minimized in the tray), single instance. *`tauri-plugin-notification` is deferred to Phase 2, where the first notifications (auto-pause undo, prompts) are needed.*

**Frontend**
- [x] Stores: `timer`, `projects`, `tags`, `settings`; *entries are loaded per view with the `useEntries` composable instead of a global store*
- [x] Pages: dashboard (today, week, current timer, hours per day chart, projects this week), `projects/` (list, create/edit with alias, color, client, parent, budget, status, project tags), `tags/`, `timesheet/` (day and week list, manual add/edit/delete, split, duplicate, continue), `settings/`
- [x] Quick-switch palette: fuzzy search, `#alias @tag note` parser (18 Vitest tests incl. date helpers)
- [x] Header: running-timer pill with pause/resume/stop and quick switch

**Done when:** one user tracks a full workday from the tray and quick switch, edits entries, and data survives restart and crash. *Verified: app starts, bootstraps `zeima.db`; a full manual workday test is still to be done by a person.*

---

## Phase 2 — Work Hours & Auto-Pause ✅

- [x] Migration `0003_worktime`: `jurisdiction_selection`, `work_schedule`, `absence_type`, `absence`, `custom_holiday`, `balance_adjustment`, `work_settings` (incl. auto-pause config), `daily_summary`; `pause` gains `followup_entry_id`, `applied`. *Auto-pause rules are one config object in `work_settings`, not an `auto_pause_rule` table.*
- [x] **Work-time calculator** (`zeima-core/src/worktime.rs`, pure): interval merge, local-day clipping (DST-safe), break detection + deduction per profile, targets from schedules/holidays/absences (half days, comp time), trust-based model, balance, warnings (daily max, rest time, break). Reference month (Oct 2026, AT) checked against a hand calculation: **+4:00 h**.
- [x] `daily_summary` cache: past days are cached with the profile version; every write that affects a day deletes its rows (entries invalidate their date range + the next day for the rest check). A test compares cache and fresh computation after every kind of change.
- [x] **Country profile engine**: TOML format + generated JSON Schema (`profile.schema.json`, CI drift check), region add/remove, rule-based holidays (fixed, Western/Orthodox Easter offset, nth/last weekday, weekday before/after, validity years), profile versioning. *`extends` between countries is not implemented yet; regions cover current needs.*
- [x] Profiles `AT` (9 states), `DE` (16 states), `CH` (federal + ZH, BE) with fixtures (holidays for several years incl. regions, break thresholds). *Values written from the law texts; they need review by someone familiar with each country before release.*
- [x] Onboarding dialog "Where do you work?" (suggested from the OS locale) and a settings page to change it.
- [x] Custom (company) holidays on top of profile holidays.
- [x] `docs/country-profiles.md`
- [x] **OS watchers** (`src-tauri/src/os/`): Linux idle via GNOME Mutter, Wayland `ext-idle-notify-v1` (KDE/Sway/Hyprland) or freedesktop screensaver; lock via logind `LockedHint` + screensaver; Windows/macOS idle via `user-idle`. Suspend is detected on all platforms from wall-clock jumps. *Windows/macOS lock state is not read directly; a locked screen shows up as idle time.*
- [x] **Auto-pause engine** (`zeima-core/src/autopause.rs` + `away.rs`): idle (prompt by default), lock/sleep/app closed (exclude by default), lunch detection, night stop for forgotten timers (also on startup via heartbeat), daily-max notification, morning check-in. All legal numbers come from the profile.
- [x] Prompt window (bottom-right, does not take focus): every auto-pause can be accepted, undone ("it was work", merges the entry back), or assigned to another project; morning check-in with recent projects.
- [x] Pages: `settings/work-hours` (country, legal summary, schedules with validity dates, vacation days, break deduction, balance corrections), `settings/auto-pause`, `absences/` (vacation summary, list, types, holidays). Dashboard: today vs target, flexitime balance, week chart with targets. Timesheet: per-day target, difference, holiday, absence and warning badges.

**Done when:** balance and overtime match a hand-computed reference month (fixture test), and every auto-pause can be seen and undone. ✅ *(Live auto-pause behaviour on GNOME, X11, Windows and macOS still needs a manual test; it was verified on KDE Wayland.)*

---

## Phase 3 — Dashboard, Reports, Cost Centers ✅

- [x] Migration `0004_cost_centers` *(0003 was taken by work time)*: `cost_center` (code, name, parent, active, validity), `cost_center_assignment` (user default with validity; `group_id` reserved for the org edition), `cost_center_id` on `project` and `time_entry`. Resolution when an entry is written: explicit → project default → user default on the entry's date; inactive or expired defaults are skipped. Moving an entry to another project re-resolves it unless the cost center was picked explicitly.
- [x] Report query engine (`zeima-core/src/report.rs`, pure): filter by project, tag, cost center, billable; nested group-by over project, tag, cost center, billable, day/week/month (local days, DST-safe). *Sums clipped entry durations, so overlaps count per entry; tag rows count an entry once per tag. Reads `time_entry` only: `daily_summary` holds work-time days, which the monthly timesheet uses instead.*
- [x] **Dashboard**: widgets can be reordered and hidden (*stored per device, not synced*); new widgets for project budgets and cost centers this month. *`BarChart.vue` was already replaced by `WeekChart.vue` in Phase 2.*
- [x] Project budget warnings: notification at the warning threshold and at 100 %, once per threshold and budget period (checked after entry/project changes and every 30 s while tracking)
- [x] Pages: `reports/` (period presets, filters, two grouping levels, summary and entries views), `reports/timesheet` (printable monthly timesheet with signature lines), `cost-centers/`
- [x] Exports: CSV (entries or summary; `;` and decimal comma in German, UTF-8 BOM), XLSX (`rust_xlsxwriter`, summary + entries sheets). *Monthly PDF timesheet: printed from the webview via the system print dialog ("Save as PDF") instead of a Rust PDF library, so it reuses the app's fonts and translations.*
- [x] Full data export (JSON, Settings → Data) + automatic daily DB backup (`VACUUM INTO`, keep 14) and a start-up integrity check

**Done when:** a month can be exported to CSV/XLSX/PDF and the dashboard loads in < 300 ms for one year of seeded data. ✅ *(`tests/perf.rs`: 2,190 entries, dashboard queries ≈ 6 ms, one-year report ≈ 12 ms in release mode.)*

---

## Phase 4 — Addon System v1 (in progress)

- [x] Internal **event bus** in `zeima-core`: repositories queue `DomainEvent`s on the write transaction, published after commit (`entry.started/stopped/created/updated/deleted`); the app adds `day.closed` once per finished day (catching up to a week).
- [x] `crates/zeima-addon-sdk` (Rust, JSON contract shared with the host) + manifest schema (`addon.toml`, `crates/zeima-core/addon.schema.json`, CI drift check)
- [x] Extism host in `src-tauri` (`addons.rs`): load, permission check on every host call, per-addon KV store, `network` limited to `allowed_hosts`, 10 s per call (0.8 s for suggestions), 64 MB memory, no WASI. *Addon-created entries carry source `addon` and are not echoed back to the addon that made them.* ADR 0003.
- [x] Hooks: `entry.*`, `day.closed`, `report.export`, `quickswitch.suggest`, `schedule.tick`
- [x] Declarative settings form. *A field list in `addon.toml` (text, number, boolean, select, password) instead of full JSON Schema: ordered, simpler to write, enough for v1.*
- [x] Page: `addons/` (install from file — `.zip` or `addon.toml` while developing — permissions consent dialog, enable/disable, settings, uninstall); addon formats in the Reports export menu; addon options in the quick switch
- [x] Host APIs the calendar addons need: secret store (`keyring`), OAuth 2 PKCE helper with loopback redirect, scheduled sync (`schedule.tick`), addon-owned storage for cached events (KV), and a timeline contribution (events shown in the timesheet day/week view with "track this event"). *Secrets live in the OS keychain under the addon's id (`addon_secret` holds only the names, migration `0007`). OAuth is a manifest section (`[[oauth]]`): the app signs in, keeps the refresh token and hands out access tokens (`host::oauth_token`), so addons never see refresh tokens. Timeline items come from the `timeline.events` hook and show in the calendar view only, not the table view; all-day items are shown in the day header but not trackable. Tested with the `addons/host-test` fixture; a real Microsoft/Google sign-in is still to be tried with registered client ids.*
- [ ] **First addons: calendars** (previously planned as a built-in phase). Microsoft 365 / Outlook (Graph, `delta`), Google Calendar (`syncToken`), ICS URL and CalDAV (read-only); incremental sync every 5 min + on focus; mapping rules → entries (opt-in); absence → OOF event (opt-in)
- [x] First-party example addons: **configurable CSV export format** (`addons/csv-export`, proves `report.export`) and **Recent notes** (`addons/recent-notes`: events, storage, quick switch, notifications). *Both run in the host tests.* GitHub/Jira issue linker still open.
- [x] `docs/addons.md` developer guide. *Its quick-start addon was built from the guide alone and run through the host.*

**Done when:** Outlook events appear in the timesheet through the calendar addon and become entries in one click, and a third party can build an export-format addon from the docs alone (✅).

> 🎯 **Release 1.0 (Free)**: Phases 0–4, plus signed builds and the auto-updater (`tauri-plugin-updater`).

---

## Phase 5 — Server, Accounts, Sync, Licensing (≈ 5–7 weeks)

> Optional early spike (in parallel with Phase 2–3): steps 5a + 5b only, to validate the desktop OIDC login flow early.

**5a. Keycloak setup**
- [x] `deploy/keycloak/realm-zeima.json`: realm `zeima`, clients `zeima-desktop`, `zeima-api`, `zeima-api-admin`, `zeima-swagger`, `zeima-web` (SPEC §7.2), audience mapper, password/MFA/brute-force policy, Organizations on, self-registration, de/en. *Org roles come from the built-in group mapper with full paths (`/orgs/<orgId>/roles/<role>`) instead of a custom `orgs` claim mapper, so no Java extension is needed. Environment-specific values (secrets, URLs, SMTP) are `${VAR:default}` placeholders resolved at import. The names were `time-*` before the rename to Zeima.*
- [x] Development stack `deploy/compose.dev.yml`: Postgres 17 (databases `zeima` and `keycloak`), Keycloak 26.8 with realm import, Mailpit, and the API as an optional profile. *Not yet started here: the realm import, the loopback redirect with any port (`http://127.0.0.1/callback`) and the `${VAR}` placeholders for boolean fields still need a first run.*
- [ ] Google/Microsoft social login (cloud; needs client ids registered with each provider)
- [ ] Event-listener extension → HMAC-signed webhook to the API
- [ ] Login theme (de/en, app branding)
- [ ] `keycloak-config-cli` step in CI/CD to apply realm changes

**5b. Fastify API skeleton**
- [x] `server/` package: Fastify 5, TypeBox type provider, config, helmet/cors/rate-limit/under-pressure, Pino (authorization header redacted), problem+json error handler. *Config is a TypeBox schema checked by `loadConfig()` (every problem listed at once) instead of `@fastify/env`, so `buildApp(config)` takes a plain object in tests. Node 24 runs the TypeScript directly (type stripping): no build step.*
- [x] `@fastify/swagger` + `@fastify/swagger-ui` at `/docs` with the Keycloak OAuth2 security scheme (PKCE, client `zeima-swagger`); `bun run --cwd server openapi:export` writes `packages/api-contract/openapi.json`. *A test fails when the committed file differs, and checks that every route has tags, a summary, a 2xx schema, and 401/403 when protected.*
- [x] Auth plugin: `jose` JWKS verification (`iss`, `aud`, `exp`, `typ: Bearer`, allowed `azp`), `request.user` with org memberships, `X-Org-Id` handling. *A route is protected by declaring `security` in its schema, so docs and enforcement cannot disagree. `KEYCLOAK_INTERNAL_URL` fetches keys from inside Docker while the issuer stays public.*
- [ ] Authz plugin: `can()` driven by `packages/shared-fixtures/permissions.yaml` + a matrix test
- [ ] Drizzle schema + migrations mirroring the client schema + `organization`, `device`, `entitlement`, `kc_user`, `kc_group`, `kc_group_member`, `kc_org_member`, `audit_log`; RLS policies
- [x] `/healthz`, `/readyz` (*Keycloak JWKS; the database check comes with Drizzle*), `/metrics`; Dockerfile (Node 24 slim, non-root). *Image built and run here.*
- [ ] CI: lint, typecheck, Vitest, Testcontainers (Postgres + Keycloak), `oasdiff breaking`, route schema lint. *Done: typecheck, Vitest (incl. contract check and route lint), image build. Open: Testcontainers, `oasdiff` (needs a released contract to compare with), a linter.*

**5c. Shared logic & contracts**
- [ ] `crates/zeima-core-wasm` (wasm-bindgen, `--target nodejs`) → `@zeima/core-wasm`: work-time calc, balances, holidays, country profiles
- [ ] `packages/shared-fixtures/sync/*.json` test vectors; Rust and TS merge implementations both pass them
- [ ] `crates/zeima-api-client` generated with `progenitor`; Vue types via `openapi-typescript` + `openapi-fetch` (fetch = `@tauri-apps/plugin-http`)

**5d. Desktop auth**
- [ ] OIDC Auth Code + PKCE in the system browser, loopback redirect (deep-link fallback); `offline_access` refresh token stored in the keychain
- [ ] `auth_login`, `auth_logout`, `auth_access_token` commands; silent refresh; "Signed in as…" in `NavUser.vue`
- [ ] Server URL setting + `.well-known/zeima-server` discovery (API URL + issuer) + installer preset

**5e. Sync**
- [ ] `/v1/sync/push`, `/v1/sync/pull` (cursor), field-level LWW by HLC, server rule overrides
- [ ] Client sync worker in Rust: push the outbox, pull, apply, conflict cards UI
- [ ] WebSocket "changes available" channel
- [ ] Deterministic multi-device convergence tests

**5f. Licensing, billing, deployment**
- [ ] **Entitlements**: signed token, offline grace, `has_feature()` gating in the client and server
- [ ] Billing (cloud only): Paddle / Lemon Squeezy (or Stripe) checkout + webhooks → entitlements; account page in app
- [ ] `DEPLOYMENT_MODE = cloud | self_hosted`; billing/signup modules disabled in self-hosted mode; offline license file import in the admin UI
- [ ] `deploy/docker-compose.yml` (api + keycloak + postgres + caddy). A CI job boots it and runs login + sync E2E against it, so self-hosting can't silently break.
- [ ] Cloud deployment: EU hosting, managed Postgres for both DBs, Keycloak in HA mode (2+ nodes), backups, monitoring (OpenTelemetry → Grafana or Sentry)
- [ ] API versioning (`/v1`) + client/server compatibility check

> 🎯 **Release 1.1 (Pro)**: multi-device sync for individuals.

---

## Phase 6 — Organization Features (≈ 6–8 weeks)

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

## Phase 7 — Enterprise & Hardening (ongoing)

- SSO per org: Keycloak identity providers (Entra ID, Google Workspace, generic SAML/OIDC) linked to the org's email domain; LDAP/AD user federation for self-hosted; SCIM provisioning via a Keycloak SCIM extension; self-service SSO setup page in the admin UI
- Self-hosted polish: Helm chart, air-gapped install bundle, admin docs, upgrade guide, mountable extra country profiles
- Addon registry with signing; org addon allowlist
- Timeular BLE tracker addon; office Wi-Fi location addon
- Data import (Toggl, Clockify, Timeular CSV)
- Security review / penetration test; GDPR DPA, works-council documentation pack
- Mobile companion (Tauri mobile) exploration

---

## Cross-Cutting Rules (apply in every phase)

- Every write goes through `zeima-core` repositories, which write the `change_log` in the same transaction. No exceptions.
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
| Logic drift between Rust client and TS server | Domain math in one place (`zeima-core` → WASM); shared JSON test vectors for sync merge and permissions |
| Sync bugs corrupt data | Tombstones, change log, server-side history table, convergence tests, local backups |
| Scope creep before revenue | Ship 1.0 Free early; validate demand for Org before Phase 6 |
| Works-council or privacy objections | Privacy-first defaults (SPEC §12); documented visibility model |

## Immediate Next Steps

1. Answer the open questions in SPEC §14 (the missing feature bullet).
2. Manual end-to-end test of Phase 1 on Linux, Windows and macOS (tray behaviour differs per platform).
3. Review the AT/DE/CH profile values with someone who knows each country's law.
4. Phase 5: start `deploy/compose.dev.yml` and check the realm (import, desktop loopback redirect), then 5b's database part (Drizzle schema, RLS) and `can()`, then 5d desktop sign-in.
5. Later: the calendar addons (ICS first: no account needed; then Microsoft 365 and Google).
