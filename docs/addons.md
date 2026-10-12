# Writing addons for Time

Addons add features to Time without changing the app: export formats, quick-switch options, reactions to tracked time, and integrations with other services. An addon is a WebAssembly (WASM) module plus a manifest, `addon.toml`. Time runs it in a sandbox, so it can only use what the user granted when installing it.

This guide walks through a complete export-format addon, then documents everything an addon can use. The two addons in [`addons/`](../addons) are working examples:

- [`csv-export`](../addons/csv-export): a report export format with settings.
- [`recent-notes`](../addons/recent-notes): events, storage, quick-switch options and notifications.

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
time-addon-sdk = { git = "<repository URL>", package = "time-addon-sdk" }
extism-pdk = "1.4"   # required: the #[plugin_fn] macro refers to it by name

[profile.release]
opt-level = "s"
lto = true
strip = true
```

Inside this repository, use `time-addon-sdk = { path = "../../crates/time-addon-sdk" }` instead.

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
use time_addon_sdk::{ExportRequest, ExportResult};

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

In Time, open **Addons → Install from file** and pick the zip. Time shows the permissions and hooks it asks for, then installs and enables it. Inside this repository, `addons/build.sh <folder>` does the build and the zip in one step.

While developing, you can pick the `addon.toml` itself instead of a zip; Time then reads the module named in `wasm` from the same folder. Install again after each build: an install with the same `id` replaces the old version and keeps its settings.

## The manifest

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Reverse-DNS id: lowercase letters, digits and `-`, dot-separated (`com.example.my-addon`). Identifies the addon across updates. |
| `name` | yes | Shown to users. |
| `version` | yes | `MAJOR.MINOR.PATCH`. |
| `author` | yes | Shown in the install dialog. |
| `description` | | One or two sentences for the Addons page. |
| `homepage` | | URL for more information. |
| `min_app_version` | | Oldest Time version the addon supports. Older apps refuse to install it. |
| `wasm` | | Module file, relative to `addon.toml`. Default `plugin.wasm`. |
| `permissions` | | What the addon may do (see [Permissions](#permissions)). |
| `allowed_hosts` | with `network` | Hosts for HTTP requests: `api.example.com` or `*.example.com`. No scheme, path or port. |
| `hooks` | | What the addon reacts to (see [Hooks](#hooks)). |
| `schedule_minutes` | with `schedule.tick` | Interval for `schedule.tick`, at least 1. |
| `[[export_formats]]` | with `report.export` | `id`, `name` (menu label), `extension` (without dot). |
| `[[settings]]` | | Fields of the addon's settings form (see [Settings](#settings)). |

Time validates the manifest when installing and refuses inconsistent ones, for example `network` without `allowed_hosts`. The JSON Schema is [`crates/time-core/addon.schema.json`](../crates/time-core/addon.schema.json); editors with TOML schema support can use it for completion.

## Hooks

Each hook calls one exported function. Declare only the hooks you implement: Time calls only those, and a declared hook without its export is reported as an error on the Addons page.

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

All inputs and outputs are JSON with camelCase fields; the SDK types handle the conversion. Unknown fields are ignored, so addons keep working when Time adds fields.

**Events.** `on_event` receives every event hook the addon declared; check `event.kind` (`"entry.stopped"` etc.). `Event::entry()`, `Event::entry_id()` and `Event::date()` read the common fields. `day.closed` arrives once per day after midnight, also for days the app missed (up to a week back), with `day` holding the work-time result: `netMs`, `targetMs`, `deltaMs`, `firstStart`, `lastEnd`, `holiday` and more. Starting the timer sends `entry.created` and `entry.started`; stopping or pausing sends `entry.updated` and `entry.stopped`.

Events about entries the addon created itself (with `host::create_entry`) are not sent back to it, so an addon cannot trigger itself in a loop.

**Exports.** `ExportRequest` holds what the user selected on the Reports page:

- `formatId`: which of your `export_formats` was picked.
- `range`, `groupBy`, `totalMs`, `billableMs`, `rows`: the grouped totals as shown.
- `entries`, `projects`, `tags`, `costCenters`: the matching entries and the names to resolve their ids.
- `regional`: the user's formats (`dateFormat`, `timeFormat`, `numberFormat`, `weekStart`, `timezone`, `language`), with "automatic" already resolved.

Return the whole file as `content`; Time asks the user where to save it.

**Quick switch.** `quickswitch_suggest` is called while the user types, with what they typed. Return up to 20 options. Choosing one starts the timer with its `projectId`, `tagIds` and `note`. Answer fast: suggestions that take longer than 0.8 s are dropped.

## Host API

The SDK's `host` module calls back into Time. Each call fails, and the hook with it, if the permission named here was not granted.

| Function | Permission | Returns |
|---|---|---|
| `host::list_entries(range)` | `entries:read` | Entries overlapping `range` |
| `host::list_projects()` | `projects:read` | All projects, archived ones included |
| `host::list_tags()` | `tags:read` | All tags |
| `host::list_cost_centers()` | `cost-centers:read` | All cost centers |
| `host::create_entry(new_entry)` | `entries:write` | The created entry |
| `host::notify(title, body)` | `notify` | – (the title is prefixed with your addon's name) |
| `host::kv_get(key)` / `kv_set` / `kv_delete` / `kv_keys(prefix)` | none | The addon's own storage |

**Times and time zones.** Times are unix milliseconds (UTC). Addons have no time zone database, so entries from `list_entries` and in exports also carry `localStart` and `localEnd` in the user's zone, like `2026-10-05T09:30`.

**Created entries** must be finished (`startAt` and `endAt`). They are validated like manual entries and recorded with source `addon`. `costCenterId: None` resolves the cost center like a manual entry would.

**Storage.** Each addon has its own key-value store: up to 10,000 keys of 256 bytes, values up to 1 MB. Store JSON for anything structured. Uninstalling the addon deletes its storage.

**HTTP.** With the `network` permission, use `extism_pdk::http::request`. Requests only reach the manifest's `allowed_hosts`; everything else fails.

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

Values reach the addon as Extism config strings. Read them with `extism_pdk::config::get("delimiter")`; unset fields get their `default`. Booleans are `"true"` or `"false"`. Time checks numbers and select values before saving, and reloads the addon with the new values.

## Permissions

| Permission | Allows |
|---|---|
| `entries:read` | `host::list_entries` |
| `entries:write` | `host::create_entry` |
| `projects:read` | `host::list_projects` |
| `tags:read` | `host::list_tags` |
| `cost-centers:read` | `host::list_cost_centers` |
| `notify` | `host::notify` |
| `network` | HTTP to `allowed_hosts` |

Ask for as little as you need: the install dialog lists every permission in plain words. Data passed to a hook, such as the entries in an `ExportRequest`, needs no permission.

## Limits

- One call may run for 10 seconds; quick-switch suggestions for 0.8 seconds.
- Each addon gets up to 64 MB of memory.
- The module may be at most 50 MB.
- Addons have no file system or clock access (no WASI). The current time arrives as `Tick.now`, or take it from the event you are handling.

When a call fails or hits a limit, Time shows the error on the Addons page and carries on.

## Versioning

Increase `version` with every release. Installing a package with the same `id` replaces the old version; settings and storage are kept. Use `min_app_version` when you rely on features of a newer Time. Within a major version of the SDK, new fields are only ever added, never removed or renamed.
