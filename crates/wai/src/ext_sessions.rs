//! Session content for extensions (API 1.10): the transcript as JSON, pages of it for
//! `sessions/read`. Pure functions; `workspace.rs` routes the calls and events to them.

use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use wai_agent::{
    Choice, ChoiceKind, FieldKind, PermissionChoice, PermissionRequest, QuestionAnswer,
    QuestionField, QuestionRequest, ToolStatus,
};
use wai_ext::HostEvent;

use crate::chat::{ChatEvent, ChatItem, ChatModel, ChatSession, NoticeKind};

/// `sessions/read` without `limit`: the last 200 items.
pub const PAGE_DEFAULT: usize = 200;
/// The most items one `sessions/read` returns: a page stays under the 4 MB message cap.
pub const PAGE_MAX: usize = 500;

/// The shortest gap between two `event/session_update`s of one session: 10 a second (HOST-02).
pub const FLUSH_GAP: Duration = Duration::from_millis(100);

/// A session's transcript as extensions see it: the closed rows, then the cards waiting for an
/// answer, as wai shows them.
pub fn transcript(model: &ChatModel) -> Vec<&ChatItem> {
    model.items.iter().chain(&model.waiting).collect()
}

/// The waiting permission request `id`.
pub fn waiting_permission(model: &ChatModel, id: u64) -> Option<&PermissionRequest> {
    model.waiting.iter().find_map(|item| match item {
        ChatItem::Permission { request, .. } if request.id == id => Some(request),
        _ => None,
    })
}

/// The host event a session's card event becomes (HOST-04, HOST-06, QST-01, QST-03).
pub fn card_event(event: &ChatEvent, s: &ChatSession) -> Option<HostEvent> {
    let (agent, session) = (s.agent.clone(), s.id.clone());
    Some(match event {
        ChatEvent::PermissionAsked(id) => {
            let request = waiting_permission(&s.model, *id)?;
            HostEvent::PermissionRequested {
                agent,
                session,
                request: *id,
                title: request.title.clone(),
                options: options_json(&request.options),
            }
        }
        ChatEvent::PermissionAnswered { id, option } => HostEvent::PermissionResolved {
            agent,
            session,
            request: *id,
            option: option.clone(),
        },
        ChatEvent::QuestionAsked(id) => {
            let request = s.model.waiting_question(*id)?;
            HostEvent::QuestionRequested {
                agent,
                session,
                request: *id,
                message: request.message.clone(),
                fields: fields_json(&request.fields),
            }
        }
        ChatEvent::QuestionAnswered { id, answer } => HostEvent::QuestionResolved {
            agent,
            session,
            request: *id,
            answer: answer.to_string(),
        },
        _ => return None,
    })
}

/// `event/session_update` for the items `changed` of the session's transcript.
pub fn session_update(s: &ChatSession, changed: Vec<usize>) -> HostEvent {
    let items = transcript(&s.model);
    let changed = (changed.into_iter())
        .filter_map(|ix| Some((ix, item_json(items.get(ix)?))))
        .collect();
    HostEvent::SessionUpdate {
        agent: s.agent.clone(),
        session: s.id.clone(),
        total: items.len(),
        items: changed,
    }
}

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

/// Whether `values`, an `accept` from an extension, answers `request` (QST-05): every required
/// field has a value, and each value fits its field's kind and choices. The error names the
/// field: `Field <key>: <why>`.
pub fn check_answer(request: &QuestionRequest, values: &Map<String, Value>) -> Result<(), String> {
    let error = |key: &str, why: &str| Err(format!("Field {key}: {why}"));
    if let Some(key) = (values.keys()).find(|k| !request.fields.iter().any(|f| f.key == **k)) {
        return error(key, "not in the question");
    }
    for field in &request.fields {
        let key = &field.key;
        let value = match values.get(key) {
            // As wai's form: blank text and no choices are no value.
            Some(Value::String(s)) if s.trim().is_empty() => None,
            Some(Value::Array(a)) if a.is_empty() => None,
            value => value,
        };
        let Some(value) = value else {
            match field.required {
                true => return error(key, "a value is required"),
                false => continue,
            }
        };
        let one_of = |choices: &[Choice], v: &Value| {
            v.as_str()
                .is_some_and(|v| choices.iter().any(|c| c.value == v))
        };
        let fits = match &field.kind {
            FieldKind::SingleChoice(choices) => one_of(choices, value),
            FieldKind::MultiChoice(choices) => {
                (value.as_array()).is_some_and(|a| a.iter().all(|v| one_of(choices, v)))
            }
            FieldKind::Text => value.is_string(),
            FieldKind::Boolean => value.is_boolean(),
            FieldKind::Number => value.is_number(),
        };
        if !fits {
            let why = match &field.kind {
                FieldKind::SingleChoice(_) => "not one of its choices",
                FieldKind::MultiChoice(_) => "not a list of its choices",
                FieldKind::Text => "not text",
                FieldKind::Boolean => "not true or false",
                FieldKind::Number => "not a number",
            };
            return error(key, why);
        }
    }
    Ok(())
}

/// An extension's `question/answer` as wai's own: the answer for the agent and the line its card
/// closes with. An `accept` must pass `check_answer`.
pub fn question_answer(
    request: &QuestionRequest,
    answer: &str,
    values: Option<Map<String, Value>>,
) -> Result<(QuestionAnswer, String), String> {
    match answer {
        "accept" => {
            let values = values.unwrap_or_default();
            check_answer(request, &values)?;
            let summary = summary(request, &values);
            Ok((QuestionAnswer::Accept(values), summary))
        }
        "decline" => Ok((QuestionAnswer::Decline, "Declined".into())),
        "cancel" => Ok((QuestionAnswer::Cancel, "Cancelled".into())),
        other => Err(format!(
            "Unknown answer {other}: use accept, decline or cancel"
        )),
    }
}

/// What wai's card shows for an accepted answer: `Answered: …`, each value in field order,
/// choices by their label, a switch as Yes or No.
fn summary(request: &QuestionRequest, values: &Map<String, Value>) -> String {
    let shown = request.fields.iter().filter_map(|field| {
        let label = |v: &Value| match &field.kind {
            FieldKind::SingleChoice(choices) | FieldKind::MultiChoice(choices) => (choices.iter())
                .find(|c| v.as_str() == Some(c.value.as_str()))
                .map(|c| c.label.clone()),
            _ => None,
        };
        let shown = match values.get(&field.key)? {
            Value::Array(values) => values
                .iter()
                .filter_map(label)
                .collect::<Vec<_>>()
                .join(", "),
            Value::Bool(on) => if *on { "Yes" } else { "No" }.into(),
            Value::String(text) => {
                label(&Value::from(text.as_str())).unwrap_or_else(|| text.trim().to_string())
            }
            value => value.to_string(),
        };
        (!shown.is_empty()).then_some(shown)
    });
    match shown.collect::<Vec<_>>().join(", ") {
        shown if shown.is_empty() => "Answered".into(),
        shown => format!("Answered: {shown}"),
    }
}

/// The transcript items that changed per session, sent as one `event/session_update` at most
/// every `FLUSH_GAP`. The caller runs the timer: `mark` says when to `flush`.
pub struct Feed<K> {
    sessions: HashMap<K, Pending>,
}

#[derive(Default)]
struct Pending {
    dirty: BTreeSet<usize>,
    /// A flush is scheduled.
    scheduled: bool,
    last: Option<Instant>,
}

impl<K> Default for Feed<K> {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }
}

impl<K: Hash + Eq> Feed<K> {
    /// Item `ix` of `key` changed at `now`. `Some(wait)`: schedule a `flush` after `wait`, at
    /// least `FLUSH_GAP` after the last one; `None`: one is already scheduled.
    pub fn mark(&mut self, key: K, ix: usize, now: Instant) -> Option<Duration> {
        let pending = self.sessions.entry(key).or_default();
        pending.dirty.insert(ix);
        if pending.scheduled {
            return None;
        }
        pending.scheduled = true;
        let next = pending.last.map_or(now, |last| last + FLUSH_GAP);
        Some(next.saturating_duration_since(now))
    }

    /// The changed items of `key`, in order, each once; the next flush waits `FLUSH_GAP`.
    pub fn flush(&mut self, key: &K, now: Instant) -> Vec<usize> {
        let Some(pending) = self.sessions.get_mut(key) else {
            return Vec::new();
        };
        pending.scheduled = false;
        pending.last = Some(now);
        std::mem::take(&mut pending.dirty).into_iter().collect()
    }

    /// The session closed.
    pub fn forget(&mut self, key: &K) {
        self.sessions.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use serde_json::json;
    use wai_agent::{
        Choice, ChoiceKind, FieldKind, PermissionChoice, PermissionRequest, QuestionField,
        QuestionRequest, ToolStatus,
    };

    use super::{FLUSH_GAP, Feed, check_answer, item_json, page};
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

    fn values(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn check_answer_needs_required_fields() {
        let q = question();
        let missing = "Field b: a value is required";
        assert_eq!(check_answer(&q, &values(json!({}))), Err(missing.into()));
        assert_eq!(
            check_answer(&q, &values(json!({"note": "hi"}))),
            Err(missing.into())
        );
        assert_eq!(
            check_answer(&q, &values(json!({"b": "  "}))),
            Err(missing.into()),
            "blank is no value"
        );
        assert_eq!(check_answer(&q, &values(json!({"b": "main"}))), Ok(()));
        let all =
            json!({"b": "main", "tags": ["x", "y"], "note": "n", "draft": true, "count": 2.5});
        assert_eq!(check_answer(&q, &values(all)), Ok(()));
        // Optional fields may stay empty, as in wai's form.
        let empty = json!({"b": "main", "tags": [], "note": ""});
        assert_eq!(check_answer(&q, &values(empty)), Ok(()));
    }

    #[test]
    fn check_answer_rejects_values_outside_kind() {
        let q = question();
        let check = |v: serde_json::Value| check_answer(&q, &values(v));
        let cases = [
            (json!({"b": "dev"}), "Field b: not one of its choices"),
            (json!({"b": 1}), "Field b: not one of its choices"),
            (json!({"b": ["main"]}), "Field b: not one of its choices"),
            (
                json!({"b": "main", "tags": ["x", "z"]}),
                "Field tags: not a list of its choices",
            ),
            (
                json!({"b": "main", "tags": "x"}),
                "Field tags: not a list of its choices",
            ),
            (json!({"b": "main", "note": 3}), "Field note: not text"),
            (
                json!({"b": "main", "draft": "yes"}),
                "Field draft: not true or false",
            ),
            (
                json!({"b": "main", "count": "3"}),
                "Field count: not a number",
            ),
            (
                json!({"b": "main", "count": null}),
                "Field count: not a number",
            ),
            (
                json!({"b": "main", "color": "red"}),
                "Field color: not in the question",
            ),
        ];
        for (v, error) in cases {
            assert_eq!(check(v.clone()), Err(error.to_string()), "{v}");
        }
    }

    #[test]
    fn feed_flushes_at_most_every_100ms() {
        let (mut feed, t0) = (Feed::default(), Instant::now());
        let ms = Duration::from_millis;
        assert_eq!(FLUSH_GAP, ms(100));
        // The first change flushes at once.
        assert_eq!(feed.mark("a", 0, t0), Some(ms(0)));
        assert_eq!(feed.flush(&"a", t0), [0]);
        // The next waits out the 100 ms since that flush; changes meanwhile join it.
        assert_eq!(feed.mark("a", 1, t0 + ms(30)), Some(ms(70)));
        assert_eq!(feed.mark("a", 2, t0 + ms(60)), None);
        assert_eq!(feed.flush(&"a", t0 + ms(100)), [1, 2]);
        // Another session has its own pace.
        assert_eq!(feed.mark("b", 0, t0 + ms(110)), Some(ms(0)));
        // A change long after the last flush goes at once again.
        assert_eq!(feed.mark("a", 3, t0 + ms(350)), Some(ms(0)));
        assert_eq!(feed.flush(&"a", t0 + ms(350)), [3]);
        assert!(feed.flush(&"a", t0 + ms(450)).is_empty(), "nothing changed");
        feed.forget(&"b");
        assert!(feed.flush(&"b", t0 + ms(500)).is_empty());
    }

    #[test]
    fn feed_merges_repeated_indexes() {
        let (mut feed, t0) = (Feed::default(), Instant::now());
        for ix in [5, 5, 6, 2, 5] {
            feed.mark("a", ix, t0);
        }
        assert_eq!(feed.flush(&"a", t0), [2, 5, 6]);
    }
}
