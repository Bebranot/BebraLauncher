//! Events that happened before the page could listen (P1-18).
//!
//! Tauri does not buffer events: everything emitted before the UI's `listen` is lost. On a cold
//! start that is exactly what happens — the `ss14://` link that started the launcher, the reason
//! the running game asked for a redial, the first `play` events of a launch prepared from the
//! command line. They are kept here until the UI asks for them once
//! (`take_pending_actions`), and from then on go straight to it as usual.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// How many actions are kept at most. A cold start produces a handful; the cap only keeps a
/// broken loop from growing memory.
const LIMIT: usize = 128;

/// One event with the payload it would have been emitted with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingAction {
    /// The event name the UI already listens to: `open-link`, `redial` or `play`.
    pub event: &'static str,
    /// The same payload the live event carries.
    pub payload: serde_json::Value,
}

#[derive(Default)]
pub struct PendingActions {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The UI has asked for the pending actions: later events go straight to it.
    ui_ready: bool,
    /// Oldest first.
    actions: Vec<PendingAction>,
}

impl PendingActions {
    /// Keeps the action until the UI asks for it. Returns `false` once the UI is listening:
    /// then the caller emits it live.
    fn push(&self, action: PendingAction) -> bool {
        let mut inner = crate::lock_or_recover(&self.inner);
        if inner.ui_ready {
            return false;
        }
        if inner.actions.len() >= LIMIT {
            inner.actions.remove(0);
        }
        inner.actions.push(action);
        true
    }

    /// Everything kept so far, oldest first, and marks the UI as listening: the next events
    /// are emitted live and are not repeated here.
    pub fn take(&self) -> Vec<PendingAction> {
        let mut inner = crate::lock_or_recover(&self.inner);
        inner.ui_ready = true;
        std::mem::take(&mut inner.actions)
    }

    #[cfg(test)]
    fn is_ui_ready(&self) -> bool {
        crate::lock_or_recover(&self.inner).ui_ready
    }
}

/// Emits an event now, or keeps it for a UI that is not listening yet.
pub fn emit<T: Serialize + Clone>(app: &AppHandle, event: &'static str, payload: T) {
    let Ok(value) = serde_json::to_value(&payload) else {
        crate::applog::line(&format!("event {event} could not be serialised; dropped"));
        return;
    };
    let kept = app.state::<crate::AppState>().pending.push(PendingAction {
        event,
        payload: value,
    });
    if !kept {
        let _ = app.emit(event, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(event: &'static str, payload: impl Serialize) -> PendingAction {
        PendingAction {
            event,
            payload: serde_json::to_value(payload).unwrap(),
        }
    }

    #[test]
    fn actions_are_kept_in_order_and_taken_once() {
        let pending = PendingActions::default();
        assert!(pending.push(action("open-link", "ss14://a:1212")));
        assert!(pending.push(action("play", 1)));
        assert!(pending.push(action("redial", 2)));
        assert!(!pending.is_ui_ready());

        let taken = pending.take();
        assert_eq!(
            taken,
            [
                action("open-link", "ss14://a:1212"),
                action("play", 1),
                action("redial", 2),
            ]
        );
        assert!(pending.is_ui_ready());
        assert!(pending.take().is_empty(), "nothing is repeated");
        assert!(
            !pending.push(action("play", 3)),
            "after the UI is listening, events go live"
        );
        assert!(pending.take().is_empty());
    }

    #[test]
    fn the_oldest_actions_are_dropped_past_the_limit() {
        let pending = PendingActions::default();
        for i in 0..LIMIT + 5 {
            assert!(pending.push(action("play", i)));
        }
        let taken = pending.take();
        assert_eq!(taken.len(), LIMIT);
        assert_eq!(taken[0].payload, serde_json::json!(5));
        assert_eq!(taken[LIMIT - 1].payload, serde_json::json!(LIMIT + 4));
    }

    #[test]
    fn nothing_is_kept_after_a_take() {
        let pending = PendingActions::default();
        assert!(pending.take().is_empty());
        assert!(!pending.push(action("play", 1)));
    }
}
