//! Tray icon and menu. The icon keeps its look; one badge in its corner tells
//! two things at once: the badge's colour is the person's status (green
//! online, yellow idle, gray offline, red in a meeting, blue local mode), the
//! symbol in it the timer (a triangle while tracking, two bars while paused,
//! none when idle). The menu's top lines say both in words, and the menu offers
//! one-click switching between recently tracked activities.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
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

/// What the icon's colour tells: where the person stands for the organizations (see `presence.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Presence {
    /// Not signed in to a server: everything stays on this device. Blue.
    Local = 0,
    /// Signed in, but the server cannot be reached. Gray.
    Offline = 1,
    /// There. Green.
    Online = 2,
    /// Away from the machine or paused. Yellow.
    Idle = 3,
    /// In a meeting. Red.
    Meeting = 4,
}

const PRESENCES: [Presence; 5] = [
    Presence::Local,
    Presence::Offline,
    Presence::Online,
    Presence::Idle,
    Presence::Meeting,
];
static PRESENCE: AtomicU8 = AtomicU8::new(Presence::Local as u8);

impl Presence {
    fn from_u8(n: u8) -> Self {
        PRESENCES
            .into_iter()
            .find(|p| *p as u8 == n)
            .unwrap_or(Presence::Local)
    }
    fn label(self, lang: &str) -> &'static str {
        let de = lang == "de";
        match self {
            Presence::Local if de => "Lokaler Modus",
            Presence::Local => "Local mode",
            Presence::Offline if de => "Offline",
            Presence::Offline => "Offline",
            Presence::Online if de => "Online",
            Presence::Online => "Online",
            Presence::Idle if de => "Abwesend",
            Presence::Idle => "Idle",
            Presence::Meeting if de => "Im Termin",
            Presence::Meeting => "In a meeting",
        }
    }
    fn color(self) -> [u8; 3] {
        match self {
            Presence::Local => [0x25, 0x63, 0xeb],
            Presence::Offline => [0x9c, 0xa3, 0xaf],
            Presence::Online => GREEN,
            Presence::Idle => [0xea, 0xb3, 0x08],
            Presence::Meeting => [0xef, 0x44, 0x44],
        }
    }
}

/// Called by the presence reporter when the person's status changes: recolours the icon.
pub fn set_presence(app: &AppHandle, presence: Presence) {
    if PRESENCE.swap(presence as u8, Ordering::Relaxed) != presence as u8 {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { refresh(&app).await });
    }
}

pub struct Tray {
    icon: TrayIcon,
    /// The icon for each status and timer state.
    images: HashMap<(Presence, u8), Image<'static>>,
}

fn status_key(status: Status) -> u8 {
    match status {
        Status::Idle => 0,
        Status::Running => 1,
        Status::Paused => 2,
    }
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
    presence: Presence,
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
    let plain = Image::new_owned(base.rgba().to_vec(), base.width(), base.height());
    let mut images = HashMap::new();
    for presence in PRESENCES {
        for status in [Status::Idle, Status::Running, Status::Paused] {
            images.insert(
                (presence, status_key(status)),
                with_badge(&plain, presence.color(), status),
            );
        }
    }
    let idle = images[&(Presence::from_u8(PRESENCE.load(Ordering::Relaxed)), 0)].clone();

    let lang = app.state::<AppState>().settings().language();
    let menu = build_menu(
        app,
        &View {
            status: Status::Idle,
            status_line: text(lang, "not_tracking").into(),
            presence: Presence::from_u8(PRESENCE.load(Ordering::Relaxed)),
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
    app.manage(Tray { icon, images });
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
    let presence = Presence::from_u8(PRESENCE.load(Ordering::Relaxed));
    if let Some(icon) = tray.images.get(&(presence, status_key(view.status))) {
        let _ = tray.icon.set_icon(Some(icon.clone()));
    }
    let _ = tray.icon.set_tooltip(Some(format!(
        "Zeima · {} · {}",
        view.presence.label(view.lang),
        view.status_line
    )));
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
    let core_arc = crate::workspaces::running_core(&state).await;
    let core = &core_arc;
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
        presence: Presence::from_u8(PRESENCE.load(Ordering::Relaxed)),
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
    menu.append(&MenuItem::with_id(
        app,
        "presence",
        view.presence.label(lang),
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
            // `id` is already `start:…` (see `start_id`).
            menu.append(&MenuItem::with_id(
                app,
                id,
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
    let core_arc = crate::workspaces::running_core(&state).await;
    let core = &core_arc;
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
/// The badge in the icon's lower right corner: its colour is the person's status, the symbol the
/// timer (a triangle while tracking, two bars while paused, none when idle), white on the colour.
fn with_badge(base: &Image<'_>, color: [u8; 3], status: Status) -> Image<'static> {
    let (w, h) = (base.width() as i64, base.height() as i64);
    let mut rgba = base.rgba().to_vec();
    let radius = (w.min(h) as f64 * 0.2).max(2.5);
    let ring = radius * 1.18;
    let (cx, cy) = (w as f64 - ring - 1.0, h as f64 - ring - 1.0);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            let px = if d <= radius {
                if symbol_covers(status, dx / radius, dy / radius) {
                    [255, 255, 255, 255]
                } else {
                    [color[0], color[1], color[2], 255]
                }
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

/// Whether the timer's symbol covers the point (`x`, `y` in the badge, -1 to 1 from its centre).
fn symbol_covers(status: Status, x: f64, y: f64) -> bool {
    match status {
        Status::Idle => false,
        // A triangle pointing right.
        Status::Running => (-0.35..=0.55).contains(&x) && y.abs() <= 0.6 * (0.55 - x) / 0.9,
        // Two bars.
        Status::Paused => y.abs() <= 0.5 && ((-0.45..=-0.12).contains(&x) || (0.12..=0.45).contains(&x)),
    }
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
        // The id is used for the menu item as it is: one prefix, so a click parses back to the same start.
        assert!(start_id(&full).starts_with("start:a1|"));
        assert!(!start_id(&full).starts_with("start:start:"));
        assert!(parse_start_id("start:||").is_none());
        assert!(parse_start_id("stop").is_none());
    }

    #[test]
    fn the_badge_is_drawn_in_the_corner_in_the_status_colour() {
        let base = Image::new_owned(vec![0; 32 * 32 * 4], 32, 32);
        let badge = with_badge(&base, GREEN, Status::Idle);
        let px = |x: usize, y: usize| &badge.rgba()[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(px(0, 0), &[0, 0, 0, 0]);
        assert_eq!(px(31 - 10, 31 - 10), &[GREEN[0], GREEN[1], GREEN[2], 255]);
    }

    #[test]
    fn the_timer_is_a_white_symbol_in_the_badge() {
        let base = Image::new_owned(vec![0; 32 * 32 * 4], 32, 32);
        let white = |s| {
            with_badge(&base, GREEN, s)
                .rgba()
                .iter()
                .filter(|&&byte| byte == 255)
                .count()
        };
        // The ring is white too, so a symbol means more white pixels than none.
        assert!(white(Status::Running) > white(Status::Idle));
        assert!(white(Status::Paused) > white(Status::Idle));
        assert!(symbol_covers(Status::Running, 0.0, 0.0));
        assert!(symbol_covers(Status::Paused, 0.3, 0.0));
        assert!(!symbol_covers(Status::Paused, 0.0, 0.0));
        assert!(!symbol_covers(Status::Idle, 0.0, 0.0));
    }
}
