//! Tray icon and menu. The icon shows the timer state (a green dot while
//! tracking, an amber dot while paused) and the menu offers one-click
//! switching between recently tracked activities.

use std::collections::HashMap;
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};

use zeima_core::model::{RecentActivity, StartTimer, TimeEntry, TimerState};

use crate::commands;
use crate::events::DataKind;
use crate::state::AppState;
use crate::windows;

const RECENT_LIMIT: u32 = 5;
const GREEN: [u8; 3] = [0x22, 0xc5, 0x5e];
const AMBER: [u8; 3] = [0xf5, 0x9e, 0x0b];

pub struct Tray {
    icon: TrayIcon,
    idle: Image<'static>,
    running: Image<'static>,
    paused: Image<'static>,
}

#[derive(Clone, Copy)]
enum Status {
    Idle,
    Running,
    Paused,
}

/// Everything the menu shows, gathered before building it.
struct View {
    status: Status,
    status_line: String,
    /// Short elapsed time ("1:23") shown next to the icon where supported.
    title: Option<String>,
    /// Menu id (`start:…`, see [`start_id`]) and label.
    recent: Vec<(String, String)>,
    window_visible: bool,
    lang: &'static str,
}

fn text(lang: &str, key: &str) -> &'static str {
    let de = lang == "de";
    match key {
        "not_tracking" if de => "Keine Zeiterfassung",
        "not_tracking" => "Not tracking",
        "paused" if de => "Pausiert",
        "paused" => "Paused",
        "no_activity" if de => "Keine Tätigkeit",
        "no_activity" => "No activity",
        "stop" if de => "Stoppen",
        "stop" => "Stop",
        "pause" if de => "Pausieren",
        "pause" => "Pause",
        "resume" if de => "Fortsetzen",
        "resume" => "Resume",
        "start" if de => "Erfassung starten …",
        "start" => "Start tracking …",
        "quickswitch" if de => "Schnellwechsel …",
        "quickswitch" => "Quick switch …",
        "show" if de => "Fenster anzeigen",
        "show" => "Show window",
        "hide" if de => "Fenster ausblenden",
        "hide" => "Hide window",
        "quit" if de => "Beenden",
        "quit" => "Quit",
        _ => "",
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let base = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;
    let idle = Image::new_owned(base.rgba().to_vec(), base.width(), base.height());
    let running = with_dot(&idle, GREEN);
    let paused = with_dot(&idle, AMBER);

    let lang = app.state::<AppState>().settings().language();
    let menu = build_menu(
        app,
        &View {
            status: Status::Idle,
            status_line: text(lang, "not_tracking").into(),
            title: None,
            recent: vec![],
            window_visible: windows::main_visible(app),
            lang,
        },
    )?;
    let icon = TrayIconBuilder::with_id("main")
        .icon(idle.clone())
        .tooltip("Zeima")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;
    app.manage(Tray {
        icon,
        idle,
        running,
        paused,
    });
    Ok(())
}

/// Rebuilds the menu and icon from the current timer state.
pub async fn refresh(app: &AppHandle) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let view = match load_view(app).await {
        Ok(view) => view,
        Err(err) => {
            eprintln!("tray refresh failed: {err}");
            return;
        }
    };
    let icon = match view.status {
        Status::Idle => &tray.idle,
        Status::Running => &tray.running,
        Status::Paused => &tray.paused,
    };
    let _ = tray.icon.set_icon(Some(icon.clone()));
    let _ = tray
        .icon
        .set_tooltip(Some(format!("Zeima · {}", view.status_line)));
    let _ = tray.icon.set_title(view.title.as_deref());
    match build_menu(app, &view) {
        Ok(menu) => {
            let _ = tray.icon.set_menu(Some(menu));
        }
        Err(err) => eprintln!("tray menu failed: {err}"),
    }
}

async fn load_view(app: &AppHandle) -> zeima_core::Result<View> {
    let state = app.state::<AppState>();
    let core = &state.core;
    let lang = state.settings().language();
    let activities: HashMap<String, String> = core
        .list_activities(true)
        .await?
        .into_iter()
        .map(|a| (a.id, a.name))
        .collect();
    let projects: HashMap<String, String> = core
        .list_projects(true)
        .await?
        .into_iter()
        .map(|p| {
            let label = match &p.alias {
                Some(alias) => format!("{} ({alias})", p.name),
                None => p.name,
            };
            (p.id, label)
        })
        .collect();
    let tags: HashMap<String, String> = core
        .list_tags()
        .await?
        .into_iter()
        .map(|t| (t.id, t.name))
        .collect();
    // "Development · Website (WEB) · #review"
    let label = |activity_id: Option<&String>, project_id: Option<&String>, tag_ids: &[String]| {
        let mut parts = vec![activity_id
            .and_then(|id| activities.get(id))
            .cloned()
            .unwrap_or_else(|| text(lang, "no_activity").into())];
        parts.extend(project_id.and_then(|id| projects.get(id)).cloned());
        let tags: Vec<String> = tag_ids
            .iter()
            .filter_map(|id| tags.get(id))
            .map(|t| format!("#{t}"))
            .collect();
        if !tags.is_empty() {
            parts.push(tags.join(" "));
        }
        parts.join(" · ")
    };
    let entry_label = |e: &TimeEntry| label(e.activity_id.as_ref(), e.project_id.as_ref(), &e.tag_ids);

    let (status, status_line, title) = match core.timer_state().await? {
        TimerState::Idle => (Status::Idle, text(lang, "not_tracking").to_string(), None),
        TimerState::Running { entry } => {
            let elapsed = format_elapsed(core.now_ms() - entry.start_at);
            (
                Status::Running,
                format!("{} · {elapsed}", entry_label(&entry)),
                Some(elapsed),
            )
        }
        TimerState::Paused { resume_entry, .. } => {
            let line = match &resume_entry {
                Some(e) => format!("{} · {}", text(lang, "paused"), entry_label(e)),
                None => text(lang, "paused").to_string(),
            };
            (Status::Paused, line, None)
        }
    };
    let recent = core
        .recent_activities(RECENT_LIMIT)
        .await?
        .into_iter()
        .map(|r| {
            let label = label(Some(&r.activity_id), r.project_id.as_ref(), &r.tag_ids);
            (start_id(&r), label)
        })
        .collect();
    Ok(View {
        status,
        status_line,
        title,
        recent,
        window_visible: windows::main_visible(app),
        lang,
    })
}

/// `start:<activity>|<project or empty>|<tag,tag>`; ids contain none of `|,`.
fn start_id(r: &RecentActivity) -> String {
    format!(
        "start:{}|{}|{}",
        r.activity_id,
        r.project_id.as_deref().unwrap_or(""),
        r.tag_ids.join(",")
    )
}

fn parse_start_id(id: &str) -> Option<StartTimer> {
    let mut parts = id.strip_prefix("start:")?.split('|');
    let activity = parts.next().filter(|s| !s.is_empty())?;
    let project = parts.next().filter(|s| !s.is_empty());
    let tags = parts.next().unwrap_or("");
    Some(StartTimer {
        activity_id: Some(activity.to_string()),
        project_id: project.map(str::to_string),
        tag_ids: tags
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        note: String::new(),
        billable: None,
    })
}

fn build_menu(app: &AppHandle, view: &View) -> tauri::Result<Menu<Wry>> {
    let lang = view.lang;
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "status",
        &view.status_line,
        false,
        None::<&str>,
    )?)?;
    match view.status {
        Status::Running => {
            menu.append(&MenuItem::with_id(
                app,
                "pause",
                text(lang, "pause"),
                true,
                None::<&str>,
            )?)?;
            menu.append(&MenuItem::with_id(
                app,
                "stop",
                text(lang, "stop"),
                true,
                None::<&str>,
            )?)?;
        }
        Status::Paused => {
            menu.append(&MenuItem::with_id(
                app,
                "resume",
                text(lang, "resume"),
                true,
                None::<&str>,
            )?)?;
            menu.append(&MenuItem::with_id(
                app,
                "stop",
                text(lang, "stop"),
                true,
                None::<&str>,
            )?)?;
        }
        Status::Idle => {
            menu.append(&MenuItem::with_id(
                app,
                "start",
                text(lang, "start"),
                true,
                None::<&str>,
            )?)?;
        }
    }
    if !view.recent.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        for (id, label) in &view.recent {
            menu.append(&MenuItem::with_id(
                app,
                format!("start:{id}"),
                format!("▶ {label}"),
                true,
                None::<&str>,
            )?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quickswitch",
        text(lang, "quickswitch"),
        true,
        None::<&str>,
    )?)?;
    let toggle = if view.window_visible { "hide" } else { "show" };
    menu.append(&MenuItem::with_id(
        app,
        "toggle",
        text(lang, toggle),
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        text(lang, "quit"),
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref().to_string();
    let app = app.clone();
    match id.as_str() {
        "quit" => app.exit(0),
        // Starting needs an activity: the quick switch asks for it.
        "quickswitch" | "start" => windows::show_quickswitch(&app),
        _ => {
            tauri::async_runtime::spawn(async move {
                if let Err(err) = handle_async(&app, &id).await {
                    eprintln!("tray action '{id}' failed: {err}");
                }
            });
        }
    }
}

async fn handle_async(app: &AppHandle, id: &str) -> zeima_core::Result<()> {
    let state = app.state::<AppState>();
    let core = &state.core;
    match id {
        "toggle" => {
            windows::toggle_main(app).await;
            return Ok(());
        }
        "stop" => {
            core.stop_timer().await?;
        }
        "pause" => {
            core.pause_timer().await?;
        }
        "resume" => {
            core.resume_timer().await?;
        }
        _ => {
            let Some(start) = parse_start_id(id) else {
                return Ok(());
            };
            core.start_timer(start).await?;
        }
    }
    commands::changed(app, &[DataKind::Entry]).await;
    Ok(())
}

/// `h:mm`, e.g. `0:05` or `12:30`.
pub fn format_elapsed(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

/// Copy of `base` with a filled status dot (white ring) in the bottom-right.
fn with_dot(base: &Image<'_>, color: [u8; 3]) -> Image<'static> {
    let (w, h) = (base.width() as i64, base.height() as i64);
    let mut rgba = base.rgba().to_vec();
    let radius = (w.min(h) as f64 * 0.24).max(2.0);
    let ring = radius * 1.25;
    let (cx, cy) = (w as f64 - ring - 1.0, h as f64 - ring - 1.0);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            let px = if d <= radius {
                [color[0], color[1], color[2], 255]
            } else if d <= ring {
                [255, 255, 255, 255]
            } else {
                continue;
            };
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&px);
        }
    }
    Image::new_owned(rgba, w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_is_formatted_as_hours_and_minutes() {
        assert_eq!(format_elapsed(0), "0:00");
        assert_eq!(format_elapsed(5 * 60_000 + 59_000), "0:05");
        assert_eq!(format_elapsed(12 * 3_600_000 + 30 * 60_000), "12:30");
        assert_eq!(format_elapsed(-1), "0:00");
    }

    #[test]
    fn recent_menu_ids_round_trip() {
        let full = RecentActivity {
            activity_id: "a1".into(),
            project_id: Some("p1".into()),
            tag_ids: vec!["t1".into(), "t2".into()],
        };
        let start = parse_start_id(&start_id(&full)).unwrap();
        assert_eq!(start.activity_id.as_deref(), Some("a1"));
        assert_eq!(start.project_id.as_deref(), Some("p1"));
        assert_eq!(start.tag_ids, ["t1", "t2"]);

        let bare = RecentActivity {
            activity_id: "a1".into(),
            project_id: None,
            tag_ids: vec![],
        };
        let start = parse_start_id(&start_id(&bare)).unwrap();
        assert_eq!(start.project_id, None);
        assert!(start.tag_ids.is_empty());
        assert!(parse_start_id("start:||").is_none());
        assert!(parse_start_id("stop").is_none());
    }

    #[test]
    fn dot_is_drawn_in_the_corner() {
        let base = Image::new_owned(vec![0; 32 * 32 * 4], 32, 32);
        let dotted = with_dot(&base, GREEN);
        let px = |x: usize, y: usize| &dotted.rgba()[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(px(0, 0), &[0, 0, 0, 0]);
        assert_eq!(px(31 - 10, 31 - 10), &[GREEN[0], GREEN[1], GREEN[2], 255]);
    }
}
