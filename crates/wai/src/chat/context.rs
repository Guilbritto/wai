//! Per-session context size: a 1M Claude session loads and forks with this `_meta`.

use gpui_kit::component::IndexPath;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::*;

use super::model::TurnState;
use super::view::{ChatEvent, ChatSession};

/// The `_meta` that lifts the user's `CLAUDE_CODE_DISABLE_1M_CONTEXT` for one session (flag
/// settings win over the user's) and keeps idle compaction off until 1M. `None` for 200k.
pub(crate) fn context_meta(long: bool) -> Option<wai_agent::Meta> {
    let meta = serde_json::json!({"claudeCode": {"options": {
        "settings": {"env": {"CLAUDE_CODE_DISABLE_1M_CONTEXT": "0"}},
        "env": {"CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS": "1000000"}
    }}});
    match meta {
        serde_json::Value::Object(map) if long => Some(map),
        _ => None,
    }
}

/// The Context chip's values, 200k first: `SIZES[long as usize]`.
const SIZES: [&str; 2] = ["200k", "1M"];

impl ChatSession {
    pub(super) fn new_context_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Vec<&'static str>>> {
        let selected = Some(IndexPath::new(self.long_context as usize));
        let select = cx.new(|cx| SelectState::new(SIZES.to_vec(), selected, window, cx));
        let confirm = cx.subscribe_in(
            &select,
            window,
            |this, _, event: &SelectEvent<Vec<&'static str>>, window, cx| {
                if let SelectEvent::Confirm(Some(size)) = event {
                    this.set_long_context(*size == SIZES[1], window, cx);
                }
            },
        );
        self._subscriptions.push(confirm);
        select
    }

    /// Runs this session at 1M (`long`) or 200k: the agent loads it again, history and all, with
    /// or without `context_meta`. Not while a turn or a load runs.
    pub(crate) fn set_long_context(
        &mut self,
        long: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if long == self.long_context || self.context_locked() {
            return self.sync_context_picker(window, cx);
        }
        self.long_context = long;
        self.retake = true;
        cx.emit(ChatEvent::ContextChanged);
        self.load(false, window, cx);
    }

    /// A turn or a load is running: the size stays.
    pub(crate) fn context_locked(&self) -> bool {
        matches!(self.model.state, TurnState::Working | TurnState::NeedsYou)
    }

    pub(super) fn sync_context_picker(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = &self.context_picker {
            let size = SIZES[self.long_context as usize];
            picker.update(cx, |p, cx| p.set_selected_value(&size, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::context_meta;

    #[test]
    fn only_one_million_carries_a_meta() {
        assert_eq!(context_meta(false), None);
        let meta = serde_json::Value::Object(context_meta(true).unwrap());
        let expected = serde_json::json!({"claudeCode": {"options": {
            "settings": {"env": {"CLAUDE_CODE_DISABLE_1M_CONTEXT": "0"}},
            "env": {"CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS": "1000000"}
        }}});
        assert_eq!(meta, expected);
    }
}
