use crate::chat::{ChatItem, ChatModel, TurnState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    NeedsYou,
    Done,
}

/// When a session's state change deserves an OS notification: only while the window is in the
/// background, when the session starts waiting for the user or a turn finishes.
pub fn should_notify(
    prev: &TurnState,
    next: &TurnState,
    window_active: bool,
) -> Option<NotifyKind> {
    match (prev, next) {
        _ if window_active => None,
        (_, TurnState::NeedsYou) => Some(NotifyKind::NeedsYou),
        (TurnState::Working, TurnState::Idle) => Some(NotifyKind::Done),
        _ => None,
    }
}

/// Title and body: the pending permission's title, or the start of the last agent message.
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

/// Shows a native notification off the UI thread (on macOS, delivery waits up to 2 s for
/// confirmation). A failure is logged, never shown.
pub fn notify(title: &str, body: &str) {
    // Tests never pop OS notifications.
    if cfg!(test) {
        return;
    }
    let (title, body) = (title.to_owned(), body.to_owned());
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
    use wai_agent::PermissionRequest;

    use super::{NotifyKind, message, should_notify};
    use crate::chat::{ChatItem, ChatModel, TurnState};

    #[test]
    fn notification_policy() {
        use TurnState::*;
        let error = || Error("boom".into());
        let table = [
            (Working, NeedsYou, false, Some(NotifyKind::NeedsYou)),
            (Working, Idle, false, Some(NotifyKind::Done)),
            (Working, NeedsYou, true, None),
            (Working, Idle, true, None),
            (Idle, Working, false, None),
            (NeedsYou, Working, false, None),
            (Working, error(), false, None),
            (error(), Idle, false, None),
        ];
        for (prev, next, active, want) in table {
            assert_eq!(
                should_notify(&prev, &next, active),
                want,
                "{prev:?} -> {next:?}, window active: {active}"
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
