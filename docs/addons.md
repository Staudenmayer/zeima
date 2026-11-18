# Writing addons for Zeima

Addons add features to Zeima without changing the app: export formats, quick-switch options, reactions to tracked time, items in the timesheet calendar, and integrations with other services. An addon is a WebAssembly (WASM) module plus a manifest, `addon.toml`. Zeima runs it in a sandbox, so it can only use what the user granted when installing it.

This guide walks through a complete export-format addon, then documents everything an addon can use: the hooks it can react to, the host API it can call, the permissions it needs and the data types of the SDK. The addons in [`addons/`](../addons) are working examples:

- [`csv-export`](../addons/csv-export): a report export format with settings.
- [`recent-notes`](../addons/recent-notes): events, storage, quick-switch options and notifications.
- [`day-digest`](../addons/day-digest): reads meetings, absences, work days and activities, uses the local date and time of a tick, and reacts to `meeting.changed` and `absence.changed`.

[`ics-calendar`](../addons/ics-calendar) shows an ICS feed in the timesheet (network with a user-entered address, `schedule.tick`, `timeline.events`, storage). [`host-test`](../addons/host-test) is a test fixture for secrets, accounts and timeline items; it is short enough to read as an example too.

## Quick start: an export format

You need Rust with the WASM target:

```sh
rustup target add wasm32-unknown-unknown
```

### 1. Create the crate

```sh
cargo new --lib hours-export && cd hours-export
```

`Cargo.toml`:

```toml
[package]
name = "hours-export"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
zeima-addon-sdk = { git = "<repository URL>", package = "zeima-addon-sdk" }
extism-pdk = "1.4"   # required: the #[plugin_fn] macro refers to it by name

[profile.release]
opt-level = "s"
lto = true
strip = true
```

Inside this repository, use `zeima-addon-sdk = { path = "../../crates/zeima-addon-sdk" }` instead.

### 2. Write the manifest

`addon.toml`, next to `Cargo.toml`:

```toml
id = "com.example.hours-export"     # reverse-DNS, lowercase
name = "Hours per project (text)"
version = "1.0.0"
author = "Example Ltd"
description = "Exports total hours per project as plain text."
min_app_version = "0.1.0"
wasm = "hours_export.wasm"          # the crate name with _ and .wasm
hooks = ["report.export"]

[[export_formats]]
id = "hours"
name = "Hours per project (TXT)"
extension = "txt"
```

The format appears in the Reports page's **Export** menu once the addon is installed.

### 3. Implement the hook

`src/lib.rs`:

```rust
use std::collections::HashMap;

use extism_pdk::{plugin_fn, FnResult, Json};
use zeima_addon_sdk::{ExportRequest, ExportResult};

#[plugin_fn]
pub fn export_report(Json(req): Json<ExportRequest>) -> FnResult<Json<ExportResult>> {
    let names: HashMap<_, _> = req.projects.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    let mut totals: HashMap<String, i64> = HashMap::new();
    for e in &req.entries {
        let name = e.project_id.as_ref().and_then(|id| names.get(id)).cloned().unwrap_or("–".into());
        let end = e.end_at.unwrap_or(e.start_at);
        *totals.entry(name).or_default() += end - e.start_at;
    }
    let mut lines: Vec<_> = totals.into_iter().collect();
    lines.sort();
    let content = lines
        .into_iter()
        .map(|(name, ms)| format!("{name}: {:.2} h\n", ms as f64 / 3_600_000.0))
        .collect();
    Ok(Json(ExportResult { content }))
}
```

### 4. Build, pack and install

```sh
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/hours_export.wasm .
zip hours-export-1.0.0.zip addon.toml hours_export.wasm
```

In Zeima, open **Addons → Install from file** and pick the zip. Zeima shows the permissions and hooks it asks for, then installs and enables it. Inside this repository, `addons/build.sh <folder>` does the build and the zip in one step.

While developing, you can pick the `addon.toml` itself instead of a zip; Zeima then reads the module named in `wasm` from the same folder. Install again after each build: an install with the same `id` replaces the old version and keeps its settings.

## A second example: reading the day

An addon can look at the person's day and react when it changes. This one (the core of [`day-digest`](../addons/day-digest)) sends one notification each morning with the day's meetings and target, and tells the person when a meeting is cancelled.

`addon.toml`:

```toml
id = "com.example.day-digest"
name = "Day digest"
version = "1.0.0"
author = "Example Ltd"
min_app_version = "1.0.0"
wasm = "day_digest.wasm"
permissions = ["meetings:read", "work:read", "notify"]
hooks = ["schedule.tick", "meeting.changed"]
schedule_minutes = 15
```

`src/lib.rs`:

```rust
use extism_pdk::{plugin_fn, FnResult, Json};
use zeima_addon_sdk::{host, DateRange, Event, Range, Tick};

#[plugin_fn]
pub fn on_schedule_tick(Json(tick): Json<Tick>) -> FnResult<()> {
    // Once a day, from 08:00 (the tick knows the user's local date and time).
    if tick.local_time.as_str() < "08:00"
        || host::kv_get("last")?.as_deref() == Some(tick.local_date.as_str())
    {
        return Ok(());
    }
    let day = DateRange { from: tick.local_date.clone(), to: tick.local_date.clone() };
    let target = host::list_work_days(day)?.first().map_or(0, |d| d.target_ms);
    // The next 24 hours of meetings the person has not declined.
    let meetings: Vec<_> = host::list_meetings(Range { from: tick.now, to: tick.now + 86_400_000 })?
        .into_iter()
        .filter(|m| !m.meeting.cancelled && m.my_status.as_deref() != Some("declined"))
        .collect();
    host::notify("Today", &format!("{} meetings, target {} min", meetings.len(), target / 60_000))?;
    host::kv_set("last", &tick.local_date)
}

#[plugin_fn]
pub fn on_event(Json(event): Json<Event>) -> FnResult<()> {
    if event.kind == "meeting.changed" && event.change().as_deref() == Some("cancelled") {
        if let Some(meeting) = event.meeting() {
            host::notify("Meeting cancelled", &meeting.title)?;
        }
    }
    Ok(())
}
```

The install dialog lists the permissions in plain words ("Read your meetings and what you answered", "Read your work days"), so the person knows what the addon sees.

## The manifest

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Reverse-DNS id: lowercase letters, digits and `-`, dot-separated (`com.example.my-addon`). Identifies the addon across updates. |
| `name` | yes | Shown to users. |
| `version` | yes | `MAJOR.MINOR.PATCH`. |
| `author` | yes | Shown in the install dialog. |
| `description` | | One or two sentences for the Addons page. |
| `homepage` | | URL for more information. |
| `min_app_version` | | Oldest Zeima version the addon supports. Older apps refuse to install it. |
| `wasm` | | Module file, relative to `addon.toml`. Default `plugin.wasm`. |
| `permissions` | | What the addon may do (see [Permissions](#permissions)). |
| `allowed_hosts` | with `network` | Hosts for HTTP requests: `api.example.com` or `*.example.com`. No scheme, path or port. |
| `hooks` | | What the addon reacts to (see [Hooks](#hooks)). |
| `host_settings` | | Keys of text settings whose value is an address the addon may request (a calendar feed, a self-hosted server). The host of each value is allowed in addition to `allowed_hosts`, so the user decides where the addon connects; the install dialog says so. Counts as the `network` host requirement. |
| `schedule_minutes` | with `schedule.tick` | Interval for `schedule.tick`, at least 1. |
| `[[export_formats]]` | with `report.export` | `id`, `name` (menu label), `extension` (without dot). |
| `[[settings]]` | | Fields of the addon's settings form (see [Settings](#settings)). |
| `[[oauth]]` | | Accounts the user can connect, such as Microsoft 365 or Google (see [Accounts](#accounts-oauth)). Needs `network`. |

Zeima validates the manifest when installing and refuses inconsistent ones, for example `network` without `allowed_hosts`. The JSON Schema is [`crates/zeima-core/addon.schema.json`](../crates/zeima-core/addon.schema.json); editors with TOML schema support can use it for completion.

## Hooks

Each hook calls one exported function. Declare only the hooks you implement: Zeima calls only those, and a declared hook without its export is reported as an error on the Addons page.

| Hook | Export | Input | Output |
|---|---|---|---|
| `entry.started` | `on_event` | `Event` with `entry` | – |
| `entry.stopped` | `on_event` | `Event` with `entry` (now with `endAt`) | – |
| `entry.created` | `on_event` | `Event` with `entry` | – |
| `entry.updated` | `on_event` | `Event` with `entry` | – |
| `entry.deleted` | `on_event` | `Event` with `entryId` | – |
| `day.closed` | `on_event` | `Event` with `date` and `day` | – |
| `meeting.changed` | `on_event` | `Event` with `change`, `meetingId` and, depending on the change, `meeting`, `status`, `occurrenceAt` | – |
| `absence.changed` | `on_event` | `Event` with `change`, `absenceId` and, unless deleted, `absence` | – |
| `schedule.tick` | `on_schedule_tick` | `Tick { now, timezone, localDate, localTime }` | – |
| `report.export` | `export_report` | `ExportRequest` | `ExportResult { content }` |
| `quickswitch.suggest` | `quickswitch_suggest` | `SuggestRequest { query }` | `Vec<Suggestion>` |
| `timeline.events` | `timeline_events` | `TimelineRequest { range, timezone }` | `Vec<TimelineEvent>` |

All inputs and outputs are JSON with camelCase fields; the SDK types handle the conversion. Unknown fields are ignored, so addons keep working when Zeima adds fields.

**Events.** `on_event` receives every event hook the addon declared; check `event.kind` (`"entry.stopped"` etc.). `Event::entry()`, `Event::entry_id()` and `Event::date()` read the common fields. `day.closed` arrives once per day after midnight, also for days the app missed (up to a week back), with `day` holding the work-time result: `netMs`, `targetMs`, `deltaMs`, `firstStart`, `lastEnd`, `holiday` and more. Starting the timer sends `entry.created` and `entry.started`; stopping or pausing sends `entry.updated` and `entry.stopped`.

Events about entries the addon created itself (with `host::create_entry`) are not sent back to it, so an addon cannot trigger itself in a loop.

**Meetings.** `meeting.changed` is sent when the person creates, changes, cancels or answers a meeting **on this device** (changes that arrive from other devices or from colleagues through sync do not send an event; read them with `host::list_meetings`). `Event::change()` tells which: `created`, `updated`, `cancelled`, `answered` (their own answer) or `answerCleared` (they took back the answer for one date). `Event::meeting_id()` is always there. `Event::meeting()` holds the meeting after the change for `created`, `updated` and `cancelled`; for `answered`, `Event::status()` is `accepted`, `tentative` or `declined`, and `Event::occurrence_at()` is the start of the date if the answer is for that date only (`None` for the whole series).

**Absences.** `absence.changed` is sent when the person adds, changes, deletes or decides an absence on this device. `Event::change()` is `created`, `updated`, `deleted`, or `approved` / `rejected` when the person decides a colleague's request; `Event::absence_id()` is always there, `Event::absence()` the absence after the change (not for `deleted`).

**Ticks.** `schedule.tick` runs every `schedule_minutes` (at least 1) while Zeima runs. The `Tick` carries the time in the user's zone, so an addon can act "from 08:00" or "once a day" without a time zone database: `timezone` (IANA), `localDate` (`2026-10-05`) and `localTime` (`08:30`, 24 hours), next to `now` (unix ms).

**Exports.** `ExportRequest` holds what the user selected on the Reports page:

- `formatId`: which of your `export_formats` was picked.
- `range`, `groupBy`, `totalMs`, `billableMs`, `rows`: the grouped totals as shown.
- `entries`, `projects`, `tags`, `costCenters`: the matching entries and the names to resolve their ids.
- `regional`: the user's formats (`dateFormat`, `timeFormat`, `numberFormat`, `weekStart`, `timezone`, `language`), with "automatic" already resolved.

Return the whole file as `content`; Zeima asks the user where to save it.

**Quick switch.** `quickswitch_suggest` is called while the user types, with what they typed. Return up to 20 options. Choosing one starts the timer with its `activityId`, `projectId`, `tagIds` and `note`; without `activityId` the user picks the activity first. Answer fast: suggestions that take longer than 0.8 s are dropped.

**Timeline.** `timeline_events` is called when the timesheet shows a day or week, with the shown `range` and the user's `timezone`. Return the items in that range, such as calendar events, as `TimelineEvent`s: `id`, `title`, `startAt`, `endAt`, and optionally `allDay`, `detail` (a second line, such as the location), `color` (`#rrggbb`) and the values for a new entry (`activityId`, `projectId`, `tagIds`, `note`). Timed items appear as dashed outlines behind the entries in the calendar view. Clicking one opens a new entry with the item's times and values ("Track this event"); the note defaults to the title. All-day items are listed in the day header and cannot be tracked.

Read the items from your own storage, not from the network: the call has 2 seconds, and the timesheet waits for it. Fetch and cache them in `schedule.tick` instead. After each tick the timesheet reloads the items, so new events show up without the user doing anything. At most 500 items per call are used; items that end before they start are dropped.

## Host API

The SDK's `host` module calls back into Zeima. Each call fails, and the hook with it, if the permission named here was not granted.

| Function | Permission | Returns |
|---|---|---|
| `host::list_entries(range)` | `entries:read` | Entries overlapping `range` |
| `host::list_projects()` | `projects:read` | All projects, archived ones included |
| `host::list_activities()` | `activities:read` | All activities, archived ones included |
| `host::list_tags()` | `tags:read` | All tags |
| `host::list_cost_centers()` | `cost-centers:read` | All cost centers |
| `host::list_meetings(range)` | `meetings:read` | The person's meetings overlapping `range` (unix ms), one item per date, with the answers |
| `host::list_absences(date_range)` | `absences:read` | The person's own absences overlapping the dates, with their approval status |
| `host::list_work_days(date_range)` | `work:read` | Target, worked time, balance, holiday and warnings for each local day (at most 366 days) |
| `host::create_entry(new_entry)` | `entries:write` | The created entry |
| `host::notify(title, body)` | `notify` | – (the title is prefixed with your addon's name) |
| `host::kv_get(key)` / `kv_set` / `kv_delete` / `kv_keys(prefix)` | none | The addon's own storage |
| `host::secret_get(key)` / `secret_set` / `secret_delete` | none | The addon's own secrets, in the OS keychain |
| `host::oauth_token(provider_id)` | `network` | An access token for a connected `[[oauth]]` account, or `None` |

**Times and time zones.** Times are unix milliseconds (UTC). Addons have no time zone database, so entries from `list_entries` and in exports also carry `localStart` and `localEnd` in the user's zone, like `2026-10-05T09:30`.

**Meetings, absences and work days.** These read what the person sees in the timesheet, the absence list and the balance:

- `list_meetings(Range { from, to })` returns `MeetingOccurrence`s: one per date a meeting happens, with the `meeting` (title, place, online link, activity, project, tags, attendees, repeat rule, `cancelled`), the `startAt` / `endAt` of that date, the `responses` of everyone (`accepted`, `tentative`, `declined`, per series or per date), the person's own `myStatus` for this date, their `seriesStatus`, `myDateAnswer` (they answered this date on its own) and `isOrganizer`. A person counts as taking part if they organize it or did not decline it. Meetings exist only in an organization's workspace; in a personal workspace the list is empty.
- `list_absences(DateRange { from, to })` takes local dates (`2026-10-05`, both inclusive) and returns the person's own `Absence`s: `typeId`, `typeName` and `typeKey` (`vacation`, `sick`, … or `None` for a custom type), `startDate`, `endDate`, `halfDay` (`am` / `pm`), `status` (`requested`, `approved`, `rejected`), `note` and the decision (`decidedBy`, `decisionNote`). Requests that wait for a decision and rejected ones are included: look at `status`.
- `list_work_days(DateRange { from, to })` returns a `WorkDay` per date: `targetMs`, `workedMs`, `netMs` (after break deductions), `deltaMs`, `adjustmentMs`, `breakTakenMs`, `breakRequiredMs`, `breakDeductedMs`, `firstStart`, `lastEnd`, `holiday` (a name if it is a public holiday), `absenceTypeIds` and `warnings` (`exceedsDailyMax`, `restTooShort`, `breakTooShort`, each as JSON with a `kind`). It is the same result `day.closed` sends.

**Created entries** must be finished (`startAt` and `endAt`). They are validated like manual entries and recorded with source `addon`. `costCenterId: None` resolves the cost center like a manual entry would.

**Storage.** Each addon has its own key-value store: up to 10,000 keys of 256 bytes, values up to 1 MB. Store JSON for anything structured. Uninstalling the addon deletes its storage.

**Secrets.** API keys and tokens belong in the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux), not in key-value storage. `host::secret_set(key, value)` stores up to 8 KB under the addon's id; no other addon can read it. Uninstalling the addon deletes its secrets. Keychain access can be slow or show an unlock prompt the first time, so read secrets when you need them rather than on every call.

**HTTP.** With the `network` permission, use `extism_pdk::http::request`. Requests only reach the manifest's `allowed_hosts` and the hosts of its `host_settings` values; everything else fails.

## Accounts (OAuth)

For services with OAuth 2, such as Microsoft 365 and Google, Zeima handles the sign-in. You declare the provider, the user clicks **Connect** on the Addons page, and you ask for an access token when you need one:

```toml
permissions = ["network"]
allowed_hosts = ["graph.microsoft.com"]

[[oauth]]
id = "microsoft"                     # letters, digits, _
name = "Microsoft 365"               # shown on the Addons page
auth_url = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
token_url = "https://login.microsoftonline.com/common/oauth2/v2.0/token"
client_id = "00000000-0000-0000-0000-000000000000"
scopes = ["offline_access", "Calendars.Read"]
```

```rust
let Some(token) = host::oauth_token("microsoft")? else {
    return Ok(()); // not connected yet
};
let req = HttpRequest::new("https://graph.microsoft.com/v1.0/me/calendarView?…")
    .with_header("Authorization", format!("Bearer {token}"));
```

How it works:

- **Connect** opens the provider's sign-in page in the system browser (authorization code flow with PKCE). The provider redirects to `http://127.0.0.1:<port>` on a free port, where Zeima receives the code and exchanges it for tokens. The user can cancel; the sign-in times out after 5 minutes.
- The refresh token is stored in the OS keychain. Access tokens are kept in memory only and renewed shortly before they expire. Your addon never sees the refresh token.
- `host::oauth_token` returns `None` while the account is not connected. If the provider rejects the refresh token (`invalid_grant`, for example after a password change), Zeima deletes it and the Addons page shows the account as not connected again. Treat `None` as "do nothing until the user connects".
- After connecting, the addon's next `schedule.tick` runs within 30 seconds instead of waiting for the full interval, so the first sync starts right away.
- Uninstalling the addon or clicking **Disconnect** deletes the refresh token. The provider may still list Zeima as an authorized app until the user removes it there.

Register Zeima as a **public client** (desktop/mobile app) with the provider:

- **Redirect URI**: `http://127.0.0.1` (the provider must accept any port on loopback addresses, as Microsoft and Google do). If the provider only accepts `http://localhost`, set `redirect_host = "localhost"`.
- **Microsoft**: "Mobile and desktop applications" platform. Include `offline_access` in `scopes`, or no refresh token is issued.
- **Google**: "Desktop app" client. Google requires its `client_secret` even for desktop apps (it is not secret in an app users install); add it as `client_secret`, and set `auth_params = { access_type = "offline", prompt = "consent" }` so that Google issues a refresh token.

`auth_url` and `token_url` must be `https://`. Zeima talks to them itself, so they do not need to be in `allowed_hosts`; the hosts of the API you call with the token do.

## Settings

`[[settings]]` entries become a form on the Addons page (gear button):

```toml
[[settings]]
key = "delimiter"            # letters, digits, _
label = "Delimiter"
type = "select"              # text | number | boolean | select | password
default = ";"
required = false
description = "Shown under the field."
options = [{ value = ";", label = "Semicolon" }, { value = ",", label = "Comma" }]  # select only
```

Values reach the addon as Extism config strings. Read them with `extism_pdk::config::get("delimiter")`; unset fields get their `default`. Booleans are `"true"` or `"false"`. Zeima checks numbers and select values before saving, and reloads the addon with the new values.

## Permissions

| Permission | Allows |
|---|---|
| `entries:read` | `host::list_entries` |
| `entries:write` | `host::create_entry` |
| `projects:read` | `host::list_projects` |
| `activities:read` | `host::list_activities` |
| `tags:read` | `host::list_tags` |
| `cost-centers:read` | `host::list_cost_centers` |
| `meetings:read` | `host::list_meetings`; the `meeting.changed` hook |
| `absences:read` | `host::list_absences`; the `absence.changed` hook |
| `work:read` | `host::list_work_days` |
| `notify` | `host::notify` |
| `network` | HTTP to `allowed_hosts`; `host::oauth_token` |

Ask for as little as you need: the install dialog lists every permission in plain words. Data passed to a hook, such as the entries in an `ExportRequest`, needs no permission. The `meeting.changed` and `absence.changed` hooks carry meetings and absences, so declare them together with `meetings:read` / `absences:read`: the install dialog then shows the person what the addon reads.

## Limits

- One call may run for 10 seconds; quick-switch suggestions for 0.8 seconds, timeline items for 2 seconds.
- Each addon gets up to 64 MB of memory.
- The module may be at most 50 MB.
- Addons have no file system or clock access (no WASI). The current time arrives as `Tick.now`, or take it from the event you are handling.

When a call fails or hits a limit, Zeima shows the error on the Addons page and carries on.

## Versioning

Increase `version` with every release. Installing a package with the same `id` replaces the old version; settings and storage are kept. Use `min_app_version` when you rely on features of a newer Zeima: the meeting, absence and work-day calls, the activities call and the `meeting.changed` / `absence.changed` hooks need `1.0.0`, and so does the local date and time in `Tick`. Within a major version of the SDK, new fields are only ever added, never removed or renamed.

**Changes in 1.0.** The SDK gained `host::list_activities`, `list_meetings`, `list_absences` and `list_work_days` with the types `Activity`, `Meeting`, `MeetingOccurrence`, `MeetingResponse`, `Absence`, `WorkDay` and `DateRange`; the events `meeting.changed` and `absence.changed` (with the `Event` helpers `change`, `meeting_id`, `meeting`, `status`, `occurrence_at`, `absence_id`, `absence`, `day`); and `timezone`, `localDate` and `localTime` on `Tick`. Tag groups no longer exist in the app: `Tag.group_name` is still read from old data, but do not rely on it.

## Signing and the registry

An addon runs code on the person's machine, inside a sandbox with the permissions they agreed to.
Signing tells them *who made it*, and the registry gives an installation one place to offer addons
from. Both are optional: an unsigned addon installs from a file as before, unless an organization
says otherwise (see below).

### Signing

A publisher has an Ed25519 key pair. The signature covers the manifest and the module, not the zip,
so it survives repacking and breaks if either file changes (more permissions, a swapped module).

```sh
node scripts/addons.mjs keygen acme-2026            # acme-2026.key (secret!) and the public key
addons/build.sh hours-export                         # builds addons/dist/<id>-<version>.zip
node scripts/addons.mjs sign addons/dist/<id>-<version>.zip --key acme-2026.key --key-id acme-2026
node scripts/addons.mjs verify addons/dist/<id>-<version>.zip --public <public key>
```

With `ZEIMA_SIGN_KEY=acme-2026.key ZEIMA_SIGN_KEY_ID=acme-2026`, `addons/build.sh` signs as part of the
build. The signature is the file `addon.sig` in the zip: `{"keyId": "...", "signature": "<base64url>"}`;
the message is `zeima-addon-v1\n<sha256 of addon.toml>\n<sha256 of the module>\n`
(`crates/zeima-core/src/addon_sig.rs`, test vectors in `packages/shared-fixtures/addons`).

The app trusts a key when it is built into the app (`BUILTIN` in `src-tauri/src/registry.rs`) or when
the signed-in server's registry lists it as a publisher. The install dialog and the addon list show:

| Shown | Meaning |
|---|---|
| Signed by *Name* | a trusted publisher signed exactly these files |
| Not signed | no signature |
| Signed by a publisher this app does not know | a signature, but nobody the app trusts vouches for the key |
| The signature does not match | the files were changed after signing, or are damaged; **cannot be installed** |

### The registry of a server

A server offers addons from a folder, `ADDON_REGISTRY_DIR` (`deploy/addon-registry` with the compose
file, a volume with the Helm chart). Make it with the same tool:

```sh
node scripts/addons.mjs publish deploy/addon-registry \
  --publisher acme-2026="Acme IT"=<public key> \
  addons/dist/com.acme.hours-export-1.2.0.zip
```

This copies the packages, writes `index.json` (id, name, version, permissions, hooks, size, SHA-256, the
key that signed it, and the publishers) and can be run again to add versions. The server serves the
index (`GET /v1/addons/registry`) and the packages to signed-in users and does nothing else with them:
**the app checks the hash and the signature itself.** In the app, Addons lists what the server offers
and installs it after the same consent dialog as for a file.

### What an organization can require

Owners and admins can restrict addons for their people (Organization → Addons, `PUT
/v1/orgs/:id/addons`): *any addon*, or *only the addons on a list* (by id), and optionally *only
signed addons* (signed by a publisher the app trusts). The apps enforce it for every organization the
person belongs to; the strictest one wins. A refused addon cannot be installed, and an installed one
stops running (the Addons page says why). The rules are kept on the device, so they apply offline, and
are refreshed every half hour and whenever the Addons page opens. This is a control for managed
installations, not protection against a user who modifies their own copy of the app.

