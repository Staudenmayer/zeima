//! Hybrid logical clock (HLC).
//!
//! Every write is stamped with an HLC so changes from different devices can be
//! ordered for sync. Timestamps are encoded as fixed-width strings, so
//! comparing the strings compares the timestamps:
//! `{wall_ms:015}-{counter:06}-{node}`.

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Timestamp {
    pub wall_ms: i64,
    pub counter: u32,
}

pub fn encode(ts: Timestamp, node: &str) -> String {
    format!("{:015}-{:06}-{}", ts.wall_ms, ts.counter, node)
}

pub fn decode(s: &str) -> Result<(Timestamp, &str)> {
    let invalid = || Error::InvalidHlc(s.to_string());
    let mut parts = s.splitn(3, '-');
    let wall_ms = parts.next().and_then(|p| p.parse().ok()).ok_or_else(invalid)?;
    let counter = parts.next().and_then(|p| p.parse().ok()).ok_or_else(invalid)?;
    let node = parts.next().filter(|n| !n.is_empty()).ok_or_else(invalid)?;
    Ok((Timestamp { wall_ms, counter }, node))
}

#[derive(Debug, Clone)]
pub struct HlcClock {
    last: Timestamp,
    node: String,
}

impl HlcClock {
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            last: Timestamp::default(),
            node: node.into(),
        }
    }

    /// Timestamp for a local event.
    pub fn tick(&mut self, now_ms: i64) -> String {
        if now_ms > self.last.wall_ms {
            self.last = Timestamp {
                wall_ms: now_ms,
                counter: 0,
            };
        } else {
            self.last.counter += 1;
        }
        encode(self.last, &self.node)
    }

    /// Merge a timestamp received from another device, so later local ticks
    /// sort after it.
    pub fn observe(&mut self, remote: &str, now_ms: i64) -> Result<()> {
        let (r, _) = decode(remote)?;
        let l = self.last;
        let wall_ms = now_ms.max(l.wall_ms).max(r.wall_ms);
        let counter = if wall_ms == l.wall_ms && wall_ms == r.wall_ms {
            l.counter.max(r.counter) + 1
        } else if wall_ms == l.wall_ms {
            l.counter + 1
        } else if wall_ms == r.wall_ms {
            r.counter + 1
        } else {
            0
        };
        self.last = Timestamp { wall_ms, counter };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_strictly_increasing_even_if_wall_clock_stalls_or_goes_back() {
        let mut c = HlcClock::new("a");
        let t1 = c.tick(1000);
        let t2 = c.tick(1000);
        let t3 = c.tick(900);
        let t4 = c.tick(2000);
        assert!(t1 < t2 && t2 < t3 && t3 < t4);
    }

    #[test]
    fn observe_moves_past_remote() {
        let mut a = HlcClock::new("a");
        let mut b = HlcClock::new("b");
        let remote = b.tick(5000);
        a.observe(&remote, 1000).unwrap();
        assert!(a.tick(1000) > remote);
    }

    #[test]
    fn roundtrip() {
        let s = encode(
            Timestamp {
                wall_ms: 42,
                counter: 7,
            },
            "dev-1",
        );
        let (ts, node) = decode(&s).unwrap();
        assert_eq!(
            ts,
            Timestamp {
                wall_ms: 42,
                counter: 7
            }
        );
        assert_eq!(node, "dev-1");
        assert!(decode("garbage").is_err());
    }
}
