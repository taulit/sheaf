//! Back with a swipe of two fingers to the right on the touchpad (plan 03 §7).
//!
//! The touchpad's scrolling arrives as [`ScrollEvent`]s with exact pixel deltas. Under
//! Wayland GPUI 0.3.12 reports every one of them as [`TouchPhase::Moved`]: a gesture
//! has no start or end there, so a pause between events ends it too. A gesture goes
//! back once it moved far enough to the right and much further sideways than up or
//! down; one that started as vertical scrolling never does. Mouse wheels (deltas in
//! lines) and Shift, which turns a wheel sideways, never go back.
//!
//! Nothing in the note text scrolls sideways that a swipe could be meant for: the
//! editor scales wide tables down to fit and wraps code (`gpui-markdown-editor`
//! `v0.1.0`, `view/layout.rs`).

use std::time::{Duration, Instant};

use gpui::{
    DispatchPhase, Entity, IntoElement, ScrollDelta, ScrollEvent, ScrollSource, Styled as _,
    TouchPhase, canvas,
};

use super::NoteView;
use crate::app::actions::Back;

/// How far to the right a gesture must move to go back.
pub const DISTANCE: f32 = 140.;
/// How much further sideways than up or down it must have moved by then.
const DOMINANCE: f32 = 2.5;
/// Up or down movement after which a gesture counts as scrolling.
const VERTICAL: f32 = 24.;
/// A pause this long between two events starts a new gesture.
pub const PAUSE: Duration = Duration::from_millis(180);

/// The gesture in progress.
#[derive(Debug, Default)]
pub struct Swipe {
    dx: f32,
    dy: f32,
    last: Option<Instant>,
    /// It went back, or it is scrolling or going left: nothing more until it ends.
    decided: bool,
}

impl Swipe {
    /// Takes the next event at `now`; `true` once it completes a swipe back.
    pub fn feed(&mut self, event: &ScrollEvent, now: Instant) -> bool {
        let pause = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) >= PAUSE);
        self.last = Some(now);
        if pause || event.touch_phase == TouchPhase::Started {
            self.reset();
        }
        if matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.reset();
            return false;
        }
        let ScrollDelta::Pixels(delta) = event.delta else {
            return false;
        };
        if self.decided || event.source != ScrollSource::Pointer || event.modifiers.shift {
            self.decided = true;
            return false;
        }
        self.dx += delta.x.as_f32();
        self.dy += delta.y.as_f32();
        let sideways = self.dx.abs() >= DOMINANCE * self.dy.abs();
        if self.dy.abs() > VERTICAL && !sideways || self.dx <= -DISTANCE {
            self.decided = true;
            return false;
        }
        if self.dx >= DISTANCE && sideways {
            self.decided = true;
            return true;
        }
        false
    }

    fn reset(&mut self) {
        let last = self.last;
        *self = Swipe {
            last,
            ..Swipe::default()
        };
    }
}

/// Laid over the note text: watches the scrolling there before the editor takes it,
/// which it does for every event that also moves up or down.
pub fn listener(view: Entity<NoteView>) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |_, bounds, window, _| {
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture || !bounds.contains(&event.position) {
                    return;
                }
                let now = cx.background_executor().now();
                if view.update(cx, |view, _| view.swipe.feed(event, now)) {
                    window.dispatch_action(Box::new(Back), cx);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, point, px};

    use super::*;

    fn moved(dx: f32, dy: f32) -> ScrollEvent {
        ScrollEvent {
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            ..ScrollEvent::default()
        }
    }

    /// Feeds `events` 10 ms apart; whether one of them went back.
    fn swipes(swipe: &mut Swipe, start: Instant, events: &[ScrollEvent]) -> Vec<bool> {
        events
            .iter()
            .enumerate()
            .map(|(i, event)| swipe.feed(event, start + Duration::from_millis(10 * i as u64)))
            .collect()
    }

    #[test]
    fn a_swipe_to_the_right_goes_back_once() {
        let mut swipe = Swipe::default();
        let events = vec![moved(30., 2.); 8];
        let went = swipes(&mut swipe, Instant::now(), &events);
        assert_eq!(went.iter().filter(|went| **went).count(), 1);
        assert!(went[4], "back after 150 px: {went:?}");
    }

    #[test]
    fn scrolling_up_and_down_never_goes_back() {
        let mut swipe = Swipe::default();
        let mut events = vec![moved(1., -12.); 4];
        // Drifting sideways at the end of a scroll does not count either.
        events.extend(vec![moved(40., 0.); 6]);
        let went = swipes(&mut swipe, Instant::now(), &events);
        assert!(!went.contains(&true));
    }

    #[test]
    fn a_diagonal_movement_is_not_a_swipe() {
        let mut swipe = Swipe::default();
        let went = swipes(&mut swipe, Instant::now(), &vec![moved(30., 20.); 8]);
        assert!(!went.contains(&true));
    }

    #[test]
    fn short_or_leftward_swipes_do_nothing() {
        let mut swipe = Swipe::default();
        assert!(!swipes(&mut swipe, Instant::now(), &vec![moved(30., 0.); 4]).contains(&true));
        let mut swipe = Swipe::default();
        assert!(!swipes(&mut swipe, Instant::now(), &vec![moved(-30., 0.); 10]).contains(&true));
    }

    #[test]
    fn a_pause_or_an_end_starts_a_new_gesture() {
        let start = Instant::now();
        let mut swipe = Swipe::default();
        assert!(!swipes(&mut swipe, start, &vec![moved(1., 30.); 3]).contains(&true));
        let later = start + PAUSE * 2;
        assert!(swipes(&mut swipe, later, &vec![moved(30., 0.); 6]).contains(&true));

        let mut swipe = Swipe::default();
        let mut events = vec![moved(30., 0.); 3];
        events.push(ScrollEvent {
            touch_phase: TouchPhase::Ended,
            ..moved(0., 0.)
        });
        events.extend(vec![moved(30., 0.); 3]);
        assert!(!swipes(&mut swipe, start, &events).contains(&true));
    }

    #[test]
    fn wheels_shift_and_touchscreens_never_go_back() {
        let wheel = ScrollEvent {
            delta: ScrollDelta::Lines(point(5., 0.)),
            ..ScrollEvent::default()
        };
        let shifted = ScrollEvent {
            modifiers: Modifiers::shift(),
            ..moved(30., 0.)
        };
        let touch = ScrollEvent {
            source: ScrollSource::Touch,
            ..moved(30., 0.)
        };
        for event in [wheel, shifted, touch] {
            let mut swipe = Swipe::default();
            let events = vec![event; 10];
            assert!(!swipes(&mut swipe, Instant::now(), &events).contains(&true));
        }
    }
}
