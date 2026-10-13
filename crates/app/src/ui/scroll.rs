//! Mouse-wheel gestures (303 pitch scrolls, knobs): a run of wheel edits
//! is one undo step, closed once the wheel has rested.

use super::SidAcidApp;

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
    /// Ends wheel gestures (303 pitch, knobs) once the wheel has rested.
    /// A gesture's undo step is closed only if no other edit came after
    /// it, so another open gesture is never cut short. Returns the time
    /// until the next gesture ends, if one is still open.
    pub(crate) fn tick_scroll_gestures(&mut self, now: f64) -> Option<f64> {
        let mut next: Option<f64> = None;
        let mut wait = |left: f64| next = Some(next.map_or(left, |n: f64| n.min(left)));
        for synth in 0..2 {
            let Some(g) = self.view.scroll_at[synth] else {
                continue;
            };
            if let Some(left) = g.remaining(now) {
                wait(left);
            } else {
                self.view.scroll_at[synth] = None;
                self.view.scroll_accum[synth] = 0.0;
                self.close_scroll_step(g);
            }
        }
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
    use crate::document::Document;
    use crate::ui::step_edit::nudge_note;
    use crate::ui::step303::StepOp;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()))
    }

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
    fn pitch_scrolls_close_together_are_one_undo_step() {
        let mut app = app();
        let slot = app.doc.project.selected[0];
        let start = app.doc.project.synths[0].patterns[slot as usize].steps[0];
        let frame = |app: &mut SidAcidApp, op: Option<StepOp>, now: f64| {
            if let Some(op) = op {
                app.apply_step_ops(0, slot, vec![(0, op)], now);
            }
            app.tick_scroll_gestures(now);
        };
        for t in [0.0, 0.1, 0.2] {
            frame(&mut app, Some(StepOp::Nudge(1)), t);
            frame(&mut app, None, t + 0.05);
        }
        frame(&mut app, None, 0.4);
        assert!(app.view.scroll_at[0].is_some(), "still open");
        frame(&mut app, None, 0.6);
        assert!(app.view.scroll_at[0].is_none());
        frame(&mut app, Some(StepOp::Nudge(1)), 1.6);
        frame(&mut app, None, 2.0);
        app.doc.undo();
        let mid = app.doc.project.synths[0].patterns[slot as usize].steps[0];
        assert_eq!(mid, nudge_note(start, 3), "three notches, one step");
        app.doc.undo();
        assert_eq!(
            app.doc.project.synths[0].patterns[slot as usize].steps[0],
            start
        );
        assert!(!app.doc.can_undo());
    }
}
