# Per-session context size (Claude)

## Intent

Long Claude sessions in wai lose decisions when Claude Code auto-compacts. Each compaction
summarizes the previous summary, so decisions erode. Example: the wai-waycarbon redesign session
(`0c5456b3`, 2026-11-17) compacted 9 times in about 3 hours, each time at about 167k tokens.

The 1M window works on this account: session `21515b03` (2026-11-12, in wai) reached 438k tokens
in a single request. The owner then turned 1M off everywhere (`CLAUDE_CODE_DISABLE_1M_CONTEXT=1`
in the `env` block of `~/.claude/settings.json`) because token use was too high.

What we want: 200k stays the default. The owner turns 1M on only for the sessions that need it,
including mid-session, when a session turns out to be long.

## Goals

- A Claude session's header gets a **Context** picker: `200k` (default) or `1M`.
- Switching mid-session keeps the conversation: the session reloads in place under the same
  session id.
- The choice belongs to the session id. It survives an app restart, reopening from history, and
  dragging a history row into a pane. A fork inherits it.
- The terminal's `claude` is unaffected and stays at 200k.

## Non-goals

- Other agents (Codex, Antigravity). The picker shows only for agent id `claude`.
- Sizes other than 200k and 1M, such as a 400k cap via `CLAUDE_CODE_AUTO_COMPACT_WINDOW`. Later,
  with the same mechanism.
- Choosing the context in the draft, before the first message. *(Builder's call: switching right
  after the start covers it. Add later if asked.)*
- A settings-level default, such as "every session in project X uses 1M".
- Keeping decisions across compaction some other way (pinned notes, compaction instructions).

## Anti-goals

- A 200k session silently running at 1M, which is what the global flag exists to prevent. Any
  failure falls back to 200k, never to 1M.
- Losing or duplicating the conversation when switching.
- Changing what the terminal `claude` does.

## Constraints (as the code stands)

**Adapter** (`@agentclientprotocol/claude-agent-acp@0.103.0`, pinned at
`crates/wai-settings/src/lib.rs:26`; paths under its `dist/`):

- It accepts `_meta.claudeCode.options` on session/new, session/load and session/fork
  (`v1/acp-agent.js:7511`).
- `options.env` is spread over `process.env` for that session's Claude Code process (`:7583`).
- `options.settings` becomes that process's programmatic (flag) settings (`:7570`).
- On session/load, a change to any of these options rebuilds the session's Claude Code process
  (`OPTION_REBUILDS_SESSION` at `:306`, fingerprint at `:410`).
- Claude Code applies the `env` block of `settings.json` after the process env (comment at
  `idle-compaction.js:192`). So `options.env` alone cannot undo the global flag.

**Model list:**

- With 1M off, the model list has no `[1m]` rows. wai has seen 14 Claude models, none with `[1m]`.
- With 1M allowed, Claude Code lists `[1m]` rows, and the adapter knows how to match them
  (`session-model.js`, `resolveModelPreference`).

**Idle compaction:** the adapter compacts a session about 54 minutes after its last response, so
that the next prompt does not rewrite the expired 1-hour cache.

- It only does this when the context is at least 200k tokens (`idle-compaction.js`,
  `DEFAULT_MIN_TOKENS`), so it never touches a 200k session. A 1M session left idle for an hour
  will be compacted.
- It is off when `settings.idleCompaction === false`, when `autoCompactEnabled === false`, or
  when `DISABLE_COMPACT` / `DISABLE_AUTO_COMPACT` is set.

**wai:**

- No `_meta` is sent today (`crates/wai-agent/src/actor.rs:670`, `:682`, `:695`).
  `agent-client-protocol` 5.0.0 has a `.meta(...)` builder on `NewSessionRequest`,
  `LoadSessionRequest` and `ForkSessionRequest`.
- Pickers:
  - Built from the agent's config options in `sync_pickers` (`crates/wai/src/chat/view.rs:449`).
  - Drawn as `bar-{id}` chips (`chat/input.rs:726-797`).
  - A pick runs `set_option` (`view.rs:499`), which puts the old value back on error.
  - The only picker wai owns is the legacy mode picker, so a Context picker owned by wai is new.
- Reload: `ChatSession::load` (`view.rs:1089`). The Restart button (`render.rs:606`) already
  re-runs it on a live session: it clears the items and replays.
  - **Caveat:** `take_started` (`view.rs:433-446`) only takes config options when the session has
    none, so a reload keeps the stale model list unless that changes.
- `load` call sites:
  - app-restart restore: `workspace.rs:894`
  - opening a history row: `workspace.rs:1546`
  - dragging a history row into a pane: `workspace.rs:1625`
  - fork: `fork_session` at `workspace.rs:1714`, then `load` at `:1723`
- Persistence: `AppState` (`crates/wai-settings/src/lib.rs:496`) is `#[serde(default)]`, and a
  parse error resets the whole state, so new fields must be lenient. `hidden: BTreeSet<String>`
  is the existing pattern for a set of session ids.
- Claude-only behavior is keyed on the agent id `"claude"`. Examples: `input.rs:702`,
  `sidebar.rs:205`, `sidebar.rs:1540`.

## Approaches

1. **A Context picker owned by wai, sent per session in `_meta`** (chosen).
   - wai remembers the choice per session id and re-sends it on every load and fork.
   - A switch reloads the session in place.
   - The global flag stays, so the terminal is unaffected.
   - Cost: code in three layers (agent request, persisted state, UI), and Claude only.
2. **Two agents in `settings.toml` (`claude` and `claude-1m`), no code.**
   - The global flag has to leave `settings.json`, because its `env` beats the command's env. The
     terminal then goes back to 1M unless it is guarded some other way.
   - No mid-session switch.
3. **Stay at 200k and pin decisions in the system prompt.**
   - No extra tokens.
   - Someone has to keep the list, and whatever never became a "decision" is still lost.

The owner picked **1**: 1M is paid for only when a session proves long, and it can be switched on
mid-session.

## Design

### Step 0: spike (decides the mechanism)

For a 1M session, wai sends `_meta.claudeCode.options` on new, load and fork. These options turn
1M back on for that session's Claude Code process.

**Mechanism A (preferred):**

```json
{"claudeCode": {"options": {"settings": {"env": {"CLAUDE_CODE_DISABLE_1M_CONTEXT": "0"}}}}}
```

The flag-settings tier outranks user settings, and the global flag stays in
`~/.claude/settings.json`. A 200k session sends no `_meta`, because the global flag already
applies.

The spike runs the pinned adapter by hand over stdio, or through a throwaway wai test. It confirms:

1. A session/new with this meta lists `[1m]` model rows.
2. After picking the `[1m]` row, the session's `usage_update` reports a size of about 1,000,000.
3. A session/new without the meta still lists no `[1m]` rows, with the global flag in place.
4. A session/load of an existing session with the meta rebuilds it with 1M and keeps its history.
5. Downsizing (a load without the meta) on a session whose context is already above 200k
   compacts on the next prompt instead of failing. *(Builder's call if it fails: refuse the 200k
   pick while the context is above the 200k compaction threshold, and say "run /compact first".
   The session stays at 1M.)*
6. A session/load with the meta right after session/new, before any prompt, works with nothing
   on disk yet. *(Builder's call if it fails: keep the chip disabled until the first turn ends.)*

**Mechanism B (fallback, only if A fails):**

- The flag moves out of `settings.json` into the shell env (`export` in `~/.zshrc`), so the
  terminal stays at 200k.
- wai always sends `options.env.CLAUDE_CODE_DISABLE_1M_CONTEXT`: `"1"` for 200k and `"0"` for 1M.
- This needs the owner to edit their own files, so the builder **stops and asks** before taking
  B.

If the spike shows that putting `model` in the same settings object is simpler than picking the
row afterwards, that is the builder's call.

### Agent layer (`wai-agent`)

- `new_session`, `load_session` and `fork_session` take an optional `serde_json::Map` meta. The
  matching `Command` variants carry it, and it is forwarded with `.meta(...)`.
- `wai-agent` stays agent-agnostic: the Claude-specific JSON is built in `wai`, by a single
  function.

### Switching

- Picking `1M` stores the session id in the set (see Persistence), then runs the existing reload
  (`ChatSession::load`, as Restart does) with the meta.
- After the reload, wai picks the `[1m]` row of the current model's family through the existing
  `set_option` (for example `opus` → `opus[1m]`).
- If no such row exists, wai removes the id from the set, reloads at 200k, and shows the error the
  same way a failed config pick does.
- Picking `200k` removes the id, reloads without the meta, and picks the plain row of the same
  family.
- The chip is disabled while a turn is running, and shows a busy state while reloading.
  *(Builder's call: switching mid-turn would cancel the turn.)* So no queued prompt or pending
  permission exists during a switch. The composer's text and attachments must survive the reload.
- A model with no `[1m]` row cannot be known in advance, because 200k sessions list no `[1m]`
  rows. The chip stays enabled, and the error-and-revert above covers it. *(Builder's call.)*
- The reload has to refresh the config options and pickers. That means fixing the
  `take_started` caveat for this path.

### Persistence

- `AppState.long_context: BTreeSet<String>` holds the session ids that run at 1M. It is
  `#[serde(default)]`, like `hidden`.
- Every `load` (restore, history, drag) reads the set to build the meta.
- A fork inserts the new id when the source id is in the set, and passes the meta to
  `fork_session` and to the `load` that follows.
- Any id not in the set is 200k.
- When a restore or history load of an id in the set fails, the id stays in the set. The
  existing load error and Restart show, and Restart retries with the meta. wai never drops the
  choice on its own. *(Builder's call.)*

### UI

- A `bar-context` chip next to the model and effort chips, for agent id `claude` only.
- Values: `200k` and `1M`. It follows the existing chip visuals.
- The usage text already shows the size the agent reports (`agents_view.rs:77`), so `/ 1000k`
  appears on its own.

### Idle compaction on 1M sessions

**Decided (owner): off for 1M sessions.** Otherwise the first long pause compacts exactly the
sessions this feature protects. The accepted cost: the first prompt after a pause rewrites the
full cache.

- The adapter's `SettingsManager` reads only settings files (`settings.js`), so an
  `idleCompaction: false` inside the meta's `settings` would be ignored.
- `DISABLE_COMPACT` and `DISABLE_AUTO_COMPACT` would also stop Claude Code's own compaction near
  the 1M limit, so they are out.
- The lever is the session env: `options.env.CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS = "1000000"`.
  `minTokens()` (`idle-compaction.js:198`) then sits above anything the context reaches, because
  Claude Code compacts before 1M.
- A 200k session never reaches the default 200k minimum, so it needs nothing.

So a 1M session's meta is:

```json
{"claudeCode": {"options": {
  "settings": {"env": {"CLAUDE_CODE_DISABLE_1M_CONTEXT": "0"}},
  "env": {"CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS": "1000000"}
}}}
```

Spike item 7: with this meta, the adapter's log shows the idle compaction skipped
(`context … is below the 1000000 minimum`). `CLAUDE_AGENT_ACP_IDLE_COMPACT_DELAY_MS` in the same
`env` shortens the wait for the test.

### Tests

- The fake agent gets `--echo-meta`: it stores each request's `_meta` per session on new, load
  and fork, and a turn replies `meta:<json>`. Copy the `--echo-mcp` pattern (`record_mcp`,
  `wai-fake-agent.rs:257`).
- Cases:
  - Switching to 1M reloads with the meta and keeps the session id.
  - Restore and history reopen send the meta for ids in the set.
  - A fork inherits it.
  - A 200k session sends no meta.
  - A non-`claude` agent shows no chip.
- Closest existing tests to copy: `ext_host.rs:832` `mcp_servers_passed_to_new_session` (it
  covers a load after start) and `start.rs:599` `model_pick_applies_to_the_new_session`.
- To reach the Claude-only path, test settings can register the fake agent under id `claude`.

## Left to the builder

- The exact meta JSON and the model-row matching, both decided by the spike.
- Names, chip visuals (follow the existing chips), and how the busy state looks.

## Spike results

Measured on 2026-11-17 with `claude-agent-acp@0.103.0`, model `opus` (`claude-opus-5-5`), and the
owner's `~/.claude/settings.json` (`CLAUDE_CODE_DISABLE_1M_CONTEXT=1`). Regression check:
`one_million_context_via_meta` in `crates/wai-agent/tests/real_claude.rs`.

- Mechanism A holds with the meta as designed:
  `{"claudeCode":{"options":{"settings":{"env":{"CLAUDE_CODE_DISABLE_1M_CONTEXT":"0"}},"env":{"CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS":"1000000"}}}}`.
- No model row ends in `[1m]`, with or without the meta: `opus` is natively 1M, and the flag only
  caps it. Switching size is a reload with or without the meta; no model row changes.
- CLI `getContextUsage()`: flag on → `rawMaxTokens` 200000, `autoCompactThreshold` 167000;
  meta → 1000000 and 967000. The turn result's `Usage.size` follows (200000 / 1000000).
- A load with the meta right after `session/new` works, before any prompt. A load without it goes
  back to 200k and keeps the history. A fork with the meta is 1M.
- `Usage.size` mid-turn can carry the adapter's cached window for the model; only the turn
  result's (last) value is authoritative.
