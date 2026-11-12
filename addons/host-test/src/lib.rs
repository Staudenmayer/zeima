//! Test fixture: `schedule.tick` stores a secret, `timeline_events` reports
//! what the host returned for the secret and the OAuth token.

use extism_pdk::{plugin_fn, FnResult, Json};
use zeima_addon_sdk::{host, Tick, TimelineEvent, TimelineRequest};

const HOUR: i64 = 3_600_000;

#[plugin_fn]
pub fn on_schedule_tick(Json(tick): Json<Tick>) -> FnResult<()> {
    host::secret_set("api_key", &format!("key-{}", tick.now))?;
    host::secret_set("temporary", "x")?;
    host::secret_delete("temporary")
}

#[plugin_fn]
pub fn timeline_events(Json(req): Json<TimelineRequest>) -> FnResult<Json<Vec<TimelineEvent>>> {
    let token = host::oauth_token("demo")?;
    let key = host::secret_get("api_key")?;
    let start = req.range.from + 9 * HOUR;
    Ok(Json(vec![
        TimelineEvent {
            id: "standup".into(),
            title: format!("token={} key={}", token.unwrap_or_default(), key.unwrap_or_default()),
            start_at: start,
            end_at: start + HOUR / 4,
            detail: Some(req.timezone),
            color: Some("#22c55e".into()),
            ..Default::default()
        },
        // Ends before it starts: the host drops it.
        TimelineEvent {
            id: "broken".into(),
            title: "broken".into(),
            start_at: start,
            end_at: start - 1,
            ..Default::default()
        },
    ]))
}
