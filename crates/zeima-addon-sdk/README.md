# zeima-addon-sdk

The Rust SDK for Zeima addons: the types of every hook input and host call, the names of the host functions, and (with the `guest` feature, on `wasm32`) the `host` module an addon calls into.

An addon is a WASM module plus an `addon.toml`. Start with the developer guide, [`docs/addons.md`](../../docs/addons.md). Working examples are in [`addons/`](../../addons): `csv-export` (report format), `recent-notes` (events, storage, quick switch), `ics-calendar` (timeline, network), `day-digest` (meetings, absences, work days, `meeting.changed`).

```toml
[dependencies]
zeima-addon-sdk = { git = "<repository URL>", package = "zeima-addon-sdk", features = ["guest"] }
extism-pdk = "1.4"
```

What the SDK offers, by area:

| Area | Types | Host calls (permission) |
|---|---|---|
| Time | `Entry`, `NewEntry`, `Range`, `Project`, `Activity`, `Tag`, `CostCenter` | `list_entries` (`entries:read`), `create_entry` (`entries:write`), `list_projects`, `list_activities`, `list_tags`, `list_cost_centers` |
| Meetings | `Meeting`, `MeetingOccurrence`, `MeetingResponse` | `list_meetings` (`meetings:read`) |
| Absences and work time | `Absence`, `WorkDay`, `DayWarning`, `DateRange` | `list_absences` (`absences:read`), `list_work_days` (`work:read`) |
| Hooks | `Event` (helpers `entry`, `entry_id`, `date`, `day`, `change`, `meeting_id`, `meeting`, `status`, `occurrence_at`, `absence_id`, `absence`), `Tick`, `ExportRequest`, `SuggestRequest`, `TimelineRequest` | |
| Results | `ExportResult`, `Suggestion`, `TimelineEvent` | |
| Platform | | `notify` (`notify`), `kv_*`, `secret_*`, `oauth_token` (`network`) |

Every payload is JSON with camelCase fields and unknown fields are ignored, so an addon keeps working when Zeima adds fields. Within a major version fields are only added.

```sh
cargo check -p zeima-addon-sdk --target wasm32-unknown-unknown --features guest
```
