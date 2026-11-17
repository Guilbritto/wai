//! Per-session context size: a 1M Claude session loads and forks with this `_meta`.

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
