//! The speech bubble's timeline: pop, typing, hold, leave. Pure and clock-driven.

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Pop-in length.
pub(crate) const POP: Duration = Duration::from_millis(200);
/// Time to type one character.
pub(crate) const CHAR: Duration = Duration::from_millis(30);
/// Leave length.
pub(crate) const LEAVE: Duration = Duration::from_millis(150);
/// A sticky bubble collapses to "!" after this much hold time.
pub(crate) const STICKY_AFTER: Duration = Duration::from_secs(6);
/// Ticker period while animating.
pub(crate) const TICK: Duration = Duration::from_millis(30);

/// How long a typed bubble of `chars` characters stays: 2.5 s + 50 ms per char, at most 7 s.
pub(crate) fn hold(chars: usize) -> Duration {
    (Duration::from_millis(2_500) + Duration::from_millis(50) * chars as u32)
        .min(Duration::from_secs(7))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    Click,
    Timeout,
}

/// What to draw at an instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Look {
    /// 0..=1, eased with ease-out-back (may pass 1 briefly).
    pub pop: f32,
    /// Characters typed so far.
    pub chars: usize,
    /// Timer line, 1 to 0; 0 for a sticky bubble.
    pub left: f32,
    /// Sticky, shrunk to "!".
    pub collapsed: bool,
    /// 0..=1 once leaving.
    pub leave: f32,
}

pub(crate) struct Bubble {
    text: Arc<str>,
    chars: usize,
    sticky: bool,
    shown: Instant,
    last: Instant,
    held: Duration,
    leaving: Option<(Instant, Ended)>,
    ended: bool,
    pub hovered: bool,
}

fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    if t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let u = t - 1.;
    1. + (C1 + 1.) * u * u * u + C1 * u * u
}

impl Bubble {
    pub(crate) fn new(text: Arc<str>, sticky: bool, now: Instant) -> Self {
        Self {
            chars: text.chars().count(),
            text,
            sticky,
            shown: now,
            last: now,
            held: Duration::ZERO,
            leaving: None,
            ended: false,
            hovered: false,
        }
    }

    pub(crate) fn text(&self) -> &Arc<str> {
        &self.text
    }

    fn collapsed(&self) -> bool {
        self.sticky && self.held >= STICKY_AFTER
    }

    /// Moves to `now`. Hold time grows only while not `paused` and fully typed.
    /// Returns how it ended, once, when the leave finishes.
    pub(crate) fn advance(&mut self, now: Instant, paused: bool) -> Option<Ended> {
        if self.ended {
            return None;
        }
        if self.leaving.is_none() {
            if !paused {
                let typed_at = self.shown + CHAR * self.chars as u32;
                self.held += now.saturating_duration_since(self.last.max(typed_at));
            }
            let total = hold(self.chars);
            if !self.sticky && self.held >= total {
                // Back-date the leave to the instant the hold completed.
                self.leaving = Some((now - (self.held - total), Ended::Timeout));
                self.held = total;
            }
        }
        self.last = self.last.max(now);
        let (at, how) = self.leaving?;
        if now.saturating_duration_since(at) >= LEAVE {
            self.ended = true;
            return Some(how);
        }
        None
    }

    /// Starts the leave; false when already leaving or ended.
    pub(crate) fn click(&mut self, now: Instant) -> bool {
        if self.ended || self.leaving.is_some() {
            return false;
        }
        self.leaving = Some((now, Ended::Click));
        true
    }

    pub(crate) fn look(&self, now: Instant) -> Look {
        let since = now.saturating_duration_since(self.shown);
        let left = if self.sticky {
            0.
        } else {
            (1. - self.held.as_secs_f32() / hold(self.chars).as_secs_f32()).clamp(0., 1.)
        };
        let leave = self.leaving.map_or(0., |(at, _)| {
            (now.saturating_duration_since(at).as_secs_f32() / LEAVE.as_secs_f32()).clamp(0., 1.)
        });
        Look {
            pop: ease_out_back(since.as_secs_f32() / POP.as_secs_f32()),
            chars: ((since.as_millis() / CHAR.as_millis()) as usize).min(self.chars),
            left,
            collapsed: self.collapsed(),
            leave,
        }
    }

    /// Something changes over time: popping, typing, holding, or leaving. False once ended, or collapsed and not leaving.
    pub(crate) fn animating(&self, _now: Instant) -> bool {
        !self.ended && (self.leaving.is_some() || !self.collapsed())
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn hold_grows_with_length_and_caps() {
        assert_eq!(hold(0), ms(2_500));
        assert_eq!(hold(20), ms(3_500));
        assert_eq!(hold(140), ms(7_000));
    }

    #[test]
    fn types_one_char_per_30ms() {
        let t0 = Instant::now();
        let b = Bubble::new("abcdef".into(), false, t0);
        assert_eq!(b.look(t0).chars, 0);
        assert_eq!(b.look(t0 + ms(95)).chars, 3);
        assert_eq!(b.look(t0 + ms(1_000)).chars, 6);
    }

    #[test]
    fn counts_chars_not_bytes() {
        let t0 = Instant::now();
        let b = Bubble::new("a♥…".into(), false, t0);
        assert_eq!(b.look(t0 + ms(1_000)).chars, 3);
    }

    #[test]
    fn times_out_after_typing_plus_hold_plus_leave() {
        // "abc": typed at 90 ms, hold 2_650 ms, leave 150 ms → ends at 2_890 ms.
        let t0 = Instant::now();
        let mut b = Bubble::new("abc".into(), false, t0);
        let mut ended = None;
        for t in (0..=2_880).step_by(30) {
            ended = ended.or(b.advance(t0 + ms(t), false));
        }
        assert_eq!(ended, None);
        assert_eq!(b.advance(t0 + ms(2_900), false), Some(Ended::Timeout));
        assert!(b.ended());
        assert_eq!(b.advance(t0 + ms(3_000), false), None); // once only
    }

    #[test]
    fn paused_time_does_not_count() {
        let t0 = Instant::now();
        let mut b = Bubble::new("abc".into(), false, t0);
        for t in (0..=10_000).step_by(30) {
            assert_eq!(b.advance(t0 + ms(t), true), None);
        }
        assert!(b.look(t0 + ms(10_000)).left > 0.99);
    }

    #[test]
    fn click_leaves_then_reports_click() {
        let t0 = Instant::now();
        let mut b = Bubble::new("abc".into(), false, t0);
        assert!(b.click(t0 + ms(500)));
        assert!(!b.click(t0 + ms(510)));
        assert_eq!(b.advance(t0 + ms(600), false), None);
        assert_eq!(b.advance(t0 + ms(660), false), Some(Ended::Click));
    }

    #[test]
    fn sticky_never_times_out_and_collapses() {
        let t0 = Instant::now();
        let mut b = Bubble::new("Ei!".into(), true, t0);
        for t in (0..=60_000).step_by(30) {
            assert_eq!(b.advance(t0 + ms(t), false), None);
        }
        let look = b.look(t0 + ms(60_000));
        assert!(look.collapsed);
        assert_eq!(look.left, 0.);
        assert!(!b.animating(t0 + ms(60_000)));
    }

    #[test]
    fn clicked_collapsed_bubble_keeps_animating_until_it_leaves() {
        let t0 = Instant::now();
        let mut b = Bubble::new("Ei!".into(), true, t0);
        assert_eq!(b.advance(t0 + ms(7_000), false), None);
        assert!(!b.animating(t0 + ms(7_000)));
        assert!(b.click(t0 + ms(7_000)));
        assert!(b.animating(t0 + ms(7_000)));
        assert_eq!(b.advance(t0 + ms(7_150), false), Some(Ended::Click));
        assert!(!b.animating(t0 + ms(7_150)));
    }

    #[test]
    fn pop_overshoots_then_settles() {
        let t0 = Instant::now();
        let b = Bubble::new("abc".into(), false, t0);
        assert_eq!(b.look(t0).pop, 0.);
        assert!(b.look(t0 + ms(140)).pop > 1.);
        assert_eq!(b.look(t0 + ms(200)).pop, 1.);
    }

    #[test]
    fn one_big_step_ends_it() {
        // A stalled app, or a test clock jump: one advance crosses typing, hold and leave.
        let t0 = Instant::now();
        let mut b = Bubble::new("abc".into(), false, t0);
        assert_eq!(b.advance(t0 + ms(4_000), false), Some(Ended::Timeout));
    }
}
