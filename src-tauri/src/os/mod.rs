//! OS activity: how long since the last input, and whether the screen is
//! locked. Each platform reports what it can; unknown values are `None`.
//! Sleep/suspend is detected platform-independently from clock jumps
//! (see `time_core::autopause::Detector`).

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod generic;
#[cfg(target_os = "linux")]
mod linux;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub idle_ms: Option<i64>,
    pub locked: Option<bool>,
}

pub trait ActivityProbe: Send {
    fn snapshot(&mut self) -> Snapshot;
    /// Which sources are in use, for logs and the settings page.
    fn describe(&self) -> String;
}

pub fn probe() -> Box<dyn ActivityProbe> {
    #[cfg(target_os = "linux")]
    return Box::new(linux::LinuxProbe::new());
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    return Box::new(generic::UserIdleProbe);
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    return Box::new(NoProbe);
}

#[allow(dead_code)]
struct NoProbe;

impl ActivityProbe for NoProbe {
    fn snapshot(&mut self) -> Snapshot {
        Snapshot::default()
    }

    fn describe(&self) -> String {
        "none".into()
    }
}

#[cfg(test)]
mod tests {
    /// Manual check on a real desktop: `cargo test -p time-app probe_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_live() {
        let mut probe = super::probe();
        println!("sources: {}", probe.describe());
        for _ in 0..3 {
            println!("{:?}", probe.snapshot());
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
