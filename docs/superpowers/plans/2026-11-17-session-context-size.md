# Per-session Context Size Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Claude session in wai can run at 200k (default) or 1M, picked per session from a
`bar-context` chip, kept across restarts, history reopen, drag, and fork.

**Architecture:** wai remembers the 1M session ids in `AppState.long_context`. Every
`session/load` and `session/fork` of such an id carries `_meta.claudeCode.options`, which turns 1M
back on for that session's Claude Code process and turns off its idle compaction. A switch reloads
the session in place, then picks the `[1m]` (or plain) row of the current model's family.

**Tech Stack:** Rust, GPUI / gpui-component, agent-client-protocol 5.0.0,
`@agentclientprotocol/claude-agent-acp@0.103.0`.

**Spec:** `docs/superpowers/specs/2026-11-17-session-context-size-design.md`

## Global Constraints

- Default is 200k. Any failure falls back to 200k, never to 1M.
- A 200k session sends no `_meta` at all.
- The 1M meta is exactly (Task 2 may change it; then it changes in one place, `context_meta`):
  ```json
  {"claudeCode": {"options": {
    "settings": {"env": {"CLAUDE_CODE_DISABLE_1M_CONTEXT": "0"}},
    "env": {"CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS": "1000000"}
  }}}
  ```
- `wai-agent` stays agent-agnostic. The Claude JSON is built only in `wai` by `context_meta`.
- The chip shows only for agent id `"claude"`. Values `200k` and `1M`.
- `~/.claude/settings.json` is not touched. Mechanism B (moving the flag to `~/.zshrc`) needs
  the owner's approval: stop and ask.
- New `AppState` fields are lenient (`#[serde(default)]`); a parse error resets the whole state.
- Commits: `feat:`/`fix:`/`test:`/`docs:`, no scope, no `Co-Authored-By`.
- Never quit or kill the running wai.

**Deviation from the spec, on purpose:** `new_session` gets no meta. A new session is always
200k (draft-time picking is a non-goal), so only `load_session` and `fork_session` change.

## Review Focus

1. **A load of a 1M session fails** (agent down, session gone). Expect: the id stays in the
   set, the pane shows the load error, Restart retries with the meta (Restart re-runs `load`,
   which reads `long_context`, so no code). Test in Task 3.
2. **The current model has no `[1m]` row** (e.g. `haiku`, or `default`). Expect: back to 200k,
   an error notice, the chip on `200k`. Test in Task 4.
3. **Composer text typed before a switch.** Expect: still in the composer after the reload. Test
   in Task 4.
4. **Picking the value the chip already shows.** Expect: nothing happens (no reload). Test in
   Task 4.
5. **Downsizing a session whose context is above 200k.** Expect: the next prompt compacts and
   works. Checked by hand in Task 5, because reaching 200k costs a real 200k-token request.

---

### Task 1: Agent layer carries `_meta` on load and fork

**Files:**
- Modify: `crates/wai-agent/src/actor.rs` (`Command::LoadSession`/`ForkSession` at `:23-45`,
  public methods at `:116-155`, request building at `:667-698`)
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (flags `:147-232`, `record_mcp` `:257`,
  `config_options` `:270`, reply `:407-416`, LoadSession `~:933`, ForkSession `~:966`)
- Modify callers: `crates/wai/src/chat/view.rs:1089`, `crates/wai/src/workspace.rs:1714`,
  `crates/wai-agent/tests/fake_agent.rs` (load/fork calls), `crates/wai-agent/tests/real_claude.rs:136,165,171`
- Test: `crates/wai-agent/tests/fake_agent.rs`

**Interfaces:**
- Produces:
  - `pub type Meta = serde_json::Map<String, serde_json::Value>;` in `wai-agent` (re-exported
    from `types.rs`).
  - `AgentHandle::load_session(&self, id: SessionId, cwd: PathBuf, mcp: Vec<McpServer>, meta: Option<Meta>) -> Result<SessionStarted, AgentError>`
  - `AgentHandle::fork_session(&self, id: SessionId, cwd: PathBuf, mcp: Vec<McpServer>, meta: Option<Meta>) -> Result<SessionStarted, AgentError>`
  - Fake agent flag `--echo-meta`: a turn replies `meta:<compact JSON of the session's last
    load/fork meta>` (`meta:` when none). It takes precedence over `--models` and `--echo-mcp`
    in the reply.
  - Fake agent with `--models`: a session whose last load/fork carried a non-empty meta also
    lists model rows `fake-small[1m]` ("Fake Small 1M") and `fake-large[1m]` ("Fake Large 1M").
    A load without meta clears it.

- [ ] **Step 1: Write the failing tests** in `fake_agent.rs`, next to the existing
  `--echo-mcp` load/fork test (`:640`):

```rust
#[test]
fn meta_reaches_load_and_fork() {
    // spawn with "--echo-meta"; meta = {"k": "v"}
    // new_session -> turn replies "meta:"
    // load_session("abc", cwd(), vec![], Some(meta)) -> a turn on "abc" replies r#"meta:{"k":"v"}"#
    // load_session("abc", cwd(), vec![], None)       -> a turn replies "meta:"
    // fork_session("abc", cwd(), vec![], Some(meta)) -> a turn on the fork replies r#"meta:{"k":"v"}"#
}

#[test]
fn meta_lists_the_one_million_rows() {
    // spawn with "--models"
    // load_session(.., Some(meta)): the "model" option's value ids are
    //   ["fake-small", "fake-large", "fake-small[1m]", "fake-large[1m]"]
    // load_session(.., None): ["fake-small", "fake-large"]
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p wai-agent --test fake_agent meta_`
Expected: compile error (`load_session` takes 3 arguments).

- [ ] **Step 3: Implement**
  - `actor.rs`: add `meta: Option<Meta>` to the two `Command` variants and methods, and forward
    with `.meta(meta)` on `LoadSessionRequest` / `ForkSessionRequest`.
  - Fake: `meta: Mutex<HashMap<String, Meta>>` on `Fake`, filled by a `record_meta` next to
    `record_mcp` in the Load and Fork handlers (remove the entry when the request has none).
    `config_options` widens the model `select` to four values when the session has a meta.
  - Every existing caller passes `None`.

- [ ] **Step 4: Run the tests**

Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent --test fake_agent && cargo build -p wai`
Expected: all PASS, wai compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-agent crates/wai/src/chat/view.rs crates/wai/src/workspace.rs
git commit -m "feat: let a session load or fork carry the agent's _meta"
```

### Task 2: Spike, real adapter honors the 1M meta (gate)

Decides the mechanism. Costs a few tiny prompts on the owner's subscription.

**Files:**
- Modify: `crates/wai-agent/tests/real_claude.rs` (one new `#[ignore]` test, kept as a
  regression check for adapter upgrades)

**Interfaces:**
- Consumes: Task 1's `load_session(.., meta)`, `fork_session(.., meta)`; `SessionEvent::Usage { used, size, .. }`.
- Produces: the confirmed meta JSON and the `[1m]` row ids. Write both into the spec under a new
  `## Spike results` section.

- [ ] **Step 1: Write the test** `one_million_context_via_meta`, `#[ignore = "needs network, npx and a Claude Code login"]`.
  It runs with the owner's real `~/.claude/settings.json` (global flag on) and asserts, printing
  each model row id:
  1. `new_session` lists no model row ending in `[1m]` (spike item 3).
  2. `load_session(same id, .., Some(meta))` right after it, before any prompt, succeeds and lists
     `[1m]` rows (items 1, 4, 6).
  3. `set_config("model", "<family>[1m]")`, then the prompt `reply with ok`: a `Usage` event has
     `size >= 900_000` (item 2).
  4. `load_session(.., None)`: no `[1m]` rows again, and a prompt still answers with the earlier
     turn in history (anti-goal: no lost conversation).
  5. `fork_session(.., Some(meta))` lists `[1m]` rows.

  For item 7, add `"CLAUDE_AGENT_ACP_IDLE_COMPACT_DELAY_MS": "5000"` to the meta's `env` in a
  second run and check the adapter's stderr for
  `is below the 1000000 minimum` (print it with `--nocapture`).

- [ ] **Step 2: Run it**

Run: `cargo test -p wai-agent --test real_claude one_million_context_via_meta -- --ignored --nocapture`
Expected: PASS. Note the printed `[1m]` row ids (e.g. `opus[1m]` vs `claude-opus-5-5[1m]`) and
whether `default` has a `[1m]` row.

- [ ] **Step 3: Gate**
  - PASS: mechanism A holds. Record results in the spec and go on.
  - Fails at 2 or 3 (no `[1m]` rows, or size stays 200k): **stop and ask the owner** about
    mechanism B. Do not edit `~/.zshrc` or `~/.claude/settings.json`.
  - Fails only at "right after `new_session`": go on, and in Task 4 keep the chip disabled until
    the session's first turn ends.
  - Row ids are not `<id>[1m]`: adjust `sized_model` in Task 4 to the real pattern.

- [ ] **Step 4: Commit**

```bash
git add crates/wai-agent/tests/real_claude.rs docs/superpowers/specs/2026-11-17-session-context-size-design.md
git commit -m "test: check the real adapter turns 1M on through _meta"
```

### Task 3: wai remembers 1M sessions and sends the meta on every load and fork

**Files:**
- Modify: `crates/wai-settings/src/lib.rs` (`AppState` at `:496`, next to `hidden` at `:533`)
- Create: `crates/wai/src/chat/context.rs` (`mod context;` in `chat/mod.rs`)
- Modify: `crates/wai/src/chat/view.rs` (`ChatEvent` `:45`, `ChatSession` `:138`, `new` `:220`, `load` `:1089`)
- Modify: `crates/wai/src/workspace.rs` (field near `hidden` `:305/:518`, `restore` `:868`,
  `save_state` `:1017`, event handler `:1228`, `fork` `:1700`, `loading_session` `:1736`)
- Test: `crates/wai-settings/src/lib.rs` tests, `crates/wai/src/chat/context.rs` tests,
  `crates/wai/src/workspace.rs` tests (copy `restore_reopens_sessions` `:5275` for restore)

**Interfaces:**
- Consumes: `wai_agent::Meta`, `load_session(.., meta)`, `fork_session(.., meta)` (Task 1).
- Produces:
  - `AppState.long_context: BTreeSet<String>` with `#[serde(default, skip_serializing_if = "BTreeSet::is_empty")]`.
  - `pub(crate) fn context_meta(long: bool) -> Option<wai_agent::Meta>` in `chat/context.rs`:
    `None` for `false`, the Global Constraints JSON for `true`.
  - `ChatSession.long_context: bool` (`pub(crate)`, default `false`).
  - `ChatEvent::ContextChanged`: the session's `long_context` changed.
  - `Workspace.long_context: BTreeSet<String>` (`pub(crate)`).

- [ ] **Step 1: Write the failing tests**
  - `wai-settings`: `long_context_round_trips` (save then load keeps `{"fake-1"}`) and
    `state_without_long_context_loads` (a state JSON with no `long_context` key keeps its
    `open_sessions`).
  - `context.rs`: `context_meta(false) == None`; `context_meta(true)` serialized equals the
    Global Constraints JSON.
  - `workspace.rs` tests, with `fake_settings("--echo-meta")`:
    - `long_session_loads_with_meta`: start a session, insert its id into
      `w.long_context`, set `s.long_context = true`, `s.load(false, ..)`, wait for Idle; the next
      turn's reply starts with `meta:{"claudeCode"` and contains `"CLAUDE_CODE_DISABLE_1M_CONTEXT":"0"`.
    - `short_session_loads_without_meta`: same without the flag; the reply is `meta:`.
    - `restore_sends_meta_for_long_ids`: write a state with one open session id in
      `long_context`, `restore`, wait for Idle; the turn reply carries the meta, and
      `s.long_context` is `true`. Same for a history row opened via `resumed` (use
      `history_settings` + `--echo-meta`).
    - `fork_inherits_long_context`: a long session forked through `w.fork(..)`: the fork's id is
      in `w.long_context`, and its turn reply carries the meta.
    - `failed_load_keeps_long_context`: a long id starting with `missing` (the fake refuses to
      load it): after the error, the id is still in `w.long_context`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p wai-settings long_context && cargo test -p wai`
Expected: compile errors (no `long_context`, no `context_meta`).

- [ ] **Step 3: Implement**
  - `load` passes `context_meta(self.long_context)` to `load_session`.
  - `restore` sets `self.long_context = state.long_context` **before** the open-sessions loop
    (that loop already goes through `loading_session`).
  - `loading_session` sets `s.long_context = self.long_context.contains(&id)` on the new
    session. Restore, history, drag and fork all go through it.
  - `fork`: `let long = self.long_context.contains(&id)`; pass `context_meta(long)` to
    `fork_session`; on success insert `started.id` when `long`, before `loading_session`.
  - Event handler: `ChatEvent::ContextChanged` inserts or removes the session's id from
    `self.long_context` from `s.long_context`, then `save_state`.
  - `save_state` writes `long_context: self.long_context.clone()`.
  - `// ponytail: ids of deleted sessions stay in the set; prune when it shows up in state size.`

- [ ] **Step 4: Run the tests**

Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-settings && cargo test -p wai`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-settings/src/lib.rs crates/wai/src
git commit -m "feat: remember 1M Claude sessions and reload them with 1M"
```

### Task 4: The Context chip and the in-place switch

**Files:**
- Modify: `crates/wai/src/chat/context.rs` (switch logic, as an `impl ChatSession` block)
- Modify: `crates/wai/src/chat/view.rs` (`take_started` `:433`, `load` success branch `~:1097`,
  fields)
- Modify: `crates/wai/src/chat/input.rs` (`render_bar` `:726-797`)
- Test: `crates/wai/src/chat/view.rs` tests

**Interfaces:**
- Consumes: `context_meta`, `ChatSession.long_context`, `ChatEvent::ContextChanged` (Task 3);
  `set_option(&mut self, option: &str, value: String, cx)` (`view.rs:499`).
- Produces:
  - `pub(crate) fn sized_model(current: &str, values: &[ConfigValue], long: bool) -> Option<String>`:
    strips a trailing `[1m]` from `current`, appends it back when `long`, returns it only if a
    value has that id. (Pattern per Task 2's results.)
  - `pub(crate) fn set_long_context(&mut self, long: bool, window: &mut Window, cx: &mut Context<Self>)`
  - `ChatSession.resizing: Option<bool>`: the size a switch is waiting to pick after its reload.
  - `ChatSession.context_picker: Option<Entity<SelectState<Vec<Choice>>>>`: `Some` only when
    `agent == "claude"`, created in `ChatSession::new`, values `200k` (id `200k`) and `1M`
    (id `1m`), selected from `long_context`. Confirm calls `set_long_context(value == "1m", ..)`.

- [ ] **Step 1: Write the failing tests**
  - `sized_model`: `("opus", [opus, opus[1m]], true) -> Some("opus[1m]")`;
    `("opus[1m]", [opus], false) -> Some("opus")`; `("haiku", [haiku], true) -> None`.
  - `view.rs` tests, with the fake agent registered under id `claude`
    (`fake_settings("--models --echo-meta")` with its `agents` key and `default_agent` renamed to
    `"claude"`):
    - `switch_to_one_million_keeps_the_session`: set model to `fake-large`, type `draft` in the
      composer, `set_long_context(true)`, wait for Idle. Same `s.id`; the model option's current
      is `fake-large[1m]`; the id is in `w.long_context`; the composer still holds `draft`; the
      next turn replies with the meta.
    - `switch_back_to_200k`: then `set_long_context(false)`: current is `fake-large`, the id is
      out of the set, the reply is `meta:`.
    - `no_one_million_row_reverts`: `s.set_option("model", "fake-tiny".into(), cx)` first (the
      fake stores any value and has no `fake-tiny[1m]` row), then `set_long_context(true)`: after
      it settles, `long_context` is `false`, the id is out of the set, the next turn replies
      `meta:`, and the notices hold an error starting `Could not switch Context`.
    - `same_value_does_nothing`: `set_long_context(false)` on a 200k session sends no load (the
      session stays Idle, no `Bound` re-emitted; check `s.bound` is unchanged).
    - `context_chip_only_for_claude`: render a frame; `window.find("bar-context")` is `Some` for
      the `claude` session and `None` with plain `fake_settings("--models")`.
    - `context_chip_disabled_during_a_turn`: with `slow_reply`, during the turn
      `window.find("bar-context")` reports disabled (copy `send_stays_next_to_stop_during_a_turn`).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p wai`
Expected: compile errors (no `set_long_context`, no `sized_model`).

- [ ] **Step 3: Implement**
  - `set_long_context`: return if `long == self.long_context` or a turn is running
    (`model.state` is `Working`/`NeedsYou`). Set `long_context`, emit `ContextChanged`, set
    `resizing = Some(long)`, call `self.load(false, window, cx)`.
  - `take_started`: also replace `config` (and set `pickers_stale`) when `resizing.is_some()`.
  - `load` success branch, after `take_started`: `if let Some(long) = self.resizing.take()`, find
    the Model option, `sized_model(&current, &values, long)`:
    - `Some(id)` → `set_option(&model_option_id, id, cx)`.
    - `None` with `long` → `long_context = false`, emit `ContextChanged`, notice
      `Could not switch Context: {model} has no 1M version`, then `self.load(false, ..)` (back
      to 200k without the meta).
    - `None` with `!long` → leave the model as the agent reports it.
  - Keep the picker's selection in sync with `long_context` after each case.
  - `render_bar`: when `context_picker` is `Some`, add a `bar_chip("bar-context", ..)` right
    before the model chip in `rest`, `.disabled(...)` while a turn runs or `self.loading`.
  - If Task 2 found "load right after new fails": also disabled until the first turn ends.

- [ ] **Step 4: Run the tests**

Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src crates/wai-agent/src/bin/wai-fake-agent.rs
git commit -m "feat: pick 200k or 1M context per Claude session"
```

### Task 5: Check it by hand in the real app

**Files:** none.

- [ ] **Step 1: Build and install** per `CLAUDE.md` ("Install a local build on this Mac"), in the
  background. Check the bundle inode changed and the running wai is still alive.
- [ ] **Step 2: Ask the owner to restart wai**, then check with them, on a real Claude session:
  - the `Context` chip shows `200k`; picking `1M` reloads the conversation in place and the usage
    text shows `/ 1000k` after the next prompt;
  - the terminal's `claude` still shows 200k;
  - quitting and reopening wai keeps the session at 1M.
- [ ] **Step 3: Review Focus 5 (downsizing above 200k).** When a real 1M session passes 200k,
  switch it to 200k and send a prompt. Expected: Claude Code compacts, the turn answers.
  If it errors instead: in `set_long_context`, refuse `long == false` while
  `usage.used > 160_000` with the notice `Run /compact first, then switch to 200k`, plus a test.
- [ ] **Step 4:** `cargo clean` (the disk is nearly full).
