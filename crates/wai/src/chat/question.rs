//! The question card: an agent's form (an ACP elicitation, e.g. Claude's AskUserQuestion), one
//! control per field, then Submit, Decline and Cancel.

use std::collections::HashMap;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use serde_json::{Map, Value};
use wai_agent::{Choice, FieldKind, QuestionAnswer, QuestionField, QuestionRequest};

use super::model::ChatItem;
use super::view::ChatSession;
use crate::aurora;

/// What the user entered so far in an open question's form, per field.
pub struct QuestionForm {
    fields: Vec<FieldState>,
    _subscriptions: Vec<Subscription>,
}

enum FieldState {
    Choice(Option<usize>),
    Choices(Vec<bool>),
    /// A text or a number field.
    Text(Entity<InputState>),
    Switch(bool),
}

impl QuestionForm {
    fn new(fields: &[QuestionField], window: &mut Window, cx: &mut Context<ChatSession>) -> Self {
        let mut subscriptions = Vec::new();
        let fields = fields
            .iter()
            .map(|field| match &field.kind {
                FieldKind::SingleChoice(_) => FieldState::Choice(None),
                FieldKind::MultiChoice(choices) => FieldState::Choices(vec![false; choices.len()]),
                FieldKind::Boolean => FieldState::Switch(false),
                FieldKind::Text | FieldKind::Number => {
                    let input = cx.new(|cx| match field.kind {
                        FieldKind::Number => InputState::new(window, cx).step(1.),
                        _ => InputState::new(window, cx),
                    });
                    // Submit follows what is typed.
                    subscriptions
                        .push(cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify()));
                    FieldState::Text(input)
                }
            })
            .collect();
        Self {
            fields,
            _subscriptions: subscriptions,
        }
    }

    /// Each filled field: its key, the value the agent gets and how the card shows it.
    fn filled(&self, fields: &[QuestionField], cx: &App) -> Vec<(String, Value, String)> {
        let filled = fields
            .iter()
            .zip(&self.fields)
            .filter_map(|(field, state)| {
                let (value, shown) = match (state, &field.kind) {
                    (FieldState::Choice(Some(i)), FieldKind::SingleChoice(choices)) => {
                        let choice = choices.get(*i)?;
                        (Value::from(choice.value.clone()), choice.label.clone())
                    }
                    (FieldState::Choices(picked), FieldKind::MultiChoice(choices)) => {
                        let chosen: Vec<_> = choices
                            .iter()
                            .zip(picked)
                            .filter_map(|(choice, picked)| picked.then_some(choice))
                            .collect();
                        if chosen.is_empty() {
                            return None;
                        }
                        let values: Vec<_> = chosen.iter().map(|c| c.value.clone()).collect();
                        let labels: Vec<_> = chosen.iter().map(|c| c.label.as_str()).collect();
                        (Value::from(values), labels.join(", "))
                    }
                    (FieldState::Switch(on), _) => {
                        (Value::Bool(*on), if *on { "Yes" } else { "No" }.into())
                    }
                    (FieldState::Text(input), kind) => {
                        let text = input.read(cx).value().trim().to_string();
                        if text.is_empty() {
                            return None;
                        }
                        match kind {
                            FieldKind::Number => (number(&text)?, text),
                            _ => (Value::from(text.clone()), text),
                        }
                    }
                    _ => return None,
                };
                Some((field.key.clone(), value, shown))
            });
        filled.collect()
    }
}

/// Whether every required field is filled.
fn ready(fields: &[QuestionField], filled: &[(String, Value, String)]) -> bool {
    fields
        .iter()
        .filter(|f| f.required)
        .all(|f| filled.iter().any(|(key, ..)| *key == f.key))
}

/// An integer when it is one, else a finite number; `None` for anything else.
fn number(text: &str) -> Option<Value> {
    match text.parse::<i64>() {
        Ok(n) => Some(n.into()),
        Err(_) => serde_json::Number::from_f64(text.parse().ok()?).map(Value::Number),
    }
}

/// The element id of a field's control, e.g. `field-color`; its options are ids 0, 1, …
fn field_id(field: &QuestionField) -> ElementId {
    ElementId::Name(format!("field-{}", field.key).into())
}

/// A control's listener that edits field `field` of row `ix`'s form.
fn edit<E: ?Sized + 'static>(
    ix: usize,
    field: usize,
    cx: &Context<ChatSession>,
    f: impl Fn(&mut FieldState, &E) + 'static,
) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    cx.listener(move |this: &mut ChatSession, event: &E, _, cx| {
        if let Some(state) = this
            .questions
            .get_mut(&ix)
            .and_then(|f| f.fields.get_mut(field))
        {
            f(state, event);
            cx.notify();
        }
    })
}

/// The message and, while open, the form and its buttons; once closed, how it ended.
pub fn card(
    ix: usize,
    request: &QuestionRequest,
    answered: Option<&str>,
    forms: &mut HashMap<usize, QuestionForm>,
    window: &mut Window,
    cx: &mut Context<ChatSession>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let card = super::permission::frame(aurora::lilas(), "QUESTION", request.message.clone(), cx)
        .id(("question", ix));
    if let Some(answered) = answered {
        return card
            .child(div().text_color(muted).child(answered.to_string()))
            .into_any_element();
    }
    let form = forms
        .entry(ix)
        .or_insert_with(|| QuestionForm::new(&request.fields, window, cx));
    let can_submit = ready(&request.fields, &form.filled(&request.fields, cx));
    let fields = request
        .fields
        .iter()
        .zip(&form.fields)
        .enumerate()
        .map(|(i, (field, state))| render_field(ix, i, field, state, muted, cx));

    let submit = cx.listener(move |this: &mut ChatSession, _, _, cx| {
        let (Some(ChatItem::Question { request, .. }), Some(form)) =
            (this.model.items.get(ix), this.questions.get(&ix))
        else {
            return;
        };
        let filled = form.filled(&request.fields, cx);
        if !ready(&request.fields, &filled) {
            return;
        }
        let shown: Vec<_> = filled.iter().map(|(_, _, shown)| shown.as_str()).collect();
        let summary = match shown.join(", ") {
            shown if shown.is_empty() => "Answered".to_string(),
            shown => format!("Answered: {shown}"),
        };
        let content: Map<_, _> = filled.into_iter().map(|(k, v, _)| (k, v)).collect();
        this.answer_question(ix, QuestionAnswer::Accept(content), summary, cx);
    });
    let close = |answer: QuestionAnswer, summary: &'static str| {
        cx.listener(move |this: &mut ChatSession, _, _, cx| {
            this.answer_question(ix, answer.clone(), summary.into(), cx)
        })
    };
    let buttons = h_flex()
        .gap_2()
        .child(
            Button::new("question-submit")
                .small()
                .primary()
                .label("Submit")
                .disabled(!can_submit)
                .on_click(submit),
        )
        .child(
            Button::new("question-decline")
                .small()
                .label("Decline")
                .on_click(close(QuestionAnswer::Decline, "Declined")),
        )
        .child(
            Button::new("question-cancel")
                .small()
                .ghost()
                .label("Cancel")
                .on_click(close(QuestionAnswer::Cancel, "Cancelled")),
        );
    card.children(fields).child(buttons).into_any_element()
}

/// An option's secondary text, under its label.
fn option_description(choice: &Choice, muted: Hsla) -> Option<Div> {
    let description = choice.description.clone()?;
    Some(div().text_sm().text_color(muted).child(description))
}

/// The field's title (`*` when required), description and control.
fn render_field(
    ix: usize,
    i: usize,
    field: &QuestionField,
    state: &FieldState,
    muted: Hsla,
    cx: &Context<ChatSession>,
) -> impl IntoElement + use<> {
    let title = match field.required {
        true => format!("{} *", field.title),
        false => field.title.clone(),
    };
    let control = match (state, &field.kind) {
        (FieldState::Choice(selected), FieldKind::SingleChoice(choices)) => {
            RadioGroup::vertical(field_id(field))
                .children(choices.iter().map(|c| {
                    Radio::new(c.value.clone())
                        .label(c.label.clone())
                        .children(option_description(c, muted))
                }))
                .selected_index(*selected)
                .on_click(edit(ix, i, cx, |state, choice: &usize| {
                    *state = FieldState::Choice(Some(*choice))
                }))
                .into_any_element()
        }
        (FieldState::Choices(picked), FieldKind::MultiChoice(choices)) => v_flex()
            .id(field_id(field))
            .gap_2()
            .children(choices.iter().zip(picked).enumerate().map(|(n, (c, on))| {
                Checkbox::new(n)
                    .label(c.label.clone())
                    .children(option_description(c, muted))
                    .checked(*on)
                    .on_click(edit(ix, i, cx, move |state, on: &bool| {
                        if let FieldState::Choices(picked) = state {
                            picked[n] = *on;
                        }
                    }))
            }))
            .into_any_element(),
        (FieldState::Switch(on), _) => Switch::new(field_id(field))
            .checked(*on)
            .on_click(edit(ix, i, cx, |state, on: &bool| {
                *state = FieldState::Switch(*on)
            }))
            .into_any_element(),
        (FieldState::Text(input), FieldKind::Number) => NumberInput::new(input).into_any_element(),
        (FieldState::Text(input), _) => Input::new(input).into_any_element(),
        // ponytail: a state always matches its field's kind (both come from `request.fields`).
        _ => div().into_any_element(),
    };
    v_flex()
        .gap_1()
        .child(div().font_weight(FontWeight::MEDIUM).child(title))
        .children(
            field
                .description
                .clone()
                .map(|d| div().text_sm().text_color(muted).child(d)),
        )
        .child(control)
}

impl ChatSession {
    /// Sends the answer to row `ix`'s question and closes its card with `summary`.
    fn answer_question(
        &mut self,
        ix: usize,
        answer: QuestionAnswer,
        summary: String,
        cx: &mut Context<Self>,
    ) {
        let Some(ChatItem::Question { request, .. }) = self.model.items.get(ix) else {
            return;
        };
        let id = request.id;
        let sent = self.handle.answer_question(id, answer);
        self.update_model(cx, |m| match sent {
            Ok(()) => m.on_question_answered(id, &summary),
            // Already resolved on the agent's side (cancelled or withdrawn).
            Err(_) => m.on_question_resolved(id),
        });
        self.questions.remove(&ix);
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn numbers_keep_integers() {
        assert_eq!(number("3"), Some(3.into()));
        assert_eq!(number("2.5"), Some(2.5.into()));
        assert_eq!(number("inf"), None);
        assert_eq!(number("x"), None);
    }
}
