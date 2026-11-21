//! Session content for extensions (API 1.10): the transcript as JSON, pages of it for
//! `sessions/read`. Pure functions; `workspace.rs` routes the calls and events to them.

use serde_json::{Value, json};
use wai_agent::{ChoiceKind, FieldKind, PermissionChoice, QuestionField, ToolStatus};

use crate::chat::{ChatItem, NoticeKind};

/// `sessions/read` without `limit`: the last 200 items.
pub const PAGE_DEFAULT: usize = 200;
/// The most items one `sessions/read` returns: a page stays under the 4 MB message cap.
pub const PAGE_MAX: usize = 500;

/// One transcript item. A tool call carries its title, kind and status, never its output.
pub fn item_json(item: &ChatItem) -> Value {
    match item {
        ChatItem::User { text } | ChatItem::Steered { text } => {
            json!({"kind": "user", "text": text})
        }
        ChatItem::Agent { text } => json!({"kind": "agent", "text": text}),
        ChatItem::Thought { text } => json!({"kind": "thought", "text": text}),
        ChatItem::Tool(t) => json!({
            "kind": "tool",
            "title": t.title,
            "tool_kind": t.kind,
            "status": status_name(t.status),
        }),
        ChatItem::Permission { request, answered } => json!({
            "kind": "permission",
            "request": request.id,
            "title": request.title,
            "options": options_json(&request.options),
            "answered": answered,
        }),
        ChatItem::Question { request, answered } => json!({
            "kind": "question",
            "request": request.id,
            "message": request.message,
            "fields": fields_json(&request.fields),
            "answered": answered,
        }),
        ChatItem::Notice { text, kind } => {
            let level = match kind {
                NoticeKind::Info => "info",
                NoticeKind::Error => "error",
            };
            json!({"kind": "notice", "text": text, "level": level})
        }
        ChatItem::Edits { files, .. } => {
            let files: Vec<_> = files.iter().map(|f| f.path.display().to_string()).collect();
            json!({"kind": "edits", "files": files})
        }
    }
}

/// A permission request's options: `[{id, name, kind}]`.
pub fn options_json(options: &[PermissionChoice]) -> Value {
    let options = options.iter().map(|o| {
        let kind = match o.kind {
            ChoiceKind::AllowOnce => "allow_once",
            ChoiceKind::AllowAlways => "allow_always",
            ChoiceKind::RejectOnce => "reject_once",
            ChoiceKind::RejectAlways => "reject_always",
        };
        json!({"id": o.id, "name": o.name, "kind": kind})
    });
    Value::Array(options.collect())
}

/// A question's fields: `[{key, title, description, required, kind, choices}]`; `choices` is
/// empty for a text, boolean or number field.
pub fn fields_json(fields: &[QuestionField]) -> Value {
    let fields = fields.iter().map(|f| {
        let (kind, choices) = match &f.kind {
            FieldKind::SingleChoice(c) => ("single", &c[..]),
            FieldKind::MultiChoice(c) => ("multi", &c[..]),
            FieldKind::Text => ("text", &[][..]),
            FieldKind::Boolean => ("boolean", &[][..]),
            FieldKind::Number => ("number", &[][..]),
        };
        let choices: Vec<_> = (choices.iter())
            .map(|c| json!({"value": c.value, "label": c.label, "description": c.description}))
            .collect();
        json!({
            "key": f.key,
            "title": f.title,
            "description": f.description,
            "required": f.required,
            "kind": kind,
            "choices": choices,
        })
    });
    Value::Array(fields.collect())
}

fn status_name(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Pending => "pending",
        ToolStatus::InProgress => "in_progress",
        ToolStatus::Completed => "completed",
        ToolStatus::Failed => "failed",
    }
}

/// `sessions/read`'s result, `{items, total}`: `limit` items (default 200, at most 500) from
/// `from`, else the last ones.
pub fn page(items: &[&ChatItem], from: Option<usize>, limit: Option<usize>) -> Value {
    let total = items.len();
    let limit = limit.unwrap_or(PAGE_DEFAULT).min(PAGE_MAX);
    let start = from.unwrap_or(total.saturating_sub(limit)).min(total);
    let end = (start + limit).min(total);
    let items: Vec<_> = items[start..end].iter().map(|i| item_json(i)).collect();
    json!({"items": items, "total": total})
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use wai_agent::{
        Choice, ChoiceKind, FieldKind, PermissionChoice, PermissionRequest, QuestionField,
        QuestionRequest, ToolStatus,
    };

    use super::{item_json, page};
    use crate::chat::{ChatItem, EditedFile, NoticeKind, ToolItem};

    fn tool() -> ToolItem {
        ToolItem {
            id: "t1".into(),
            title: "Edit main.rs".into(),
            kind: "edit".into(),
            status: ToolStatus::Completed,
            text: "secret output".into(),
            diffs: Vec::new(),
            locations: Vec::new(),
            first_sight: Vec::new(),
            expanded: false,
            subagent: None,
        }
    }

    fn permission() -> PermissionRequest {
        PermissionRequest {
            id: 7,
            session: "s".into(),
            tool_call_id: "t1".into(),
            title: "Run cargo test".into(),
            diff: None,
            options: vec![
                PermissionChoice {
                    id: "allow".into(),
                    name: "Allow".into(),
                    kind: ChoiceKind::AllowOnce,
                },
                PermissionChoice {
                    id: "reject".into(),
                    name: "Reject".into(),
                    kind: ChoiceKind::RejectAlways,
                },
            ],
            subagent: None,
        }
    }

    fn question() -> QuestionRequest {
        let choice = |v: &str| Choice {
            value: v.into(),
            label: v.to_uppercase(),
            description: None,
        };
        let field = |key: &str, required, kind| QuestionField {
            key: key.into(),
            title: key.to_uppercase(),
            description: None,
            required,
            kind,
        };
        QuestionRequest {
            id: 3,
            session: "s".into(),
            message: "Which branch?".into(),
            fields: vec![
                QuestionField {
                    description: Some("The base".into()),
                    ..field("b", true, FieldKind::SingleChoice(vec![choice("main")]))
                },
                field(
                    "tags",
                    false,
                    FieldKind::MultiChoice(vec![choice("x"), choice("y")]),
                ),
                field("note", false, FieldKind::Text),
                field("draft", false, FieldKind::Boolean),
                field("count", false, FieldKind::Number),
            ],
            subagent: None,
        }
    }

    #[test]
    fn item_json_covers_every_kind() {
        let text = |t: &str| t.to_string();
        let cases = [
            (
                ChatItem::User { text: text("hi") },
                json!({"kind": "user", "text": "hi"}),
            ),
            (
                ChatItem::Steered { text: text("also") },
                json!({"kind": "user", "text": "also"}),
            ),
            (
                ChatItem::Agent { text: text("ok") },
                json!({"kind": "agent", "text": "ok"}),
            ),
            (
                ChatItem::Thought { text: text("hm") },
                json!({"kind": "thought", "text": "hm"}),
            ),
            (
                ChatItem::Tool(tool()),
                json!({"kind": "tool", "title": "Edit main.rs", "tool_kind": "edit",
                    "status": "completed"}),
            ),
            (
                ChatItem::Permission {
                    request: permission(),
                    answered: None,
                },
                json!({"kind": "permission", "request": 7, "title": "Run cargo test",
                    "options": [
                        {"id": "allow", "name": "Allow", "kind": "allow_once"},
                        {"id": "reject", "name": "Reject", "kind": "reject_always"},
                    ],
                    "answered": null}),
            ),
            (
                ChatItem::Question {
                    request: question(),
                    answered: Some("Declined".into()),
                },
                json!({"kind": "question", "request": 3, "message": "Which branch?",
                    "fields": [
                        {"key": "b", "title": "B", "description": "The base", "required": true,
                            "kind": "single",
                            "choices": [{"value": "main", "label": "MAIN", "description": null}]},
                        {"key": "tags", "title": "TAGS", "description": null, "required": false,
                            "kind": "multi",
                            "choices": [{"value": "x", "label": "X", "description": null},
                                {"value": "y", "label": "Y", "description": null}]},
                        {"key": "note", "title": "NOTE", "description": null, "required": false,
                            "kind": "text", "choices": []},
                        {"key": "draft", "title": "DRAFT", "description": null,
                            "required": false, "kind": "boolean", "choices": []},
                        {"key": "count", "title": "COUNT", "description": null,
                            "required": false, "kind": "number", "choices": []},
                    ],
                    "answered": "Declined"}),
            ),
            (
                ChatItem::Notice {
                    text: text("Cancelled"),
                    kind: NoticeKind::Info,
                },
                json!({"kind": "notice", "text": "Cancelled", "level": "info"}),
            ),
            (
                ChatItem::Notice {
                    text: text("Agent exited"),
                    kind: NoticeKind::Error,
                },
                json!({"kind": "notice", "text": "Agent exited", "level": "error"}),
            ),
            (
                ChatItem::Edits {
                    turn: 0,
                    files: vec![EditedFile {
                        path: PathBuf::from("main.rs"),
                        steps: vec![0],
                        reverted: 0,
                        failed: None,
                    }],
                },
                json!({"kind": "edits", "files": ["main.rs"]}),
            ),
        ];
        for (item, expected) in cases {
            assert_eq!(item_json(&item), expected, "{item:?}");
        }
    }

    #[test]
    fn tool_item_has_no_output() {
        let mut running = tool();
        running.status = ToolStatus::InProgress;
        let json = item_json(&ChatItem::Tool(running));
        assert_eq!(
            json,
            json!({"kind": "tool", "title": "Edit main.rs", "tool_kind": "edit",
                "status": "in_progress"})
        );
        assert!(!json.to_string().contains("secret output"));
    }

    #[test]
    fn page_defaults_to_last_200() {
        let items: Vec<_> = (0..600)
            .map(|i| ChatItem::Agent {
                text: i.to_string(),
            })
            .collect();
        let items: Vec<_> = items.iter().collect();
        let texts = |page: &serde_json::Value| -> Vec<String> {
            let items = page["items"].as_array().unwrap();
            (items.iter())
                .map(|i| i["text"].as_str().unwrap().to_string())
                .collect()
        };
        let range =
            |r: std::ops::Range<usize>| -> Vec<String> { r.map(|i| i.to_string()).collect() };

        let last = page(&items, None, None);
        assert_eq!(last["total"], 600);
        assert_eq!(texts(&last), range(400..600));

        let from = page(&items, Some(10), Some(5));
        assert_eq!(texts(&from), range(10..15));
        assert_eq!(from["total"], 600);

        assert_eq!(
            texts(&page(&items, Some(0), Some(1000))),
            range(0..500),
            "at most 500"
        );
        assert_eq!(texts(&page(&items, None, Some(3))), range(597..600));
        assert_eq!(texts(&page(&items, Some(598), None)), range(598..600));
        assert!(texts(&page(&items, Some(700), None)).is_empty());

        let few = &items[..3];
        assert_eq!(texts(&page(few, None, None)), range(0..3));
    }
}
