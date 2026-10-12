# Time — Product & Technical Specification

> Status: Draft v0.1 · 2026-10-04
> Working name: **Time** (current package name `tray-app`, to be renamed)

A local-first desktop time tracking and work-time management app that lives in the system tray.
It combines **Timeular**-style fast, low-friction activity tracking with **BMD**-style
attendance tracking: work hours, flexitime balance, cost centers and exports for accounting.
It runs fully offline for one person. A paid tier adds sync and organization features.

---

## 1. Goals & Non-Goals

### Goals
1. **Zero-friction tracking.** Start or switch an activity in under 2 seconds from the tray, a global shortcut or the quick-switch palette.
2. **Local first.** All data lives in a local SQLite database. The app is fully usable without an account or network. Sync is an add-on, never a requirement.
3. **Correct work time.** Work schedules, automatic breaks, flexitime/overtime balance, absences and public holidays are calculated correctly and can be audited.
4. **Ready for organizations.** The data model, IDs and permission checks are designed from day one so a later sync server can add multi-user organizations without a rewrite.
5. **Extensible.** An addon system lets first-party and third-party integrations hook into tracking, reports and exports.

### Non-Goals (v1)
- Mobile apps (the architecture should not rule them out; Tauri 2 supports mobile).
- Payroll calculation. We export data for payroll and accounting systems but do not compute wages. System-specific exports such as BMD are separate addons and are out of scope for this spec.
- Invoicing. We provide billable-hours reports and exports only.
- Screenshot or keystroke surveillance. We will never build these; see §12 Privacy.

---

## 2. Editions & Monetization

| Capability | **Free** (Local) | **Pro** (Personal Sync) | **Team / Org** |
|---|---|---|---|
| Timer, projects, tags, manual entries | ✅ | ✅ | ✅ |
| Work hours, auto-pause, absences, holidays | ✅ | ✅ | ✅ |
| Dashboard & reports, CSV/XLSX/PDF export | ✅ | ✅ | ✅ |
| Cost centers | ✅ | ✅ | ✅ (org-managed) |
| Calendar integration (personal, read) | ✅ | ✅ | ✅ |
| Addons (local) | ✅ | ✅ | ✅ (admin-allowlisted) |
| Encrypted backup / multi-device sync | — | ✅ | ✅ |
| Organization, users, groups, roles | — | — | ✅ |
| Shared projects / cost centers / tags | — | — | ✅ |
| Online users (presence) | — | — | ✅ |
| Work-location view (office / home office) | — | — | ✅ |
| Meetings: schedule & sync with attendees | — | — | ✅ |
| Approvals (timesheets, absences) | — | — | ✅ |
| SSO (Entra ID / Google Workspace / SAML / LDAP via Keycloak) | — | — | ✅ (Enterprise add-on) |
| Hosting | local only | cloud | **cloud (default)** or **self-hosted** (offline license file) |

**Licensing model**
- The server issues a signed **entitlement token** (Ed25519-signed JWT-like blob). It contains `plan`, `seats`, `features[]`, `org_id` and `expires_at`, plus an offline grace period (e.g. 14 days).
- The client checks the signature with an embedded public key. Features are gated by `has_feature("presence")`, not by plan name, so plans can be repackaged later.
- **Free never expires and never locks data.** A lapsed paid plan drops back to Free: local data stays readable and editable, and only sync and org features stop.
- Payments are handled by a Merchant of Record (Paddle or Lemon Squeezy recommended because they handle EU VAT) or Stripe Billing. This is a server concern only.

---

## 3. Architecture

```
┌──────────────────────────── Desktop App (Tauri 2) ────────────────────────────┐
│  Vue 3 UI (webview)                                                            │
│    pages/ · components/ · stores (Pinia) · i18n (de, en)                       │
│        │  typed IPC (tauri-specta generated bindings)                          │
│  ──────┼────────────────────────────────────────────────────────────────────── │
│  Rust app shell (src-tauri)                                                    │
│    tray · global shortcuts · windows · OS idle/lock/suspend watchers           │
│    command handlers ──► time-core (domain crate)                               │
│                            ├─ repositories (SQLite via sqlx)                   │
│                            ├─ timer engine & auto-pause rules                  │
│                            ├─ work-time calculator (schedules, balances)       │
│                            ├─ event bus ──► addon host (WASM, Extism)          │
│                            └─ change log (outbox) ──► sync client (paid)       │
│    integrations: calendar (MS Graph / Google / ICS), keyring for secrets       │
└────────────────────────────────────────┬───────────────────────────────────────┘
             OIDC login (system browser,     │ HTTPS REST + WebSocket
             Auth Code + PKCE)               │ Bearer access token (paid tiers only)
                    │                        │
┌───────────────────▼──────────┐   ┌─────────▼──────────────────────────────────────┐
│  Keycloak                    │   │  Time API (Node 24 · Fastify · TypeScript)     │
│   realm "time"               │   │   OpenAPI 3.1 via @fastify/swagger → /docs     │
│   users · credentials · MFA  │◄──┤   JWT verify (JWKS) · authz policy layer       │
│   organizations · groups     │   │   sync · org data · presence (WS) · meetings   │
│   roles (realm + org groups) │──►│   calendar sync · licensing · billing webhooks │
│   SSO brokering (Entra,      │   │   Keycloak Admin API client (service account)  │
│   Google, SAML, LDAP)        │   │   @time/core-wasm (time-core compiled to WASM) │
│   events ──► webhook ────────┼──►│                                                │
└──────────────┬───────────────┘   └──────────────┬─────────────────────────────────┘
               │ own DB                           │ Drizzle ORM
        ┌──────▼──────┐                    ┌──────▼──────┐
        │ Postgres    │                    │ Postgres    │   (same server, separate
        │ "keycloak"  │                    │ "time"      │    databases when self-hosted)
        └─────────────┘                    └─────────────┘
```

### 3.1 Key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Local store | **SQLite** (WAL mode) in the app data dir | Embedded, robust, single file and easy backup. The current `tauri-plugin-sql` `postgres` feature is not suitable for local-first use. |
| Data access | **Rust owns the DB** (`sqlx` with the sqlite feature). The frontend calls typed commands and never sends raw SQL. | The timer, auto-pause and sync all run in Rust even when the window is hidden. One write path means one place for validation, change logging and permissions. `tauri-plugin-sql` will be removed. |
| IPC typing | **tauri-specta** generates `src/bindings.ts` | End-to-end types with no hand-written `invoke` strings. |
| Domain code | Separate crate **`crates/time-core`** with no Tauri dependency | Unit-testable. It is also compiled to **WASM** (`@time/core-wasm`) so the Node server uses exactly the same work-time math, country profiles and holiday rules as the desktop. That requires pure logic to stay I/O-free, with SQLite repositories behind a `db` cargo feature. |
| Server | **Fastify** (TypeScript, Node 24 LTS) + TypeBox schemas | Fast, schema-first. Every route schema produces the OpenAPI document, request validation and response serialization from a single definition. |
| API docs / contract | **@fastify/swagger** (OpenAPI 3.1) + **@fastify/swagger-ui** at `/docs` | The generated `openapi.json` is the contract. The desktop Rust sync client and the Vue API types are generated from it. |
| Identity & access | **Keycloak** (OIDC) | Handles login, MFA, password reset, social login, enterprise SSO (Entra ID, Google, SAML, LDAP/AD) and org/role assignment without us building it. It is self-hostable, which matches §7.1. |
| Server DB access | **Drizzle ORM** + drizzle-kit migrations (Postgres) | Typed SQL, plain SQL migration files, works with RLS. |
| IDs | **UUIDv7** (time-sortable), generated client-side | Records can be created offline on any device without collisions. |
| Time storage | UTC `INTEGER` unix-millis + IANA `tz` column on entries | Correct handling of DST, travel and cross-midnight entries. |
| Sync model | Per-row change log, **HLC** timestamps, field-level last-writer-wins plus server-side rules | Simple and predictable for this data shape. CRDTs are overkill because entries rarely see concurrent edits. |
| Secrets | OS keychain via `keyring` crate | OAuth refresh tokens and the sync session never go into SQLite. |
| Addons | **WASM via Extism**, with a permission-declared manifest | Sandboxed, language-agnostic, and does not require trusting arbitrary native code. |
| Charts | `@unovis/vue` (already installed) | Already in the project. |
| UI kit | shadcn-vue (reka-ui) + Tailwind 4 (already installed) | Already in the project. |

### 3.2 Repository layout (target)

```
time/
├─ docs/                     SPEC.md, PLAN.md, adr/ (architecture decision records)
├─ src/                      Vue frontend
│  ├─ bindings.ts            generated by tauri-specta (do not edit)
│  ├─ pages/                 file-based routes (vue-router auto-routes)
│  ├─ components/            app components + ui/ (shadcn)
│  ├─ stores/                Pinia stores (timer, projects, settings, session…)
│  ├─ composables/
│  └─ i18n/                  de.ts, en.ts
├─ src-tauri/                Tauri app shell (tray, windows, commands, OS hooks)
├─ crates/
│  ├─ time-core/             domain models, repos, timer, work-time calc, rules
│  │  └─ jurisdictions/      country profiles: AT.toml, DE.toml, CH.toml + fixtures
│  ├─ time-core-wasm/        wasm-bindgen wrapper → npm package @time/core-wasm
│  ├─ time-api-client/       Rust client generated from openapi.json (progenitor)
│  └─ time-addon-sdk/        Rust SDK for WASM addons
├─ server/                   (Phase 5) Fastify API (cloud + self-hosted, one image)
├─ packages/
│  ├─ api-contract/          openapi.json (generated, committed) + openapi-typescript types
│  └─ shared-fixtures/       JSON test vectors shared by Rust and TS (sync merge, permissions)
├─ deploy/
│  ├─ docker-compose.yml     api + keycloak + postgres + caddy
│  ├─ keycloak/              realm-time.json (realm import), login theme
│  └─ helm/                  (later)
├─ Cargo.toml                Rust workspace
└─ package.json              bun workspaces: root app, server, packages/*
```

---

## 4. Domain Model

All synced tables share these **common columns**:

```
id            TEXT PRIMARY KEY      -- UUIDv7
workspace_id  TEXT NOT NULL         -- personal workspace or org id
created_at    INTEGER NOT NULL      -- unix ms UTC
updated_at    INTEGER NOT NULL
deleted_at    INTEGER NULL          -- tombstone (soft delete; required for sync)
hlc           TEXT NOT NULL         -- hybrid logical clock of last change
origin_device TEXT NOT NULL
```

A **workspace** is the ownership boundary. Free users have one local personal workspace. Joining an org adds an org workspace. Data never mixes across workspaces.

### 4.1 Entities

**Workspace** — `id, kind (personal|org), name, settings_json`

**User** — `id, display_name, email, avatar_url, locale, timezone`
**Membership** — `user_id, workspace_id, role (owner|admin|manager|member), employee_no, active_from, active_to`
**Group** — `id, name, parent_group_id, manager_user_id` (departments and teams; hierarchical)
**GroupMember** — `group_id, user_id`

**Project**
| Field | Notes |
|---|---|
| `name` | |
| `alias` | Short ID / project number, e.g. `P-2024-017` or `INT`. Unique per workspace and usable in the quick switch (`@INT`). |
| `color`, `icon` | |
| `parent_id` | Optional sub-projects / phases |
| `client` | Optional free text (v1) |
| `cost_center_id` | Default cost center for entries on this project |
| `budget_minutes` | Allotted time ("allotted time"). Can be null. |
| `budget_period` | `total` \| `monthly` \| `weekly` |
| `budget_warn_pct` | Warning threshold, default 80 % |
| `billable_default` | bool |
| `hourly_rate_cents` | Optional, for reports |
| `status` | `active` \| `paused` \| `archived` |
| `visibility` | `private` \| `workspace` \| `groups` (org) |

**Activity** — `name, color, archived`
- What kind of work an entry is (e.g. *Meeting*, *Development*, *Review*). Created by the user beforehand (or inline from the quick switch).
- **Required** when starting the timer, creating an entry by hand or editing one. Project and tags stay optional.
- Archived activities are hidden from pickers; activities with entries cannot be deleted, only archived.

**Tag** — `name, color, project_id NULL, group_name NULL`
- `project_id = NULL` → global tag. A non-null `project_id` makes it a **project-scoped tag** (the project's "own tag system", e.g. *Design / Backend / QA* only within project X).
- `group_name` allows mutually exclusive tag groups (e.g. activity type: *Development* vs *Meeting*).

**CostCenter** (Kostenstelle) — `code, name, parent_id, active, valid_from, valid_to`
**CostCenterAssignment** — default cost center per `user` or `group` with a validity range. An entry gets its cost center from: explicit value → project → user/group default.

**TimeEntry**
| Field | Notes |
|---|---|
| `user_id` | |
| `activity_id` | required for new entries; `NULL` only on entries recorded before activities existed or created by addons |
| `project_id` | nullable (untracked work time is still work time) |
| `cost_center_id` | resolved at write time, overridable |
| `start_at`, `end_at` | UTC ms; `end_at NULL` = running |
| `tz` | IANA zone at start |
| `note` | Markdown-free plain text |
| `billable` | |
| `source` | `timer` \| `manual` \| `calendar` \| `meeting` \| `addon:<id>` \| `import` |
| `source_ref` | e.g. calendar event id |
| `locked` | true after timesheet approval or period close |
| **TimeEntryTag** | `entry_id, tag_id` |

Invariant: at most **one running entry per user**. Overlapping entries are allowed but flagged in the UI and in reports.

**Pause** — `user_id, start_at, end_at, kind (manual|auto_lunch|auto_idle|auto_night|auto_lock), rule_id, accepted (bool|null)`
Pauses are first-class records, not holes in entries, so auto-pause decisions can be audited and undone.

**WorkSchedule** — `user_id | group_id, valid_from, valid_to, model (fixed|flextime|trust), days[7]: { target_minutes, core_start, core_end, earliest_start, latest_end }`
**AutoPauseRule** — see §5.3
**Absence** — `user_id, type_id, start_date, end_date, half_day (am|pm|null), status (requested|approved|rejected), note`
**AbsenceType** — `name, counts_as_work (bool), affects_vacation (bool), color` (vacation, sick, comp-time/Zeitausgleich, training, business trip)
**Holiday** — `date, name, region, source (profile|custom)`. Generated from the active country profile's holiday rules (§5.13), plus user/org custom entries.
**JurisdictionSelection** — `workspace_id | group_id | user_id, profile_id (e.g. "AT"), region (e.g. "AT-9"), overrides_json, valid_from`. The most specific selection wins (user → group → workspace).
**WorkLocation** — `user_id, date, location (office|home|remote|travel|off), office_id NULL, note` (planned and actual)
**Office** — `name, address, tz`

**Meeting** — `title, organizer_id, start_at, end_at, tz, location / online_url, project_id, cost_center_id, recurrence (RRULE), calendar_ref`
**MeetingAttendee** — `meeting_id, user_id | external_email, response (none|accepted|tentative|declined)`

**CalendarAccount** — `provider (msgraph|google|caldav|ics), account_label, sync_cursor, enabled` (tokens live in the keychain)
**CalendarEvent** (cache) — `account_id, external_id, title, start_at, end_at, is_all_day, show_as (busy|free|oof), location, raw_json`
**CalendarMapping** — rules that map events to projects/tags (e.g. title contains "Standup" → project INT, tag Meeting)

**Addon** — `addon_id, version, enabled, granted_permissions[], settings_json`
**ChangeLog** (local outbox, not synced itself) — `seq, table, row_id, op (upsert|delete), hlc, payload_json, pushed_at NULL`
**Setting** — key/value per user and per device

### 4.2 Derived values (never stored as truth)
- **Day work time** = Σ entries (clipped to day, overlaps merged) − pauses
- **Target time** = schedule for that weekday − holidays − absences that count as work
- **Balance** (Gleitzeitsaldo) = Σ (worked − target), plus manual corrections (a `BalanceAdjustment` entity with reason and author)
- **Project budget used** = Σ entry duration in the budget period

Cache these in a `daily_summary` table that is recomputed on change, for fast dashboards. The cache is rebuildable at any time.

---

## 5. Features

### 5.1 Tracking (Timeular-style)
- **Tray**
  - The icon shows state (idle / tracking / paused) and the elapsed time as the tooltip (Linux: menu title).
  - Menu: current activity + stop, pause/resume, **5 most recent activity/project/tag combinations** for one-click switching, "Start tracking…" (opens the quick switch), "Open dashboard", Quit.
- **Global shortcuts** (configurable)
  - `Ctrl+Shift+T`: toggle the main window
  - `Ctrl+Shift+Space`: open the **quick-switch palette**, a small always-on-top window
  - `Ctrl+Shift+P`: pause/resume
- **Quick-switch palette**
  - Plain text searches the **activities**; with an empty input the most recent activity/project/tag combinations are offered for one-step restarts, followed by all activities (recently used first).
  - `@` switches the list to **projects** (alias and name, recently used first), `#` to **tags** (global and the chosen project's). Picking one (Enter/Tab, or finishing the token with a space) turns it into a chip; Backspace on an empty input removes the last chip.
  - When nothing matches, the list offers to create the activity (and start it), project or tag right there.
  - For example, `dev @INT #review` + Enter starts *Development* on project INT with tag *review*.
- **Switching** stops the current entry and starts a new one with the same timestamp, so there are no gaps.
- **Manual entries**: day timeline editor (drag to create or resize), list view, duplicate, split, merge.
- **Forgotten-timer protection**: the night auto-pause (§5.3) plus a prompt on next start: "Timer ran overnight. Keep until 18:30 / custom / keep all."
- **Optional (addon, later)**: Timeular physical tracker over Bluetooth LE. Each die face maps to a project.

### 5.2 Projects & Tags
- CRUD, archive, color, alias, hierarchy, default billable flag and cost center.
- **Allotted time**: budget per total/month/week. Progress bar on the project card and dashboard. Notifications at the warn threshold and at 100 %.
- Project-scoped tags and global tags. A tag group can be marked "required" for a project, e.g. every entry on project X needs an activity tag.
- Org edition: project visibility, project members, project manager role (sees all entries on the project).

### 5.3 Auto-Pause
Rules are evaluated by the Rust timer engine, which runs whether or not the window is visible. Every auto-pause creates a `Pause` record and a notification with **Undo**.

| Rule | Trigger | Default |
|---|---|---|
| **Lunch (fixed)** | Within a window (e.g. 11:30–13:30), deduct X min if worked > Y h that day | From the country profile's `breaks` rules (AT: 30 min if > 6 h) |
| **Lunch (detected)** | Idle ≥ N min inside the lunch window → record a pause for the idle span | N = 15 |
| **Idle** | No input for N min (OS idle time) → ask on return: *discard idle time / keep / assign to other project* | 10 min, prompt |
| **Lock / Suspend** | Screen locked or system sleep → pause from lock time; resume prompt on unlock | on |
| **Night / end of day** | Timer still running after `latest_end` of the schedule or a fixed time (e.g. 22:00) → stop at last activity | 22:00 |
| **Max day length / rest time** | Warn when work exceeds the profile's daily max, or when rest between days is below its minimum; org-configurable | From profile (AT: 12 h / 11 h rest) |

No auto-pause rule hardcodes a legal value. All defaults come from the active country profile (§5.13), and users or orgs may only make them stricter unless the profile marks a value as `advisory`.

Platform notes: idle time via the `user-idle` crate (X11/Wayland support varies; detect and fall back to lock events). Suspend/resume and lock events are platform-specific (Windows `WTSRegisterSessionNotification`, macOS `NSWorkspace` notifications, Linux logind D-Bus `PrepareForSleep` / `Lock`).

### 5.4 Work Hours
- Schedule models: **fixed**, **flextime** (core hours and a range), **trust-based** (target only).
- Weekly schedule with per-day targets. Schedules have validity ranges, so a change from 40 h to 30 h on 2027-01-01 keeps history correct.
- Public holidays: computed from the active country profile and region (§5.13), plus custom ones.
- Vacation entitlement defaults, week start, overtime thresholds: from the country profile.
- Absences: request (org) or record (personal); half days; remaining vacation days.
- **Balance**: running flexitime balance, weekly and monthly overtime, manual adjustments with audit trail.
- **Period close / timesheet approval** (org): the user submits a month, a manager approves, and the entries become `locked`.

### 5.5 Dashboard
Widgets (rearrangeable, persisted per user):
- **Today**: worked vs target, current timer, pauses taken, timeline bar
- **Week**: bar chart of worked vs target per day (replaces current demo `BarChart.vue`)
- **Balance**: flexitime balance, vacation remaining
- **Projects**: time per project (donut), budget burn-down with warnings
- **Cost centers**: distribution for the selected period
- **Calendar**: upcoming meetings today, one click to "track this meeting"
- **Team** (org): online users, who is in the office / home office today

Reports page: filter by period, user(s), group, project, tag, cost center or billable. Group by any of these. Exports:
- **CSV / XLSX** (generic)
- **PDF timesheet** (monthly, signable)

### 5.6 Cost Centers
- Hierarchical list with code, name and validity.
- Defaults on project, user and group. Resolution order is in §4.1.
- Reports and exports aggregate by cost center. In the org edition, only admins can edit the list.

### 5.7 Calendar Integration
Delivered as first-party **addons** (§5.12), not built into the app. The addon host provides the secret store, OAuth helper, scheduled sync and a timeline contribution they need.
- Providers:
  - **Microsoft 365 / Outlook** (Microsoft Graph, OAuth 2 PKCE, loopback redirect)
  - **Google Calendar** (OAuth 2 PKCE)
  - **CalDAV** and **ICS URL** (read-only)
- **Read**: show events in the day timeline, suggest entries from events, auto-create entries via mapping rules (opt-in), and pause or switch the timer on meeting start (opt-in).
- **Write** (org Meetings, §5.8): create, update and cancel events in the organizer's calendar and send invites through the provider.
- **Out-of-office**: approved absences can create OOF events. Calendar OOF can suggest absences.
- Sync runs every 5 min plus on window focus, with incremental delta tokens (Graph `delta`, Google `syncToken`).

### 5.8 Meetings (Org)
- Schedule a meeting with org users and/or external emails. Shows a free/busy grid from attendees' work schedules, absences, work location and calendar busy times.
- The meeting is linked to a project and cost center. At meeting time, each attendee gets a prompt to *track this meeting* (or it is tracked automatically if they opted in).
- Bidirectional sync with the organizer's calendar provider. RSVPs sync back.
- Recurrence via RRULE.

### 5.9 User Management (Org)
- Org with **groups** (hierarchical departments/teams).
- Roles:
  - **Owner**: billing and everything else
  - **Admin**: users, groups, cost centers, settings, addons allowlist
  - **Manager**: reads entries of their group(s) **with full project-level detail** (project, tags, cost center, note, start/end); approves entries and absences
  - **Member**: own data
- **Keycloak is the source of truth** for identities, org membership, org roles and groups (details in §7.2). Admins manage everything from the app's Admin pages. The API calls the Keycloak Admin REST API, so admins never need the Keycloak console.
- Invite by email (Keycloak Organizations invitations), deactivate (Keycloak user disabled, data kept), SSO and LDAP through Keycloak identity providers / user federation.
- Fine-grained permission checks, e.g. "manager may read entries of members of their groups", are a policy layer in the API: `can(actor, action, resource)`. The rule table lives in `packages/shared-fixtures/permissions.yaml` and is shared with the desktop, which uses it only to hide or show UI. The server is authoritative.
- Per-org policies: required tags, auto-pause defaults, whether members may edit locked days, presence visibility, manager visibility (`full` default · `hide_notes` · `totals_only`).

### 5.10 Online Users (Org)
- Presence over the server WebSocket. States: `online`, `tracking` (optionally with project, if the user allows it), `in_meeting`, `away` (idle/locked), `offline`.
- **Privacy defaults**: show status only, not project. The user can opt in to sharing the current project.
- Shown in a sidebar panel and the dashboard widget, filterable by group.

### 5.11 Work Location View (Org)
- Week/month grid: rows = users (grouped by group), columns = days. Cells show 🏢 office / 🏠 home / ✈️ travel / 🌴 absent / holiday.
- Users plan their week ahead (default pattern, e.g. Mon/Wed office). The actual location can be set by a one-click tray prompt in the morning (or later, office Wi-Fi SSID detection as an addon).
- Office capacity per day (e.g. desk count) with a warning when full.

### 5.12 Addon System
- **Manifest** (`addon.toml`): `id, name, version, author, min_app_version, permissions[], hooks[], ui_contributions[]`
- **Runtime**: WASM modules via **Extism**. The host exposes a capability-scoped API:
  - read: `entries`, `projects`, `tags`, `calendar` (per permission)
  - write: `create_entry`, `set_tags`, `notify`
  - `http_fetch` to declared domains only
  - `kv_store` (per addon)
- **Hooks** (event bus): `entry.started`, `entry.stopped`, `entry.created/updated/deleted`, `day.closed`, `report.export` (custom export formats), `quickswitch.suggest`, `schedule.tick`.
- **UI contributions** (v2): settings panel (declarative JSON form schema), dashboard widget (declarative chart spec), report export format. No arbitrary HTML/JS in v1, for security.
- **Distribution**: local install from file in v1. Later a signed registry, and orgs can allowlist addons.
- First-party addons to prove the API: calendars (§5.7, the first ones), Jira/GitHub issue linking, configurable CSV export format, Timeular BLE tracker, office Wi-Fi detection.

### 5.13 Country Profiles (Jurisdictions)
All country-specific behaviour is **data, not code**. Adding a country means adding one file plus fixture tests. No Rust or Vue changes are needed.

- **Location**: `crates/time-core/jurisdictions/<ISO-3166-1>.toml` (e.g. `AT.toml`, `DE.toml`, `CH.toml`). The files are embedded at build time. Addons and self-hosted servers can provide additional or overriding profiles.
- **Inheritance**: `extends = "AT"` plus region sections (`[regions."AT-9"]` for Vienna, `[regions."DE-BY"]` for Bavaria). A region only lists what differs.
- **Profile contents**:

```toml
id = "AT"
name = { de = "Österreich", en = "Austria" }
version = "2026.1"                # profiles are versioned; calculations record which version they used
week_start = "monday"
default_locale = "de-AT"
duration_format = "decimal"       # or "hm"

[work_time]
max_daily_minutes   = 720         # 12 h incl. overtime (AZG §9)
max_weekly_minutes  = 3600        # 60 h incl. overtime
min_daily_rest_minutes = 660      # 11 h
standard_weekly_minutes = 2400    # 40 h default schedule

[[breaks]]                        # the rule with the highest matching threshold wins
after_minutes = 360               # > 6 h worked
break_minutes = 30
min_part_minutes = 10             # gaps of at least this length count towards the break
mode = "required"                 # required | advisory

[vacation]
default_days = 25
unit = "workdays"                 # or "weekdays" / "hours"

[[holidays]]                      # rule-based, so no yearly data updates are needed
name = { de = "Neujahr", en = "New Year's Day" }
rule = { fixed = "01-01" }
[[holidays]]
name = { de = "Ostermontag", en = "Easter Monday" }
rule = { easter_offset = 1 }
[[holidays]]
name = { de = "Muttertag", en = "Mother's Day" }
rule = { nth_weekday = { month = 5, weekday = "sun", n = 2 } }
observed = false                  # informational, not a day off

[regions."AT-9".holidays_add]     # region-specific additions/removals
# ...

[exports]
default = ["csv", "xlsx"]         # suggested export formats; addons can register more
```

- **Holiday rule types**: `fixed`, `easter_offset` (Western/Orthodox), `nth_weekday`, `last_weekday`, `weekday_after/before`, `substitute_if_weekend`, `valid_from/valid_to` (for law changes).
- **Selection**: a workspace sets a profile and region during onboarding (auto-suggested from the OS locale). In the org edition, groups or users can override it, e.g. a German subsidiary inside an Austrian org (see `JurisdictionSelection`, §4.1).
- **Overrides**: orgs can change any value through `overrides_json`. The UI marks values that are looser than a `required` legal minimum with a warning and an audit log entry.
- **Versioning**: when a profile changes (law change), old days keep the profile version they were computed with. Recomputing is explicit.
- **Validation**: a JSON Schema for the profile format, `cargo test` loads every profile, and each profile ships a fixture file (`AT.fixtures.toml`) with expected holidays for 3 years and expected break/balance results for sample days.
- **Disclaimer**: profiles are best-effort defaults, not legal advice. Each profile lists `sources` (links to the law text) and a `maintainer`.
- **Launch profiles**: `AT` (primary), `DE`, `CH`. Community-contributed profiles are accepted through PRs with fixtures.

---

## 6. Sync Protocol (Pro / Org)

1. Every local write goes through `time-core` and appends a `ChangeLog` row in the same transaction.
2. **Push**: `POST /v1/sync/push { device_id, changes[] }`. The server validates permissions and applies field-level LWW by HLC, with rule overrides (e.g. `locked` entries can only be changed by managers). It returns accepted and rejected changes with reasons.
3. **Pull**: `GET /v1/sync/pull?cursor=…` returns changes since the cursor for all workspaces the user can access (filtered by permission).
4. Realtime: the WebSocket notifies "changes available" and carries presence.
5. Conflicts that LWW cannot resolve (e.g. two running timers for one user from two devices) are surfaced as **conflict cards** in the UI.
6. First sync of an existing local workspace uploads it as the user's personal workspace. Joining an org does **not** move personal data unless the user chooses to.
7. Optional end-to-end encryption for the Pro personal workspace (later; org data cannot be E2EE because managers need to read it).
8. The merge (HLC compare, field-level LWW, rule overrides) is implemented in TypeScript on the server and in Rust on the client. Both run the same JSON test vectors from `packages/shared-fixtures/sync/`, so they cannot drift apart.

---

## 7. Server (Phase 5+)

### 7.0 Stack
| Concern | Choice |
|---|---|
| Runtime | Node.js 24 LTS (production image); `bun` only as package manager/dev runner |
| Framework | **Fastify 5** + `@fastify/type-provider-typebox` |
| API docs | `@fastify/swagger` (OpenAPI 3.1, generated from route schemas) + `@fastify/swagger-ui` at `/docs`. Swagger UI has an OAuth2 "Authorize" button against Keycloak (public client `time-swagger`, PKCE). It can be turned off in production with `DOCS_ENABLED=false`. |
| Auth | `jose`: verify Keycloak access tokens with JWKS (`createRemoteJWKSet`), checking `iss`, `aud=time-api`, `exp` and `azp` |
| DB | Postgres 17, **Drizzle ORM**, drizzle-kit SQL migrations, RLS as a second layer (`SET LOCAL app.workspace_id`) |
| Realtime | `@fastify/websocket` (presence, "changes available"); token checked on connect and re-checked when it expires |
| Domain logic | `@time/core-wasm` for work-time, balance, holiday and country-profile logic |
| Hardening | `@fastify/helmet`, `@fastify/cors`, `@fastify/rate-limit`, `@fastify/under-pressure` |
| Config | `@fastify/env` with a TypeBox schema; fails fast on invalid config |
| Observability | Pino (built in) JSON logs, OpenTelemetry (`@opentelemetry/sdk-node`), Prometheus `/metrics` |
| Tests | Vitest + `app.inject()`; Testcontainers for Postgres and Keycloak (with realm import) |

**Layout**
```
server/
├─ src/
│  ├─ app.ts                 buildApp(config) – registers plugins + modules (used by tests)
│  ├─ server.ts              entry point
│  ├─ config.ts              TypeBox env schema
│  ├─ plugins/               db, auth (JWT), authz (can()), swagger, websocket,
│  │                         keycloak-admin, license, errors (RFC 9457 problem+json)
│  ├─ modules/<feature>/     routes.ts · schemas.ts · service.ts · repo.ts
│  │                         sync, me, orgs, users, groups, projects, tags, cost-centers,
│  │                         entries (reports), absences, approvals, schedules, presence,
│  │                         locations, meetings, calendar, license, billing, keycloak-events, health
│  └─ db/                    schema.ts (Drizzle), migrations/
├─ test/
└─ Dockerfile
```

**API conventions**
- Base path `/v1`. Breaking changes need a new version; CI runs `oasdiff breaking` against the committed `openapi.json`.
- Every route declares `tags`, `summary`, `security`, a request schema and response schemas for all status codes. A route without a response schema fails a lint test.
- Errors use `application/problem+json` (RFC 9457) with a stable `code`.
- Pagination is cursor-based (`?cursor=&limit=`); IDs are UUIDv7.
- **Code generation from `openapi.json`**:
  - Desktop Rust client: `progenitor` → `crates/time-api-client`.
  - Vue: `openapi-typescript` + `openapi-fetch`.

**How the desktop talks to the API**
- **Sync and presence** run in Rust (in the background, even when the window is hidden).
- **Online-only org screens** (admin, team views, meetings) call the API from Vue with `openapi-fetch`, using `@tauri-apps/plugin-http` as the fetch implementation. That avoids CSP/CORS issues with a configurable server URL.
- The **access token** comes from the Rust command `auth_access_token()`. The refresh token never leaves Rust or the keychain.

### 7.2 Authentication & Authorization (Keycloak)

**Realm `time`, clients**

| Client | Type | Purpose |
|---|---|---|
| `time-desktop` | public, Auth Code + PKCE | Desktop login in the system browser. Redirect via loopback `http://127.0.0.1:<port>/callback` (RFC 8252), with `time://auth/callback` (tauri deep-link) as fallback. Scopes `openid profile email offline_access` so users stay signed in for weeks; sessions can be revoked in Keycloak. |
| `time-api` | confidential, bearer-only (audience) | Audience mapper adds `aud: time-api` to desktop tokens |
| `time-api-admin` | confidential, service account | API → Keycloak Admin REST API (manage users, groups, orgs, invitations). Only scoped `realm-management` roles: `manage-users`, `view-users`, `query-groups`, `manage-organizations`. |
| `time-swagger` | public, PKCE | Swagger UI "Authorize" (dev/staging; optional in prod) |
| `time-web` | public, PKCE | Future web app / admin console |

**Tenancy model**
- One realm. Each customer org is a **Keycloak Organization**, which provides email-domain routing to the org's own IdP (SSO) and invitations.
- Self-hosted uses the same realm layout with a single organization.

**Authorization: split of responsibilities**

| Layer | Owned by | What |
|---|---|---|
| Authentication | Keycloak | Login, MFA, password policy, brute-force protection, SSO brokering, LDAP federation |
| Coarse authorization (who is what, where) | Keycloak | Org membership; org role via group `/orgs/{orgId}/roles/{owner\|admin\|manager\|member}`; departments as groups `/orgs/{orgId}/groups/...` (hierarchical); realm roles `platform-admin`, `support` (cloud staff only) |
| Fine-grained authorization (which records) | API policy layer `can()` | Manager → their group's members' entries with project-level detail; project visibility/members; locked entries; presence privacy. Evaluated per request using token claims plus the mirrored group tree. |

We **do not use Keycloak Authorization Services (UMA/policies)** for per-record checks. They would add a network round-trip per decision and spread record-level logic across two systems.

**Token claims** (custom protocol mapper):
```json
{
  "sub": "…", "email": "…", "locale": "de",
  "orgs": { "<orgId>": { "role": "manager", "groups": ["<groupId>", "…"] } },
  "realm_access": { "roles": ["user"] }
}
```
The active org is chosen by the client with the `X-Org-Id` header and checked against the `orgs` claim.

**Mirroring into the API database**
App tables need foreign keys to users and groups (entries, schedules, cost-center defaults). The API keeps read-only mirror tables `kc_user`, `kc_group`, `kc_group_member` and `kc_org_member`, kept current by:
1. an upsert from token claims on every authenticated request (cheap, cached),
2. Keycloak admin/user events → webhook `POST /v1/internal/keycloak-events` (via a Keycloak event-listener extension; HMAC-signed),
3. a nightly reconcile job against the Admin API.

Writes always go to Keycloak first (through the API's admin endpoints), then the mirror.

**Pro (personal) users** register through Keycloak self-registration or Google/Microsoft social login. They have no organization; their data lives in their personal workspace.

**Login theme**: a custom Keycloak login theme in app branding, translated de/en (Keycloakify or FreeMarker), in `deploy/keycloak/themes/time`.

**Realm as code**: `deploy/keycloak/realm-time.json` is versioned and imported on start (`--import-realm`). Changes are applied with `keycloak-config-cli` in CI/CD so cloud and self-hosted stay identical.

### 7.1 Deployment: Cloud-first, Self-hostable
The **cloud** (EU region, GDPR) is the primary product and gets features first. The **self-hosted** edition runs the **same container images** (Time API + Keycloak + realm config) with a different configuration; there is no fork. Rules that keep this possible:

| Concern | Rule |
|---|---|
| Dependencies | Only **Postgres** and **Keycloak** are required. Self-hosters can use the bundled Keycloak or an existing Keycloak 26+ (import our realm). Optional: S3-compatible object storage (else local disk), SMTP (else email features disabled), Redis-free. No managed cloud services (no SQS, no Cognito, etc.). |
| Configuration | Everything via env vars: base URL, DB URL, `KEYCLOAK_URL` / realm / admin client secret, SMTP, storage, calendar OAuth client IDs (self-hosters register their own Microsoft/Google apps), log level. Validated at start-up. |
| Mode flag | `deployment_mode = "cloud" \| "self_hosted"`. Cloud-only modules (billing, signup, multi-org admin, usage telemetry) are compiled in but disabled in self-hosted mode. |
| Licensing | Cloud: entitlements come from billing. Self-hosted: an **offline license file** (same signed entitlement format: org, seats, features, expiry) is uploaded in the admin UI. No phone-home required; an optional online check gives automatic renewal. |
| Tenancy | Cloud is multi-tenant (RLS). Self-hosted usually runs a single org, but uses the same schema. |
| Client | The desktop app has a **server URL** setting (default: cloud). Discovery via `https://<company-domain>/.well-known/time-server` (returns API URL + Keycloak issuer) lets users type only their email or domain. Admins can preset the URL through an MDM/installer config file. |
| Packaging | `docker-compose.yml`: `api`, `keycloak` (with realm import + theme), `postgres` (databases `time` and `keycloak`), optional `caddy` for TLS. Available from the first org release. Helm chart and air-gapped install bundle later. |
| Upgrades | Versioned DB migrations that run automatically on start; realm changes applied by `keycloak-config-cli`; supported Keycloak version range documented; documented upgrade path; server ↔ client protocol versioned with a compatibility range (clients warn when the server is too old). |
| Operations | `/healthz`, `/readyz` (checks DB + Keycloak JWKS), Prometheus `/metrics`, structured JSON logs, `time-api backup` / `restore` CLI (app DB; the Keycloak DB is backed up with standard Postgres tools). |
| Country profiles | Self-hosters can mount extra/overriding profiles from a directory. |
| Telemetry | Off by default in self-hosted mode. |

Pricing difference (suggested): self-hosted is sold as annual seat licenses (Enterprise), the cloud as a monthly/annual subscription.

---

## 8. Frontend

### 8.1 Navigation (sidebar)
`Dashboard` · `Timesheet` (day/week timeline) · `Projects` · `Tags` · `Reports` · `Absences` · *(org)* `Team` (online users, work location) · `Meetings` · `Admin` (users, groups, cost centers, policies) · `Addons` · `Settings`

### 8.2 Windows
- **main**: the full app (existing)
- **quickswitch**: small, frameless, always-on-top palette, created on demand
- **prompt**: small notification-style window for idle, morning and forgotten-timer prompts (native notifications where actions are supported)

### 8.3 State
Pinia stores wrap generated bindings. Rust emits events (`timer://tick`, `entry://changed`, `presence://changed`) and stores subscribe to them. The UI never computes work-time math itself; it asks `time-core`.

### 8.4 i18n
German and English from day one (both exist). Dates and numbers use the user locale. Durations are shown as `7:42 h` and as decimal `7.70 h` (toggle; accounting systems often need decimal).

---

## 9. Non-Functional Requirements
- **Performance**: cold start to tray < 1 s; quick-switch palette open < 150 ms; dashboard for one year of data < 300 ms.
- **Reliability**: the DB uses WAL mode, automatic daily local backup (keep 14), and an integrity check on start. A crash while tracking loses at most the last heartbeat (30 s). The running entry is recovered on restart.
- **Security**: CSP enabled (currently `null`), minimal Tauri capabilities per window, secrets in the keychain, signed updates (Tauri updater), signed addon packages (later).
- **Accessibility**: keyboard-navigable everywhere; respects the reduced-motion setting.
- **Platforms**: Linux (Fedora/Ubuntu, X11 + Wayland), Windows 10/11, macOS 13+.

---

## 10. Testing
- `time-core`: unit tests for the work-time calculator (DST, cross-midnight, overlaps, schedule changes, holidays), auto-pause rules (simulated clock) and permission matrix. Property tests (`proptest`) for interval merging.
- Sync: deterministic multi-device simulation tests (two clients, one server, randomized op order → converge). Shared JSON test vectors run in both Rust and TS.
- Server: Vitest + `app.inject()` per module; integration tests with Testcontainers (Postgres + Keycloak with realm import, real tokens); authz matrix tests from `permissions.yaml`; `oasdiff` breaking-change check; schema-lint (every route has response schemas).
- Frontend: Vitest for stores and composables; Playwright (or WebdriverIO with tauri-driver) for smoke E2E.
- CI: GitHub Actions matrix (linux/windows/macos) with `cargo test`, `cargo clippy -D warnings`, `vue-tsc`, `vitest`, and a Tauri build. A server job runs lint, typecheck, tests, OpenAPI export + diff, Docker build, and a compose-stack E2E.

---

## 11. Data Ownership & Export
- Full export of all data as JSON plus CSV at any time, in every edition.
- Import from CSV (generic), Toggl and Clockify (later), and Timeular CSV.
- Deleting an account deletes server data within 30 days. Local data stays with the user.

## 12. Privacy (GDPR / Works Council)
- No screenshots, keystroke logging or app/URL tracking. Idle detection only reads "seconds since last input".
- Presence sharing of the current project is opt-in per user.
- **Managers see project-level detail** of their group members' time (projects, tags, cost centers, notes, individual entries), not only totals. This is a product decision; orgs that need it for their works council can turn on the policy `manager_visibility = totals_only` or `hide_notes`.
- Org admins can see what roles allow. Visibility rules are documented in-app so they can be shown to a works council (Betriebsrat).
- Audit log for edits to other users' entries and balances.

## 13. Glossary
| Term | Meaning |
|---|---|
| Alias | Short project code used in quick-switch and exports |
| Allotted time | Project budget in minutes per period |
| Balance / Gleitzeitsaldo | Accumulated worked − target time |
| Cost center / Kostenstelle | Accounting unit for cost allocation |
| Workspace | Data ownership boundary (personal or org) |
| HLC | Hybrid logical clock, used to order changes across devices |
| Realm | Keycloak's top-level container for users, clients and roles (we use one: `time`) |
| Keycloak Organization | Keycloak's tenant concept; one per customer org |

## 14. Open Questions
1. The pasted feature list ended with an empty bullet. Was a feature missing?
2. ~~BMD export~~ **Resolved:** built as a separate addon, not part of this spec.
3. ~~Primary jurisdiction~~ **Resolved:** Austria is the primary profile; all rules are country profiles (§5.13).
4. ~~Cloud vs self-hosted~~ **Resolved:** cloud-first, the same server is self-hostable (§7.1).
5. ~~Manager visibility~~ **Resolved:** managers see project-level detail by default (§5.9, §12).
6. Final product name and bundle identifier (currently `com.sebi.tray-app`).
