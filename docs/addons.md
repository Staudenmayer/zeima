# Writing addons for Zeima

Addons add features to Zeima without changing the app: export formats, quick-switch options, reactions to tracked time, items in the timesheet calendar, and integrations with other services. An addon is a WebAssembly (WASM) module plus a manifest, `addon.toml`. Zeima runs it in a sandbox, so it can only use what the user granted when installing it.

This guide walks through a complete export-format addon, then documents everything an addon can use. The two addons in [`addons/`](../addons) are working examples:

- [`csv-export`](../addons/csv-export): a report export format with settings.
- [`recent-notes`](../addons/recent-notes): events, storage, quick-switch options and notifications.

[`host-test`](../addons/host-test) is a test fixture for secrets, accounts and timeline items; it is short enough to read as an example too.

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
| `schedule.tick` | `on_schedule_tick` | `Tick { now }` | – |
| `report.export` | `export_report` | `ExportRequest` | `ExportResult { content }` |
| `quickswitch.suggest` | `quickswitch_suggest` | `SuggestRequest { query }` | `Vec<Suggestion>` |
| `timeline.events` | `timeline_events` | `TimelineRequest { range, timezone }` | `Vec<TimelineEvent>` |

All inputs and outputs are JSON with camelCase fields; the SDK types handle the conversion. Unknown fields are ignored, so addons keep working when Zeima adds fields.

**Events.** `on_event` receives every event hook the addon declared; check `event.kind` (`"entry.stopped"` etc.). `Event::entry()`, `Event::entry_id()` and `Event::date()` read the common fields. `day.closed` arrives once per day after midnight, also for days the app missed (up to a week back), with `day` holding the work-time result: `netMs`, `targetMs`, `deltaMs`, `firstStart`, `lastEnd`, `holiday` and more. Starting the timer sends `entry.created` and `entry.started`; stopping or pausing sends `entry.updated` and `entry.stopped`.

Events about entries the addon created itself (with `host::create_entry`) are not sent back to it, so an addon cannot trigger itself in a loop.

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
| `host::list_tags()` | `tags:read` | All tags |
| `host::list_cost_centers()` | `cost-centers:read` | All cost centers |
| `host::create_entry(new_entry)` | `entries:write` | The created entry |
| `host::notify(title, body)` | `notify` | – (the title is prefixed with your addon's name) |
| `host::kv_get(key)` / `kv_set` / `kv_delete` / `kv_keys(prefix)` | none | The addon's own storage |
| `host::secret_get(key)` / `secret_set` / `secret_delete` | none | The addon's own secrets, in the OS keychain |
| `host::oauth_token(provider_id)` | `network` | An access token for a connected `[[oauth]]` account, or `None` |

**Times and time zones.** Times are unix milliseconds (UTC). Addons have no time zone database, so entries from `list_entries` and in exports also carry `localStart` and `localEnd` in the user's zone, like `2026-10-05T09:30`.

**Created entries** must be finished (`startAt` and `endAt`). They are validated like manual entries and recorded with source `addon`. `costCenterId: None` resolves the cost center like a manual entry would.

**Storage.** Each addon has its own key-value store: up to 10,000 keys of 256 bytes, values up to 1 MB. Store JSON for anything structured. Uninstalling the addon deletes its storage.

**Secrets.** API keys and tokens belong in the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux), not in key-value storage. `host::secret_set(key, value)` stores up to 8 KB under the addon's id; no other addon can read it. Uninstalling the addon deletes its secrets. Keychain access can be slow or show an unlock prompt the first time, so read secrets when you need them rather than on every call.

**HTTP.** With the `network` permission, use `extism_pdk::http::request`. Requests only reach the manifest's `allowed_hosts`; everything else fails.

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
| `tags:read` | `host::list_tags` |
| `cost-centers:read` | `host::list_cost_centers` |
| `notify` | `host::notify` |
| `network` | HTTP to `allowed_hosts`; `host::oauth_token` |

Ask for as little as you need: the install dialog lists every permission in plain words. Data passed to a hook, such as the entries in an `ExportRequest`, needs no permission.

## Limits

- One call may run for 10 seconds; quick-switch suggestions for 0.8 seconds, timeline items for 2 seconds.
- Each addon gets up to 64 MB of memory.
- The module may be at most 50 MB.
- Addons have no file system or clock access (no WASI). The current time arrives as `Tick.now`, or take it from the event you are handling.

When a call fails or hits a limit, Zeima shows the error on the Addons page and carries on.

## Versioning

Increase `version` with every release. Installing a package with the same `id` replaces the old version; settings and storage are kept. Use `min_app_version` when you rely on features of a newer Zeima. Within a major version of the SDK, new fields are only ever added, never removed or renamed.
