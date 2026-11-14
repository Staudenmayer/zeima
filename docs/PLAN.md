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
- [~] **First addons: calendars** (previously planned as a built-in phase). **ICS feed ✅** (`addons/ics-calendar`: any `.ics`/`webcal://` link, time zones incl. Outlook names, all-day events, recurrence rules (daily/weekly/monthly/yearly, BYDAY/BYMONTHDAY, COUNT/UNTIL, EXDATE, changed instances), refresh every 15 min; the user's address is the only host it may reach (`host_settings` in the manifest). Not supported: BYSETPOS, RDATE.) Still open: Microsoft 365 / Outlook (Graph, `delta`), Google Calendar (`syncToken`), CalDAV; sync on focus; mapping rules → entries (opt-in); absence → OOF event (opt-in)
- [x] First-party example addons: **configurable CSV export format** (`addons/csv-export`, proves `report.export`) and **Recent notes** (`addons/recent-notes`: events, storage, quick switch, notifications). *Both run in the host tests.* GitHub/Jira issue linker still open.
- [x] `docs/addons.md` developer guide. *Its quick-start addon was built from the guide alone and run through the host.*

**Done when:** Outlook events appear in the timesheet through the calendar addon and become entries in one click, and a third party can build an export-format addon from the docs alone (✅).

> 🎯 **Release 1.0 (Free)**: Phases 0–4, plus signed builds and the auto-updater (`tauri-plugin-updater`).

---

## Phase 5 — Server, Accounts, Sync, Licensing (≈ 5–7 weeks)

> Optional early spike (in parallel with Phase 2–3): steps 5a + 5b only, to validate the desktop OIDC login flow early.

**5a. Keycloak setup**
- [x] `deploy/keycloak/realm-zeima.json`: realm `zeima`, clients `zeima-desktop`, `zeima-api`, `zeima-api-admin`, `zeima-swagger`, `zeima-web` (SPEC §7.2), audience mapper, password/MFA/brute-force policy, Organizations on, self-registration, de/en. *Org roles come from the built-in group mapper with full paths (`/orgs/<orgId>/roles/<role>`) instead of a custom `orgs` claim mapper, so no Java extension is needed. Environment-specific values (secrets, URLs, SMTP) are `${VAR:default}` placeholders resolved at import. The names were `time-*` before the rename to Zeima.*
- [x] Development stack `deploy/compose.dev.yml`: Postgres 17 (databases `zeima` and `keycloak`), Keycloak 26.8 with realm import, Mailpit, and the API as an optional profile. *First run done 2026-11-13 (Podman, Keycloak 26.8.1): the realm imports, the `${VAR}` placeholders resolve (SMTP host), and the desktop client accepts the loopback redirect on any port (`http://127.0.0.1:54321/callback` reaches the login page). Bind mounts use `:z` so they work with SELinux (Fedora).*
- [ ] Google/Microsoft social login (cloud). *Needs OAuth clients registered with each provider, which only the operator can do; the steps are in `docs/keycloak.md`.*
- [ ] Event-listener extension → HMAC-signed webhook to the API. *Moved to Phase 6: it keeps the organization/group mirror current, which exists only with organizations. Individuals are covered by the token upsert (`identity.ts`).*
- [x] Login theme (`deploy/keycloak/themes/zeima/login`, extends `keycloak.v2`: logo, colors, "Email" label; selected with `loginTheme` in the realm). Checked by screenshot against the self-hosted stack.
- [x] `keycloak-config-cli`: `deploy/keycloak/apply.sh` applies the realm file to a running Keycloak (placeholders converted); tried against the dev Keycloak and run in CI (job `selfhosted`) before the sign-in test. `docs/keycloak.md`.

**5b. Fastify API skeleton**
- [x] `server/` package: Fastify 5, TypeBox type provider, config, helmet/cors/rate-limit/under-pressure, Pino (authorization header redacted), problem+json error handler. *Config is a TypeBox schema checked by `loadConfig()` (every problem listed at once) instead of `@fastify/env`, so `buildApp(config)` takes a plain object in tests. Node 24 runs the TypeScript directly (type stripping): no build step.*
- [x] `@fastify/swagger` + `@fastify/swagger-ui` at `/docs` with the Keycloak OAuth2 security scheme (PKCE, client `zeima-swagger`); `bun run --cwd server openapi:export` writes `packages/api-contract/openapi.json`. *A test fails when the committed file differs, and checks that every route has tags, a summary, a 2xx schema, and 401/403 when protected.*
- [x] Auth plugin: `jose` JWKS verification (`iss`, `aud`, `exp`, `typ: Bearer`, allowed `azp`), `request.user` with org memberships, `X-Org-Id` handling. *A route is protected by declaring `security` in its schema, so docs and enforcement cannot disagree. `KEYCLOAK_INTERNAL_URL` fetches keys from inside Docker while the issuer stays public.*
- [x] Authz: `can()` / `authorize()` (`server/src/plugins/authz.ts`) driven by `packages/shared-fixtures/permissions.yaml` + a matrix test. *Rules map an action and an org role to a scope (`own`, `group`, `org`). Admins read only their own entries by default (privacy-first); this is a choice to confirm with SPEC §5.9. No route uses it yet: that comes with the first org endpoints in Phase 6.*
- [x] Drizzle schema + migrations (`server/src/db/schema.ts`, `server/drizzle/`) with RLS: `organization`, `kc_*`, `device`, `entitlement` (with license token and billing details), `audit_log`, and the typed `workspace`, `project`, `tag`, `time_entry`, `time_entry_tag`, `pause`; `sync_row` holds the merged sync state and is what sync reads. Requests run as `zeima_app` with the caller's identity (`withUser`), migrations and the Keycloak mirror as the owner; the API migrates on start. Tested against Postgres (`server/test/db.test.ts`). *Decision: the typed mirror of the remaining client tables (work time, cost centers, activities) is built from `sync_row` when server-side reports need it (Phase 6); sync does not read them.*
- [x] `/healthz`, `/readyz` (*Keycloak JWKS and the database*), `/metrics`; Dockerfile (Node 24 slim, non-root). *Image built and run here.*
- [x] CI (`.github/workflows/ci.yml`): Biome lint, typecheck, Vitest (contract check, route lint, Postgres service for the row-level-security and sync tests, `@zeima/core-wasm`), generated TS types drift check, `oasdiff breaking` on pull requests, image build, and a `selfhosted` job that boots the compose stack and runs the sign-in + sync E2E. *Testcontainers were replaced by the compose-based E2E, which tests the real Keycloak with the real realm.*

**5c. Shared logic & contracts**
- [x] `crates/zeima-core-wasm` → `@zeima/core-wasm` (`packages/core-wasm`, `bun run --cwd packages/core-wasm build`): country profiles, holidays, break rules, `computeDays` and balance as JSON-in/JSON-out functions; tested from the server (`server/test/core-wasm.test.ts`) and built in CI. *Used by the server for reports from Phase 6.*
- [x] `packages/shared-fixtures/sync/merge.json` test vectors (22 cases + order-independence checks); the Rust (`zeima-core/src/sync.rs`) and TS (`server/src/sync/merge.ts`) merges both pass them. *State-based: per-field value + HLC and the latest delete HLC, so merging is commutative and idempotent; a row is live unless deleted and not written after. Rules: only managers change locked time entries or the `locked` flag. A change carries only the changed fields; the client's `change_log` stores whole rows today, so the sync worker (5e) must diff against the last pushed state.*
- [~] Typed API clients. *TypeScript: `packages/api-contract/schema.d.ts` is generated from `openapi.json` (`bun run --cwd packages/api-contract generate`, drift-checked in CI); `openapi-fetch` is added when the Vue app first calls the API (today only Rust does). Rust: `progenitor` reads OpenAPI 3.0 only and the contract is 3.1, so the desktop's small client (`src-tauri/src/sync.rs`, `billing.rs`) stays hand-written; both sides are tested against the same wire format.*

**5d. Desktop auth**
- [x] OIDC Auth Code + PKCE in the system browser, loopback redirect; `offline_access` refresh token in the keychain (`src-tauri/src/account.rs`, on top of the addon OAuth flow in `oauth.rs`). *The realm's desktop client also allows `http://127.0.0.1` without a path, which that flow sends. The deep-link fallback (`zeima://auth/callback`) is registered but not implemented.*
- [x] Commands `account_status`, `account_set_server`, `account_sign_in`, `account_cancel`, `account_sign_out`, `account_access_token` (refreshes silently); Settings → Account card and a "Signed in as…" line in the sidebar footer. *`NavUser.vue` (the unused shadcn template) was not used. Unit-tested (address checks, provider, token claims); a real browser sign-in against the dev Keycloak is still to be tried by hand.*
- [x] Server URL setting + `GET /.well-known/zeima-server` discovery (API URL, issuer, client id, API versions). Open: installer preset.

**5e. Sync**
- [x] Server: `POST /v1/workspaces` (personal workspace, idempotent), `POST /v1/sync/push`, `GET /v1/sync/pull` (cursor, paging) with field-level LWW by HLC and the lock rules (`server/src/modules/sync/routes.ts`, merged state in `sync_row`, token upsert keeps `kc_user` current). Tested over HTTP against Postgres (`server/test/sync.test.ts`). *Open: the typed tables (`project`, `time_entry`, …) are not yet filled from `sync_row` (needed for server-side reports); org workspaces come with Phase 6; per-user write checks inside org workspaces (`can()` on `user_id`) are not wired; no HLC future-skew limit yet.*
- [x] Client sync worker in Rust (`Core::sync_outbox`/`sync_apply`, `src-tauri/src/sync.rs`). *Every minute, on "Sync now" and when the server says changes are available: the outbox goes to the server as field-level changes (against `sync_state`), then the server's changes are merged and written to the typed tables without touching the change log (a newer unpushed local edit wins per field, FK checks are off while applying, the HLC clock moves past what the server sent). `workspaceId` and `userId` are not synced; later devices find the personal workspace with `GET /v1/workspaces`. **Synced tables (14):** `project`, `tag`, `time_entry` (with tag sets), `pause`, `activity`, `cost_center`, `cost_center_assignment`, and the work-time data: `work_settings`, `jurisdiction_selection`, `work_schedule`, `absence_type`, `absence`, `custom_holiday`, `balance_adjustment`. Rows that every device creates for itself get *canonical ids* derived from what identifies them (`repo/sync_ids.rs`: work settings and the country selection once per user, built-in absence types by key, a schedule by its start date); local ids stay, `sync_alias` maps them, and an absence's type reference is translated both ways. Fields that must stay consistent travel as one *group field* (`repo/sync_groups.rs`: profile + region, start + end + zone of an entry or pause, an absence's dates + half-day). A device that already has data and finds an account with data asks first (`sync_join`: keep both, or use the account's data via `Core::sync_wipe_local`). Rows that cannot be written are recorded in `sync_conflict`, listed in Settings → Account and can be retried or dismissed. Tested with two devices and a stand-in server (`tests/sync_apply.rs`, `tests/sync_worktime.rs`) and the randomized convergence tests. Open: side-by-side conflict cards, changing the server, org workspaces.*
- [x] WebSocket "changes available" channel: `GET /v1/sync/events?deviceId=…` (`server/src/plugins/realtime.ts`, `@fastify/websocket`, same bearer token; closes with code 4401 when the token expires) tells a user's other devices to pull after a push that changed something; the desktop app keeps the connection open while signed in (`sync::spawn_listener`, reconnects with a fresh token and growing pauses) and syncs on each message. The one-minute timer remains as the fallback. *Presence for Phase 6 will use the same hub. Not in the OpenAPI document (a WebSocket is not an HTTP operation).*
- [x] Deterministic multi-device convergence tests (`crates/zeima-core/tests/sync_converge.rs`): three devices with skewed clocks make random edits to the same projects and fields, delete, and sync at random moments; all must end identical (40 seeds by default, `CONVERGE_SEEDS=600` for a long sweep, `CONVERGE_SEED=n` to replay one). *They found two real bugs now fixed: a stale field overwriting another device's edit (protection was per row, not per field) and a row deleted on one device not receiving field updates.*

**5f. Licensing, billing, deployment**
- [x] **Entitlements**: Ed25519-signed tokens (`zeima-core/src/entitlement.rs`, `server/src/licensing/entitlement.ts`, 12 shared vectors in `packages/shared-fixtures/entitlement/`), `GET /v1/entitlement` (best of the user's and their orgs' rows; a vendor-signed license token is passed through), 14-day offline grace, free never locks data. The desktop app fetches and verifies the token on every sync, gates syncing on `has_feature("sync")` and shows the plan in Settings → Account; the cloud server enforces `sync`. *Dev: debug builds trust the built-in dev key; start the API with `DEFAULT_PLAN=pro`. Production: `ENTITLEMENT_SIGNING_KEY`, `ZEIMA_ENTITLEMENT_KEYS` (`docs/releasing.md`). Seat counting comes with organizations (Phase 6).*
- [x] Billing (cloud only): Lemon Squeezy (a merchant of record, so VAT is theirs). `POST /v1/billing/webhook` (HMAC-signed, stale-event safe) turns subscriptions into entitlements; `GET /v1/billing/plans`, `POST /v1/billing/checkout` (link carries the user, org purchases need the owner), `GET /v1/billing`; Settings → Account shows upgrade buttons on the Free plan and "Manage subscription". Tested with signed events and the real database (`server/test/billing.test.ts`). *Needs a Lemon Squeezy store and `BILLING_*` settings to go live.*
- [x] `DEPLOYMENT_MODE = cloud | self_hosted`: billing routes exist only in the cloud, license routes only self-hosted. `PUT /v1/license` (role `platform-admin`) installs a vendor-signed license, `GET /v1/license` lists them (`server/test/licensing.test.ts`). *The admin UI for it comes with the Phase 6 admin pages.*
- [x] `deploy/docker-compose.yml` (api + keycloak + postgres + caddy, `deploy/.env.example`, `Caddyfile`, `postgres-init.sh`). `deploy/e2e/smoke.mjs` creates a user, signs in with Authorization Code + PKCE and `offline_access` like the app, then calls `/v1/me`, `/v1/entitlement`, creates a workspace, pushes and pulls (14 checks); a CI job (`selfhosted`) boots the stack and runs it. *Run locally with Podman 2026-11-13. The E2E found that the realm's public clients had no default scopes: tokens lacked `email`, `name` and `realm_access`, and `profile`/`email` requests were refused with `invalid_scope`. Fixed in `realm-zeima.json` (`defaultClientScopes`); an existing Keycloak keeps its old clients until its volume is recreated (`down -v`) or the client scopes are changed in the admin console. Open: Keycloak HA and upgrade notes, backups, the license-upload admin UI.*
- [~] Cloud deployment: `docs/deployment-cloud.md` (architecture, Keycloak HA settings, backups, monitoring, secrets, pre-launch checks). *Provisioning the EU hosting, managed Postgres, Keycloak cluster and monitoring needs accounts that are not in the repository.*
- [x] API versioning (`/v1`) + compatibility check: discovery lists `apiVersions` and `minClientVersion` (`MIN_CLIENT_VERSION`); the app refuses a server it is too old for and says so; CI fails pull requests that break the contract (`oasdiff`).

> 🎯 **Release 1.1 (Pro)**: multi-device sync for individuals. *Code complete for 1.1, including sync of the work-time settings, except what needs outside accounts: social login (provider clients), the Lemon Squeezy store, signing certificates, the EU cloud environment, and the first calendar addons (Phase 4).*

---

## Phase 6 — Organization Features (≈ 6–8 weeks)

Order is by dependency:

1. **User management** *(started: API ✅ — org creation, members, invitations with roles and seats, role changes, deactivate/remove, audit log (`server/src/modules/orgs`), checked against Keycloak 26.8; departments API ✅; admin page in the app ✅ (`/organization`: members, invitations, departments, activity; requests go through the `org_request` command, which only allows `/v1/orgs…` and `/v1/me`); not yet tried against a running app, and the Keycloak event webhook and org policies are still open)*: org creation (Keycloak Organization + role groups), invitations, role assignment, hierarchical department groups. All of it goes through API admin endpoints that call the Keycloak Admin API, then the mirror tables. Admin pages (`admin/users`, `admin/groups`, `admin/policies`), seat counting against the entitlement, audit log.
2. **Shared data** *(decided: a full second workspace in the app, next to the personal one. Server half ✅: one shared workspace per organization (created with it, or on first `GET /v1/workspaces` for older ones), writes checked per table (`server/src/modules/sync/orgrules.ts`: projects, tags, cost centers and policies need the matching `*.write` right; time entries, pauses, absences and a person's own schedule are written by their owner, time entries also by whoever has `entry.write_locked`; display settings never), pulls filtered by `entry.read`/`absence.read`/`member.read`. App, stage 1 ✅: `Core::scoped(workspace, user)` gives a handle on the same database that reads and writes only that workspace; `adopt_org_workspace` records a server workspace and the user's Keycloak account in it; `workspaces()` lists them (migration 0012). Stage 2 ✅: sync of the second workspace — changes are logged and pushed per workspace (`change_log.workspace_id`, migration 0013), shared rows carry the Keycloak account id as `userId` and arrive with their owner (other people's accounts get a placeholder `app_user`), `sync_once` adopts the server's org workspaces and pushes/pulls each through its scoped handle (tested between two simulated devices; not yet against a running server). Stage 3 ✅: the app has an active workspace (`AppState::data()`; data commands follow it, while settings, backups, sync and addons stay personal), `workspace_list`/`workspace_switch` (remembered across restarts), one timer at a time across workspaces, a switcher in the sidebar when there is more than one workspace, and add/edit/delete of projects, tags, cost centers and activities hidden for people without the right (the server enforces it anyway). Known limits: the tray, auto-pause and the morning prompt watch the personal workspace's timer only; other people's entries are not shown yet (the manager/team views come with the manager dashboards, item 7). Project access ✅: projects have a visibility (everyone, or members only), a manager and a member list (migration 0014; synced like other fields, edited in the project dialog in an organization's workspace); the server hides members-only projects from everyone but owners/admins, the manager and the members, and lets a project's manager edit the project but not hand it over. Not yet hidden: tags and entries that point at a hidden project; people with the `member.read` right see only a part of the member list, so a manager who is not an admin can only pick from the people they can see. Open: org work schedules and auto-pause policies per group.)*: org projects (visibility, members, project manager role), org cost centers/tags, org work schedules and auto-pause policies per group
3. **Approvals** *(timesheets ✅: a person submits a period (`timesheet_submission`, migration 0015; Approvals page in an organization's workspace), someone with `entry.approve` over them approves or rejects it, the owner can take back a waiting one or submit a rejected one again; the server checks who may do what and locks the person's entries of an approved period (`server/src/modules/sync/approvals.ts`). Absence requests ✅: in an organization's workspace every new or changed absence is a request (`absence.status`, decided_by/decision_note, migration 0016); someone with `absence.approve` over the person approves or rejects it on the Approvals page, a rejected one takes no time off, changing a decided one asks again. Notifications ✅ (a desktop notification when a sync brings something that waits for the person's decision, or a decision on their own timesheet or absence; not for the first load of a new device) and reopening ✅ (an approver sends an approved timesheet back as rejected, the server unlocks the period's entries). Item 3 is done except for in-app notification lists)*: timesheet submission/approval → `locked` entries; absence requests/approval
4. **Online users** *(first version ✅: devices report online/tracking/away to every organization they belong to (`src-tauri/src/presence.rs`, `PUT /v1/orgs/:id/presence`), the server keeps it in memory, drops people whose last device disconnects or whose report is older than 2.5 minutes, and tells the members over the existing WebSocket; an Online tab on the Organization page lists them. Privacy: an organization hears "tracking" only for a timer in its own workspace (a personal timer shows as plain online), and the project only if the person switched on "Show my project" in Settings. Open: `in_meeting` is never reported yet (needs the calendar), idle/lock detection for `away`, the per-organization presence visibility policy, the dashboard widget and filter by group, and presence across several API instances)*: server WebSocket presence hub, client presence reporter (based on timer state and the idle/lock watchers), privacy opt-in for project sharing, `team/online` panel + dashboard widget
5. **Work location** *(first version ✅: `work_location` rows, one per person and day (migration 0017, id derived from person and day so devices meet in one row), synced to everyone in the organization; a Locations page shows a week grid of people by day with the headcount in the office, and each person sets their own days. `GET /v1/orgs/:id/people` gives every member the names of the others. Open: offices with capacity, a default weekly pattern, the morning prompt, month view)*: `office`, `work_location` tables; weekly planner; morning tray prompt; `team/locations` grid (users × days) with office capacity
6. **Meetings**: scheduling UI with free/busy grid, write-through to the organizer's calendar (Graph/Google), RSVP sync, "track this meeting" prompt per attendee, RRULE recurrence
7. Manager dashboards *(first part ✅: a Team page for owners and managers shows tracked and billable time per person and the project breakdown for this/last week or month, plus the number of timesheets waiting (`Core::team_summary`); it shows what the server sends the role. Open: tags, cost centers and single entries per member, group totals, overtime outliers)*: group totals, **project-level breakdown per member** (projects, tags, cost centers, entries), overtime outliers, pending approvals

> 🎯 **Release 2.0 (Team/Org)**: launches in the cloud first. The self-hosted Docker release follows once it has run on the cloud for one stable cycle (same version, same binary).

**State for 2.0 (code complete in the repository).** All seven items above have an implementation with tests: user management (API, admin page, policies, nightly/on-demand/webhook reconcile with Keycloak), the shared workspace and its rules, timesheet and absence approvals with locking and notifications, presence (online, tracking, in a meeting, away; department filter; dashboard widget), work locations (week/month, offices with capacity, usual week), meetings (organize, answer, repeat, free/busy, `.ics` export, timeline, "track it?" prompt after a meeting) and the manager views (people, projects, tags, cost centers, entries, departments, outliers against the weekly target). `deploy/e2e/org-smoke.mjs` runs the organization flow against a real Keycloak and the API (24 steps, passes).

What 2.0 deliberately does not include, and why:
- **Calendar write-through of meetings** (Graph/Google, RSVP sync back). It needs OAuth clients registered by the operator and the Microsoft 365 / Google calendar addons (Phase 4, open). Meetings export as `.ics` for any calendar program.
- **Keycloak event listener.** The API accepts signed events (`POST /v1/internal/keycloak-events`) but the listener itself (a Keycloak provider) is not shipped; the nightly check and `POST /v1/orgs/:id/reconcile` keep the copy right meanwhile.
- **Several API instances.** Presence is held in one process; a second instance needs a shared channel (Postgres `LISTEN/NOTIFY` or Redis).
- **Auto-pause per department** is a suggested idle time (the shortest wins when shorter than the person's own), not a forced setting.
- **Windows and macOS** builds of all this are untested here (only Linux); signing, billing, social login and cloud hosting need the operator's accounts (see `docs/setup-server.md`).

**Before tagging 2.0.0:** run `deploy/e2e/org-smoke.mjs` and `deploy/e2e/smoke.mjs` against the real stack; try two apps (Linux plus Windows or macOS) on one organization by hand: invite, switch workspace, track, submit and approve a timesheet, create a meeting; fix the updater key password secret; then `node scripts/version.mjs set 2.0.0` (already applied in the repository), commit and tag.

---

## Phase 7 — Enterprise & Hardening (ongoing)

- SSO per org: Keycloak identity providers (Entra ID, Google Workspace, generic SAML/OIDC) linked to the org's email domain; LDAP/AD user federation for self-hosted; SCIM provisioning via a Keycloak SCIM extension; self-service SSO setup page in the admin UI
- Self-hosted polish: Helm chart, air-gapped install bundle, admin docs, upgrade guide, mountable extra country profiles
- Addon registry with signing; org addon allowlist
- Timeular BLE tracker addon; office Wi-Fi location addon
- Data import: **Toggl and Clockify ✅** (Settings → Data → Import: a detailed CSV export with the columns Project, Client, Description, Billable, Start date/time, End date/time, Tags, English or German headers; date order chosen by the person with a guess from the file; projects and tags matched by name and created; re-importing adds nothing; `zeima_core::import`). Timeular CSV still open (its export has a different layout)
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
5. Later: the calendar addons (ICS is done; next Microsoft 365 and Google, which need registered OAuth clients).
