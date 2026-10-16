//! Mouse-wheel gestures on knobs: a run of wheel edits is one undo step,
//! closed once the wheel has rested.

use super::SidAcidApp;

/// Scroll distance (points) that makes one notch.
pub const SCROLL_NOTCH: f32 = 40.0;

/// Adds `delta` to `accum` and returns the whole notches passed, keeping the remainder.
pub fn scroll_notches(accum: &mut f32, delta: f32) -> i32 {
    *accum += delta;
    let mut n = 0;
    while *accum >= SCROLL_NOTCH {
        *accum -= SCROLL_NOTCH;
        n += 1;
    }
    while *accum <= -SCROLL_NOTCH {
        *accum += SCROLL_NOTCH;
        n -= 1;
    }
    n
}

/// Seconds without wheel input that end a scroll gesture's undo step.
pub const SCROLL_IDLE: f64 = 0.3;

/// An open mouse-wheel gesture: when the wheel last moved, and the
/// document's edit serial after the gesture's last edit (None: no edit yet).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollGesture {
    pub last: f64,
    pub serial: Option<u64>,
}

impl ScrollGesture {
    /// Seconds until the gesture ends at `now`; None once the wheel has
    /// rested for `SCROLL_IDLE`.
    pub fn remaining(&self, now: f64) -> Option<f64> {
        let left = self.last + SCROLL_IDLE - now;
        (left > 0.0).then_some(left)
    }
}

impl SidAcidApp {
    /// Ends knob wheel gestures once the wheel has rested.
    /// A gesture's undo step is closed only if no other edit came after
    /// it, so another open gesture is never cut short. Returns the time
    /// until the next gesture ends, if one is still open.
    pub(crate) fn tick_scroll_gestures(&mut self, now: f64) -> Option<f64> {
        let mut next: Option<f64> = None;
        let mut wait = |left: f64| next = Some(next.map_or(left, |n: f64| n.min(left)));
        if let Some((_, g)) = self.view.knob_scroll {
            if let Some(left) = g.remaining(now) {
                wait(left);
            } else {
                self.view.knob_scroll = None;
                self.close_scroll_step(g);
            }
        }
        next
    }

    /// Ends a wheel gesture's undo step, unless a later edit took over.
    pub(crate) fn close_scroll_step(&mut self, g: ScrollGesture) {
        if g.serial == Some(self.doc.edit_serial()) {
            self.doc.end_gesture();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scroll_gesture_ends_after_the_wheel_rests() {
        let g = ScrollGesture {
            last: 10.0,
            serial: Some(3),
        };
        assert!(
            g.remaining(10.0)
                .is_some_and(|r| (r - SCROLL_IDLE).abs() < 1e-9)
        );
        assert!(
            g.remaining(10.1).is_some(),
            "100 ms between notches: still open"
        );
        assert!(g.remaining(10.0 + SCROLL_IDLE).is_none());
        assert!(g.remaining(11.0).is_none());
    }

    #[test]
    fn scroll_accumulates_and_fires_notches() {
        let mut a = 0.0;
        assert_eq!(scroll_notches(&mut a, 15.0), 0);
        assert_eq!(scroll_notches(&mut a, 15.0), 0);
        assert_eq!(scroll_notches(&mut a, 15.0), 1);
        assert_eq!(a, 5.0);
        assert_eq!(scroll_notches(&mut a, 100.0), 2);
        assert_eq!(a, 25.0);
        assert_eq!(scroll_notches(&mut a, -25.0 - 85.0), -2);
        assert_eq!(a, -25.0 + 25.0 - 85.0 + 80.0);
        let mut b = 0.0;
        assert_eq!(scroll_notches(&mut b, -50.0), -1);
        assert_eq!(b, -10.0);
    }
}
