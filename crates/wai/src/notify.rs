use wai_agent::StopReason;

use crate::chat::{ChatEvent, ChatItem, ChatModel, TurnState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    NeedsYou,
    Done,
}

/// Whether a session event deserves an OS notification: only while the window is in the
/// background, when the session starts waiting for the user (`NeedsYou`) or a turn finishes
/// (`EndTurn`). A cancelled or cut-off turn and the replay of a loaded session do not count.
pub fn should_notify(
    event: &ChatEvent,
    state: &TurnState,
    window_active: bool,
) -> Option<NotifyKind> {
    match event {
        _ if window_active => None,
        ChatEvent::StateChanged if *state == TurnState::NeedsYou => Some(NotifyKind::NeedsYou),
        ChatEvent::TurnEnded(StopReason::EndTurn) => Some(NotifyKind::Done),
        _ => None,
    }
}

/// Title and body: the pending permission's title or question, or the start of the last agent
/// message.
pub fn message(kind: NotifyKind, model: &ChatModel) -> (String, String) {
    let title = model.title.as_deref().unwrap_or("New session");
    let mut items = model.items.iter().rev();
    match kind {
        NotifyKind::NeedsYou => {
            let body = items.find_map(|item| match item {
                ChatItem::Permission {
                    request,
                    answered: None,
                } => Some(request.title.clone()),
                ChatItem::Question {
                    request,
                    answered: None,
                } => Some(request.message.clone()),
                _ => None,
            });
            (format!("{title} needs you"), body.unwrap_or_default())
        }
        NotifyKind::Done => {
            let body = items.find_map(|item| match item {
                ChatItem::Agent { text } => Some(text.trim().chars().take(80).collect()),
                _ => None,
            });
            (format!("{title} is done"), body.unwrap_or_default())
        }
    }
}

// Tests never pop OS notifications: they record them per test thread.
#[cfg(test)]
thread_local! {
    static SENT: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The notifications this test thread asked for, as `(title, body)`.
#[cfg(test)]
pub fn sent() -> Vec<(String, String)> {
    SENT.with_borrow(Clone::clone)
}

/// Shows a native notification. A failure is logged, never shown.
pub fn notify(title: &str, body: &str) {
    #[cfg(test)]
    SENT.with_borrow_mut(|sent| sent.push((title.into(), body.into())));
    #[cfg(not(test))]
    show(title.to_owned(), body.to_owned());
}

/// Off the UI thread: on macOS, delivery waits up to 2 s for confirmation.
#[cfg(not(test))]
fn show(title: String, body: String) {
    std::thread::spawn(move || {
        // Unset, the first notification looks an app id up through AppleScript, which blocks.
        // ponytail: an unbundled binary borrows Terminal's identity; the app bundle (Phase 4)
        // brings its own.
        #[cfg(target_os = "macos")]
        {
            static APPLICATION: std::sync::Once = std::sync::Once::new();
            APPLICATION.call_once(|| {
                if let Err(e) = notify_rust::set_application("com.apple.Terminal") {
                    eprintln!("wai: notification identity not set: {e}");
                }
            });
        }
        if let Err(e) = notify_rust::Notification::new()
            .summary(&title)
            .body(&body)
            .show()
        {
            eprintln!("wai: notification failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use wai_agent::{PermissionRequest, StopReason};

    use super::{NotifyKind, message, should_notify};
    use crate::chat::{ChatEvent, ChatItem, ChatModel, TurnState};

    #[test]
    fn notification_policy() {
        use ChatEvent::{StateChanged, TurnEnded};
        use TurnState::*;
        let (end_turn, cancelled) = (StopReason::EndTurn, StopReason::Cancelled);
        let table = [
            (StateChanged, NeedsYou, false, Some(NotifyKind::NeedsYou)),
            (
                TurnEnded(end_turn.clone()),
                Idle,
                false,
                Some(NotifyKind::Done),
            ),
            (StateChanged, NeedsYou, true, None),
            (TurnEnded(end_turn), Idle, true, None),
            // A cancel, and the end of a loaded session's replay, also go Working -> Idle.
            (StateChanged, Idle, false, None),
            (TurnEnded(cancelled), Idle, false, None),
            (TurnEnded(StopReason::MaxTokens), Idle, false, None),
            (StateChanged, Working, false, None),
            (StateChanged, Error("boom".into()), false, None),
            (ChatEvent::TitleChanged, NeedsYou, false, None),
        ];
        for (event, state, active, want) in table {
            assert_eq!(
                should_notify(&event, &state, active),
                want,
                "{event:?} in {state:?}, window active: {active}"
            );
        }
    }

    #[test]
    fn notification_text() {
        let mut model = ChatModel {
            title: Some("Fix the build".into()),
            ..Default::default()
        };
        model.items.push(ChatItem::Agent {
            text: format!("  {}", "a".repeat(100)),
        });
        model.items.push(ChatItem::Permission {
            request: PermissionRequest {
                id: 1,
                session: "s".into(),
                tool_call_id: "t".into(),
                title: "Run cargo test".into(),
                diff: None,
                options: Vec::new(),
            },
            answered: None,
        });
        assert_eq!(
            message(NotifyKind::NeedsYou, &model),
            ("Fix the build needs you".into(), "Run cargo test".into())
        );
        assert_eq!(
            message(NotifyKind::Done, &model),
            ("Fix the build is done".into(), "a".repeat(80))
        );
        model.title = None;
        assert_eq!(message(NotifyKind::Done, &model).0, "New session is done");
    }
}
