//! Linux: lock state from logind (`LockedHint`) and the freedesktop
//! screensaver; idle time from GNOME's Mutter, the Wayland
//! `ext-idle-notify-v1` protocol (KDE, Sway, Hyprland, …) or the
//! freedesktop screensaver (X11), whichever works.

use std::sync::{Arc, Mutex};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use zbus::blocking::{Connection as Bus, Proxy};

use super::{ActivityProbe, Snapshot};

/// Wayland reports "idle" after this long without input; idle time is
/// counted from then on, so thresholds below this cannot be detected.
const WAYLAND_IDLE_TIMEOUT_MS: u32 = 30_000;

enum IdleSource {
    Mutter(Bus),
    Wayland(Arc<Mutex<Option<i64>>>),
    ScreenSaver(Bus),
    None,
}

pub struct LinuxProbe {
    idle: IdleSource,
    system: Option<Bus>,
    session: Option<Bus>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn mutter_idle(bus: &Bus) -> Option<i64> {
    let reply = bus
        .call_method(
            Some("org.gnome.Mutter.IdleMonitor"),
            "/org/gnome/Mutter/IdleMonitor/Core",
            Some("org.gnome.Mutter.IdleMonitor"),
            "GetIdletime",
            &(),
        )
        .ok()?;
    reply.body().deserialize::<u64>().ok().map(|ms| ms as i64)
}

fn screensaver_idle(bus: &Bus) -> Option<i64> {
    let reply = bus
        .call_method(
            Some("org.freedesktop.ScreenSaver"),
            "/org/freedesktop/ScreenSaver",
            Some("org.freedesktop.ScreenSaver"),
            "GetSessionIdleTime",
            &(),
        )
        .ok()?;
    // Seconds.
    reply.body().deserialize::<u32>().ok().map(|s| s as i64 * 1000)
}

impl LinuxProbe {
    pub fn new() -> Self {
        let session = Bus::session().ok();
        let system = Bus::system().ok();
        let idle = Self::pick_idle_source(session.as_ref());
        Self {
            idle,
            system,
            session,
        }
    }

    fn pick_idle_source(session: Option<&Bus>) -> IdleSource {
        if let Some(bus) = session {
            if mutter_idle(bus).is_some() {
                if let Ok(bus) = Bus::session() {
                    return IdleSource::Mutter(bus);
                }
            }
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            match wayland::spawn(WAYLAND_IDLE_TIMEOUT_MS) {
                Ok(state) => return IdleSource::Wayland(state),
                Err(err) => eprintln!("wayland idle notifications unavailable: {err}"),
            }
        }
        if let Some(bus) = session {
            if screensaver_idle(bus).is_some() {
                if let Ok(bus) = Bus::session() {
                    return IdleSource::ScreenSaver(bus);
                }
            }
        }
        IdleSource::None
    }

    fn locked(&self) -> Option<bool> {
        let logind = self.system.as_ref().and_then(|bus| {
            Proxy::new(
                bus,
                "org.freedesktop.login1",
                "/org/freedesktop/login1/session/auto",
                "org.freedesktop.login1.Session",
            )
            .ok()?
            .get_property::<bool>("LockedHint")
            .ok()
        });
        let screensaver = self.session.as_ref().and_then(|bus| {
            bus.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "GetActive",
                &(),
            )
            .ok()?
            .body()
            .deserialize::<bool>()
            .ok()
        });
        match (logind, screensaver) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
        }
    }
}

impl ActivityProbe for LinuxProbe {
    fn snapshot(&mut self) -> Snapshot {
        let idle_ms = match &self.idle {
            IdleSource::Mutter(bus) => mutter_idle(bus),
            IdleSource::ScreenSaver(bus) => screensaver_idle(bus),
            IdleSource::Wayland(since) => {
                let since = *since.lock().unwrap_or_else(|e| e.into_inner());
                Some(since.map_or(0, |s| (now_ms() - s).max(0)))
            }
            IdleSource::None => None,
        };
        Snapshot {
            idle_ms,
            locked: self.locked(),
        }
    }

    fn describe(&self) -> String {
        let idle = match self.idle {
            IdleSource::Mutter(_) => "gnome-mutter",
            IdleSource::Wayland(_) => "wayland ext-idle-notify",
            IdleSource::ScreenSaver(_) => "freedesktop screensaver",
            IdleSource::None => "unavailable",
        };
        format!("idle: {idle}; lock: logind/screensaver")
    }
}

mod wayland {
    use super::*;

    struct State {
        idle_since: Arc<Mutex<Option<i64>>>,
        timeout_ms: u32,
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for State {
        fn event(
            _: &mut Self,
            _: &wl_seat::WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtIdleNotifierV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotifierV1,
            _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtIdleNotificationV1, ()> for State {
        fn event(
            state: &mut Self,
            _: &ExtIdleNotificationV1,
            event: ext_idle_notification_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let mut since = state.idle_since.lock().unwrap_or_else(|e| e.into_inner());
            match event {
                ext_idle_notification_v1::Event::Idled => {
                    *since = Some(now_ms() - state.timeout_ms as i64);
                }
                ext_idle_notification_v1::Event::Resumed => *since = None,
                _ => {}
            }
        }
    }

    /// Subscribes to idle notifications on a background thread. Fails if
    /// the compositor does not support `ext-idle-notify-v1`.
    pub fn spawn(timeout_ms: u32) -> Result<Arc<Mutex<Option<i64>>>, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=9, ()).map_err(|e| e.to_string())?;
        let notifier: ExtIdleNotifierV1 = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
        let notification = notifier.get_idle_notification(timeout_ms, &seat, &qh, ());
        let idle_since = Arc::new(Mutex::new(None));
        let mut state = State {
            idle_since: idle_since.clone(),
            timeout_ms,
        };
        std::thread::Builder::new()
            .name("wayland-idle".into())
            .spawn(move || {
                // Keep the protocol objects alive for the lifetime of the thread.
                let _keep = (seat, notifier, notification);
                loop {
                    if let Err(err) = queue.blocking_dispatch(&mut state) {
                        eprintln!("wayland idle notifications stopped: {err}");
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(idle_since)
    }
}
