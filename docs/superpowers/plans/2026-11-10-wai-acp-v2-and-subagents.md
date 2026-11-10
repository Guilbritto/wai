# wai on claude-agent-acp 0.103.0, with subagents: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** wai runs the Claude adapter 0.103.0 and shows each running Claude Code subagent (Task/Agent tool) of a session as its own line in the Agents tab, with its own status and what it is doing. A background subagent keeps its line after the main turn ends, until it finishes.

**Architecture:**
- **`wai-agent`** declares `clientCapabilities.subagents: {}`. A new `Subagents` map in the connection's broker sends everything a subagent session reports to the open session it belongs to (its root, at any depth). The rest of wai never deals with child session ids.
- **The `wai` crate's chat model** keeps a list of the session's subagents and gives each one a card in the transcript. A subagent's tool calls still show inline, tagged with it, and its permission cards show in the session's chat.
- **The Agents tab** draws one line per running subagent under its session's card.

**Tech stack:** Rust 2024; `agent-client-protocol = "5"` (lock: 5.0.0, schema 1.15.1) with the new feature `unstable_subagents`, protocol **v1**; GPUI through `gpui-kit = "=0.9.0"`; `@agentclientprotocol/claude-agent-acp@0.103.0`.

**Spec:** none. The decisions are in "Intent and decisions" below. The sources are the 0.103.0 package (`dist/serve.js`, `dist/native-subagents.js`, `dist/v1/acp-agent.js`, `dist/plans.js`, `README.md` "Subagent sessions" and "Experimental ACP v2"), the [subagents RFD](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/rfds/subagents.mdx), and the crate sources under `~/.cargo/registry/src/*/agent-client-protocol-{5.0.0,5.1.0}` and `agent-client-protocol-schema-{1.15.1,1.16.0}`.

---

## Intent and decisions

The owner's decisions are marked **(owner)**. Decisions made while writing this plan are marked **(call)**.

1. **Move to the new Claude adapter, 0.103.0. (owner)** wai pins 0.101.0 today. The owner asked to "upgrade to the new ACP protocol version", believing 0.103.0 serves only ACP v2. It does not:
   - `dist/serve.js` in 0.103.0 serves both versions by default (`CLAUDE_AGENT_ACP_PROTOCOL` unset means `auto`): "a client that requests version 2 or later gets the draft ACP v2, and every other client gets ACP v1".
   - The release note "serve ACP v2 with a direct v2 agent and remove the v1 bridge" (#1560) means this: in 0.101.0, v2 was served by wrapping the v1 agent (`v2AgentApp(logger, onAgent)`). In 0.103.0, v2 has its own agent (`dist/v2/`), and v1 moved to `dist/v1/acp-agent.js`.
   - The release's breaking change is about library imports (`deep imports of dist/ files fail`). wai runs the binary, so it is not affected.
   - The modules a v1 client meets barely changed. `acp-subagents.js` is byte-identical between 0.101.0 and 0.103.0, and `elicitation.js` only gained helpers.
2. **wai stays on ACP v1, with the adapter at 0.103.0. (owner)** Confirmed by the owner after reading these reasons:
   - **ACP v2 is a draft.** The announcement ([acp-v2-draft](https://agentclientprotocol.com/announcements/acp-v2-draft)) says "Don't ship it by default in production until we are closer to stabilization", and "Adding v2 support should not mean dropping v1."
   - **The crate gates it.** v2 sits behind `unstable_protocol_v2`, which "may change in a minor release".
   - **v2 is a second client, not a flag.** Its session API differs: `v2::ActiveSession` only sends commands, `session/prompt` returns before the turn ends, there is no `tool_call`, `plan` becomes `plan_update`, and `session/load` and modes are removed. wai would have to keep its v1 client for Codex, Copilot, Antigravity and the fake agent anyway.
   - **Subagents do not need v2.** On v1 they come with the `subagents` client capability, and 0.103.0 gives v1 clients the same child sessions (README, "Subagent sessions").
   - **Every other agent keeps working**, because nothing about the protocol version changes. Agents that do not know `subagents` see an optional capability they do not read. Task 5 checks this against the real CLIs where `real_agents.rs` exists.
3. **Subagents as child sessions, "Option B". (owner)** wai declares `clientCapabilities.subagents` (crate feature `unstable_subagents`). The adapter then:
   - announces each subagent with `subagent_update` on its parent, with a title and a state: `running`, `requires_action`, `idle` (with a stop reason) or `unknown`;
   - sends the subagent's own updates and requests (tool calls, text, plan, permission, AskUserQuestion) under the child's session id;
   - hides the launching Agent/Task call, and sends a foreground subagent's answer as a `session_message` from the child to the parent;
   - forwards subagent text to the child session without `forwardSubagentText` (`forwardsSubagentText()` is true whenever the client supports subagents);
   - sends a subagent's TodoWrite to the child session (`todoWritePlanUpdate`: "It does not with `childSessions`"). Dropping child plans therefore fixes the root-plan overwrite.
4. **Routing lives in `wai-agent`. (call)** One map, child → root, in the broker, filled from `subagent_update` (the RFD says the announcement comes before any child traffic). The broker emits everything for the root session:
   - `SessionEvent::Subagent(SubagentPatch)` for announcements and states, flattened at any depth;
   - a child's `ToolCall`, with `subagent: Some(child)`, and its `ToolCallUpdate`;
   - a child's permission request or question, with `session` = root and `subagent: Some(child)`.

   A child's text, thoughts, plan, usage, title, modes and commands are dropped. That is what wai shows today, where the adapter strips subagent text. It also follows the RFD: no summed costs, and no child slash commands.
5. **What the chat shows. (call)**
   - A tool-style card per subagent (title, status, and its answer when it arrives). It replaces the Agent/Task card the adapter now hides.
   - The subagent's tool calls stay inline as today, so diffs, edits and Revert keep working.
   - A permission card from a subagent is titled `{subagent title}: {request title}`.
6. **`current_tool` and the plan are the main agent's. (owner: "fix both")** `ChatModel::current_tool` skips calls tagged with a subagent. The plan fix comes from decision 3.
7. **Stop cancels only the session's own open requests. (owner)** wai answers `cancelled` only to requests that the root session itself sent, on Stop and at the end of a turn alike.
   - **A background subagent's** permission and question cards stay open until the user answers them. The adapter does not stop background children on the parent's `session/cancel` (README, "Subagent sessions").
   - **A foreground subagent** is stopped by the adapter itself: Claude Code stops the children the turn waits on. The adapter then:
     - withdraws the child's open requests: the abort signal in `awaitingUser` (`dist/native-subagents.js`) becomes `$/cancel_request` in the ACP SDK (`@agentclientprotocol/sdk` 2.5.0, `dist/core/jsonrpc.js`, `prepareRequest`);
     - reports the child `idle` with `cancelled`.

     wai already resolves a withdrawn question as `Cancelled`. A withdrawn permission is not handled today, so its card would stay open with no one waiting for it. **(call)** wai watches permission withdrawals the way it watches questions (Task 2). That is the agent ending the request, not wai.
8. **A session's state follows the open cards, not only the turn. (call)** A background subagent can ask before or after the parent's turn ends.
   - At the end of a turn, the session reads `Needs you` while a subagent's card is still open, not `Idle`.
   - Answering a card after the turn has ended returns the session to `Idle`. Today, any answer sets the session to `Working`.
9. **The Agents tab shows only live subagents. (owner)**
   - A line is shown while its subagent is `Running` (`{title} · Working`) or `NeedsYou` (`{title} · Needs you`), plus ` · {current tool}` when there is one.
   - The line disappears when the subagent finishes (`Done`, `Stopped`, `Failed` or `Ended`).
   - **(call)** `Unknown` is hidden too. It comes from the agent exiting (decision 11) or from the adapter losing a child (`disconnected`). The RFD forbids showing it as live, and the session card already shows an exit as `Error`.
   - **(call)** There is no cap: live subagents are as many as Claude runs at once, so the list cannot grow without bound.
10. **No crate bump. (call)** 5.1.0 (2026-11-09) adopts schema 1.16.0, whose `v1/client.rs` is identical to 1.15.1. The lock stays on 5.0.0.
11. **When the agent exits (RFD MUST). (call)** Subagents still `running` or `requires_action` become `Unknown`: a last-known running state must not be shown as live.

## Base branch and overlap with `feat/settings-and-agents`

**Recommendation:** start this plan only after `feat/settings-and-agents` has merged into `development`, then `git rebase development` this branch. The branch holds only this plan, so the rebase is trivial.

That branch (main checkout, at its Task 7 of 11) has already changed the files this plan changes:
- **`crates/wai-agent/src/actor.rs`:** `Command::SetConfig`, `program`, `session_started(.., config)`.
- **`crates/wai-agent/src/types.rs`:** `SessionStarted.config`, `ConfigOption`, `SessionEvent::ConfigChanged`.
- **`crates/wai-agent/src/convert.rs`:** `config_options`.
- **The fake agent:** `--models`, plus its `Opts` and its header list.
- **`crates/wai-agent/tests/fake_agent.rs`.**
- **`crates/wai-settings/src/lib.rs`:** `BUILTIN_AGENTS`, which holds the Claude command.
- **`crates/wai/src/chat/model.rs`:** the `ConfigChanged` arm.

Its remaining tasks also change these files:
- Task 10: `chat/model.rs`, `chat/view.rs` and `chat/render.rs`;
- Task 11: `crates/wai-agent/tests/real_agents.rs` and `docs/user-guide.md`.

Starting in parallel would mean conflicts in every task here. This plan is written against the merged tree. Where a path differs before the merge, the task says so in one line.

## Global constraints

- **Protocol:** ACP v1 only. `InitializeRequest::new(ProtocolVersion::V1)` stays, and `unstable_protocol_v2` stays off.
- **Crate:** `agent-client-protocol = "5"`. `crates/wai-agent/Cargo.toml` features become `["process", "stdio", "unstable_session_fork", "unstable_subagents"]`. No other new features and no new dependencies (`cargo deny` runs in CI).
- **Claude command:** `npx -y @agentclientprotocol/claude-agent-acp@0.103.0`. The other built-ins are unchanged:
  - `npx -y @agentclientprotocol/codex-acp@2.6.0`
  - `agy acp`
  - `copilot --acp --stdio`
- **The capability, exactly:** `ClientCapabilities::new().elicitation(..).subagents(acp::SubagentCapabilities::new())`, which goes on the wire as `"subagents": {}`.
- **RFD MUSTs that wai keeps:**
  - it never sends `session/load`, `session/resume` or any other request naming a child session;
  - it answers every request a child sends;
  - it never shows a last-known `running` as live after the agent exits.
- **UI copy:** English. The exact strings are in the tasks.
- **Platforms:** must still build on Windows x86_64.
- **The CI gate:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace`.
- **Commits:** `feat:`, `fix:`, `docs:` or `test:`, with no scope and no `Co-Authored-By`.

## Review focus

These are the failure modes most likely to bite, most likely first. Each one has a test, named here, in the task that owns the code.

1. **A background subagent asks for permission while, or after, its parent's turn ends.**
   - The card must show in the parent's chat, and the answer must reach the agent (not `Stale`).
   - The session reads `Needs you` while the card is open, and `Idle`, not `Working`, once it is answered.
   - Tests: Task 2 `background_permission_outlives_the_turn`; Task 3 `answer_after_the_turn_returns_to_idle`.
2. **Stop while a subagent's card is open.**
   - A background subagent's card stays open, and answering it still works.
   - A foreground subagent's card closes as `Cancelled` when the adapter withdraws it.
   - Tests: Task 2 `stop_leaves_a_background_subagents_request_open` and `withdrawn_permission_resolves_on_its_own`.
3. **Several subagents at once.**
   - Each one's tool calls and state stay on its own line.
   - None of their text lands in the main reply.
   - The main plan and the card's "tool in progress" stay the main agent's.
   - Tests: Task 1 `parallel_subagents_stay_apart`; Task 3 `subagent_tools_stay_off_the_main_card` and `subagent_shows_in_the_chat` (the main plan survives).
4. **A subagent launched by a subagent.** Its line shows under the same session card, and its traffic reaches the same chat. Test: Task 1 `nested_subagent_routes_to_the_root`.
5. **The agent exits while subagents run.** Their lines must not keep saying `Working`. Test: Task 3 `exit_marks_live_subagents_unknown`.

---

### Task 1: `wai-agent` declares subagents and routes their updates to the root session

**Files:**
- Modify: `crates/wai-agent/Cargo.toml` (the feature)
- Create: `crates/wai-agent/src/subagents.rs` (with its `mod tests`)
- Modify:
  - `crates/wai-agent/src/lib.rs` (`mod subagents;`)
  - `crates/wai-agent/src/types.rs`
  - `crates/wai-agent/src/convert.rs`
  - `crates/wai-agent/src/actor.rs` (initialize, the notification handler, `Broker`)
  - `crates/wai-agent/src/bin/wai-fake-agent.rs`
- Compile fixes only: every `ToolCallView { .. }` literal the compiler names (`crates/wai/src/chat/model.rs` tests), and `crates/wai/src/chat/model.rs` `apply` (temporary `=> None` arms for the two new events; Task 3 replaces them).
- Test: `crates/wai-agent/tests/fake_agent.rs`

**Interfaces:**
- **Produces, in `types.rs`:**
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SubagentState { Running, NeedsYou, Done, Stopped, Failed, Ended, Unknown }`
  - `#[derive(Debug, Clone, PartialEq)] pub struct SubagentPatch { pub id: SessionId, pub title: Option<String>, pub state: Option<SubagentState> }`. `None` means unchanged. `ponytail:` a `null` clear also reads as unchanged.
  - `SessionEvent::Subagent(SubagentPatch)` and `SessionEvent::SubagentReply { id: SessionId, text: String }`
  - `ToolCallView.subagent: Option<SessionId>`: the subagent session that made the call, `None` for the session's own.
- **Produces, in `convert.rs`:**
  - `pub(crate) fn subagent_patch(u: acp::SubagentUpdate) -> SubagentPatch`, which maps `state` like this:

    | ACP `state` | `SubagentState` |
    |---|---|
    | `running` | `Running` |
    | `requires_action` | `NeedsYou` |
    | `idle` + `end_turn` | `Done` |
    | `idle` + `cancelled` | `Stopped` |
    | `idle` + `error` | `Failed` |
    | `idle` with no or another stop reason | `Ended` |
    | `unknown`, or any other state | `Unknown` |

  - `pub(crate) fn subagent_reply(m: acp::SessionMessage, session: &str) -> Option<SessionEvent>`: `SubagentReply` when `sender_session_id` is a value other than `session` and `content` holds text blocks (joined with `\n`), else `None`. `ponytail:` `session_message_chunk` is not read, because the adapter sends whole messages on v1.
- **Produces, in `subagents.rs`:**
  - `#[derive(Default)] pub(crate) struct Subagents { roots: HashMap<SessionId, SessionId> }`
  - `pub(crate) fn root(&self, session: &str) -> Option<&SessionId>`: `Some` only for a subagent session.
  - `pub(crate) fn route(&mut self, session: SessionId, update: acp::SessionUpdate) -> Option<(SessionId, SessionEvent)>`

    It returns the event for wai and the open session it is for, following this algorithm:

    ```rust
    let root = self.roots.get(&session).cloned();
    match (update, root) {
        (U::SubagentUpdate(u), root) => {
            let root = root.unwrap_or(session);
            self.roots.insert(u.session_id.to_string(), root.clone());
            Some((root, SessionEvent::Subagent(convert::subagent_patch(u))))
        }
        (U::SessionMessage(m), None) => convert::subagent_reply(m, &session).map(|e| (session, e)),
        (update, None) => Some((session, convert::session_event(update)?)),
        (update, Some(root)) => match convert::session_event(update)? {
            SessionEvent::ToolCall(mut v) => { v.subagent = Some(session); Some((root, SessionEvent::ToolCall(v))) }
            e @ SessionEvent::ToolCallUpdate(_) => Some((root, e)),
            // ponytail: a subagent's text, plan, usage and commands are not shown.
            _ => None,
        },
    }
    ```
- **Produces, in `actor.rs`:**
  - `Broker.subagents: Mutex<Subagents>`
  - `fn on_update(&self, n: acp::SessionNotification)`: routes the notification and emits `AgentEvent::Update`. The `SessionNotification` handler in `connect` becomes `updates.on_update(n)`.
  - `initialize` adds `.subagents(acp::SubagentCapabilities::new())`.
- **Fake agent flag `--subagent`:**
  - Like claude-agent-acp, it does this only for a client that advertised `subagents` at initialize (store it like `forms`). Otherwise the turn has no subagent.
  - The turn runs the subagent after `--plan`'s plan and before the reply. The child session is `<session>-sub`.
  - In order:
    1. on the parent: `subagent_update` with title `Explore the code` and state `running`;
    2. on the child: a tool call `sub-read`, `Read lib.rs`, kind `read`, `in_progress`;
    3. on the child: a plan with one entry, `Sub step`;
    4. on the child: an agent message chunk, `sub text`;
    5. on the child: `sub-read` completed;
    6. on the parent: `session_message` `sub-reply`, from the child to the parent, with the text `found it`;
    7. on the parent: `subagent_update` with state `idle` and `end_turn`.
  - Document the flag in the bin's header list.

- [ ] **Step 1: Write the failing unit tests** in `subagents.rs`'s `mod tests`. Build the updates with the schema builders, for example `acp::SubagentUpdate::new("c").title("Explore".to_string()).state(acp::StateUpdate::Running(acp::RunningStateUpdate::new()))`.
  - `subagent_updates_route_to_the_root`:
    - `route("r", sub("c", "Explore", running))` is `Some(("r", Subagent(SubagentPatch { id: "c", title: Some("Explore"), state: Some(Running) })))`;
    - `route("c", tool_call("t1", "Read"))` is `Some(("r", ToolCall(v)))` with `v.subagent == Some("c")`;
    - `route("c", tool_call_update("t1", completed))` goes to `"r"`;
    - `route("c", agent_chunk("x"))` and `route("c", plan(..))` are `None`;
    - `route("r", agent_chunk("hi"))` is `Some(("r", AgentText("hi")))`.
  - `nested_subagent_routes_to_the_root` (Review focus 4): after `"c"` is announced under `"r"`, `route("c", sub("g", ..))` gives `("r", Subagent { id: "g", .. })`, `route("g", tool_call(..))` gives `("r", ToolCall { subagent: Some("g"), .. })`, and `root("g") == Some("r")`.
  - `parallel_subagents_stay_apart` (Review focus 3): with `"a"` and `"b"` announced under `"r"`, interleaved tool calls from `"a"` and `"b"` come out tagged `Some("a")` and `Some("b")` respectively.
  - `states_and_replies_map`:
    - every row of the state table above maps as listed;
    - `route("r", SessionMessage("m").sender_session_id("c").recipient_session_id("r").content([text "found it"]))` is `("r", SubagentReply { id: "c", text: "found it" })`;
    - the parent's outgoing view (`sender_session_id("r")`) is `None`.
- [ ] **Step 2: Write the failing integration test** `subagent_traffic_reaches_the_parent` in `fake_agent.rs`. Use `spawn(&["--subagent"])`, a new session `r`, `prompt`, then `until` `TurnEnded`. Assert that:
  - every `AgentEvent::Update` has `session == r.id`;
  - the events hold `Subagent(SubagentPatch { id: "<r>-sub", title: Some("Explore the code"), state: Some(Running) })`, and later one with `state: Some(Done)`;
  - a `ToolCall` has `id == "sub-read"` and `subagent == Some("<r>-sub")`;
  - `SubagentReply { id: "<r>-sub", text: "found it" }` is among them;
  - no `SessionEvent::Plan` arrives;
  - `agent_text(&events) == "Hello from fake agent"`.
- [ ] **Step 3: Run them and confirm they fail.**
  - Run: `cargo test -p wai-agent --lib subagents`
  - Expected: a compile error (no module `subagents`).
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent --test fake_agent subagent_traffic`
  - Expected: a compile error, or a FAIL with no `Subagent` event.
- [ ] **Step 4: Implement** the feature, the types, `subagent_patch`, `subagent_reply`, `Subagents`, `Broker::on_update`, the capability and the fake's `--subagent`. Fix the struct literals the compiler names.
- [ ] **Step 5: Run the crate's tests.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent && cargo build -p wai`
  - Expected: PASS, and `wai` builds.
- [ ] **Step 6: Commit.** `git commit -m "feat: route subagent sessions to their parent session"`

---

### Task 2: Subagent permissions and questions, Stop, and background subagents

**Files:**
- Modify:
  - `crates/wai-agent/src/types.rs`
  - `crates/wai-agent/src/actor.rs` (`Parked::Permission`, `park_permission`, `park_question`, the permission handler in `connect`)
  - `crates/wai-agent/src/bin/wai-fake-agent.rs`
- Compile fixes only: every `PermissionRequest { .. }` and `QuestionRequest { .. }` literal the compiler names (`crates/wai/src/notify.rs`, `crates/wai/src/chat/model.rs` tests).
- Test: `crates/wai-agent/tests/fake_agent.rs`

**Interfaces:**
- **Consumes:** `Subagents::root` (Task 1).
- **Produces:**
  - `PermissionRequest.subagent: Option<SessionId>` and `QuestionRequest.subagent: Option<SessionId>`. With a subagent, `session` is its root, the open session the card shows in, and `subagent` is the session that asked.
  - The broker parks a request under the session that asked (`Pending.session` = the child). `Command::Cancel` and `end_turn`, which keep calling `cancel_pending(Some(&root))`, therefore cancel only the root's own requests (decision 7). Do not add any cancel that reaches a subagent's requests.
  - `QuestionUnsupported { session }` names the root too.
  - **A withdrawn permission resolves on its own**, as a question does today:
    - `Parked::Permission` becomes `Permission { responder, _watch: Sender<()> }`;
    - `park_permission(self: &Arc<Self>, request, responder, cx: &ConnectionTo<Agent>) -> Result<(), Error>` spawns the same watch as `park_question`: when `responder.cancellation()` fires first, it calls `answer(id, None)`, which emits `PermissionResolved`;
    - the permission handler in `connect` passes its `cx`.

    This is how a stopped foreground subagent's card closes (decision 7). It fixes the same gap for the root session's own permissions.
  - Never hold the `pending` and `subagents` locks at the same time.
  - The `wai` crate needs no routing change: `Workspace::route` already finds the session by `request.session`, which is now the root. Before this task, a subagent's request matched no open session, and `unrouted` cancelled it.
- **Fake agent:**
  - With `--subagent`, `--ask-permission` and `--ask-question` are asked by the subagent, on `<session>-sub`, after its `sub-read` call starts. For `--ask-permission` that is the same `edit-1` `Write notes.md` tool call and request, sent on the child.
    - Like the adapter's `awaitingUser`, the subagent reports `requires_action` on the parent before asking, and `running` again once answered.
    - The turn still replies `chose:…` or `answered:…`.
  - New flag `--background`: with `--subagent`, the subagent outlives the turn.
    - The turn streams its reply and ends right after the subagent's tool call starts. Slow that reply with `--chunk-delay-ms` and a long `--reply` to keep the turn running.
    - With `--ask-permission`, the permission request is on the wire before the first reply chunk, and the turn does not wait for it.
    - The rest (the call completes, the reply, then `idle` `end_turn`) follows once that permission is answered, or 100 ms after the turn ends without one.
  - New flag `--withdraw-permission`: with `--ask-permission`, first send the same permission request and withdraw it at once (`$/cancel_request`), like `--withdraw-question`.
  - Document all three in the header list.

- [ ] **Step 1: Write the failing tests** in `fake_agent.rs`:
  - `subagent_permission_reaches_the_parent`:
    - `spawn(&["--subagent", "--ask-permission"])`;
    - `permission(&rx)` has `session == r.id` and `subagent == Some("<r>-sub")`;
    - answering `allow-once` ends the turn with text containing `chose:allow-once`.
  - `subagent_question_reaches_the_parent`: with `--subagent --ask-question`, `question(&rx)` has `session == r.id` and `subagent == Some("<r>-sub")`.
  - `stop_leaves_a_background_subagents_request_open` (Review focus 2):
    1. `spawn(&["--subagent", "--background", "--ask-permission", "--reply", &vec!["word"; 200].join(" "), "--chunk-delay-ms", "20"])`;
    2. wait for the permission, then `agent.cancel(r.id)`;
    3. the turn ends `Cancelled`, and the events up to `TurnEnded` hold no `PermissionResolved { id: request.id }`;
    4. `answer_permission(request.id, Some("allow-once".into()))` is `Ok(())`;
    5. a `Subagent { state: Some(Done), .. }` follows.
  - `withdrawn_permission_resolves_on_its_own` (Review focus 2), mirroring `withdrawn_question_resolves_on_its_own`:
    1. with `--ask-permission --withdraw-permission`, the first `PermissionRequested` resolves (`PermissionResolved` with its id) with no answer;
    2. answering it afterwards is `Err(BrokerError::Stale)`;
    3. the second request still answers `Ok(())`, and the turn's text contains `chose:allow-once`.
  - `background_subagent_ends_after_the_turn`: with `--subagent --background`, the events up to `TurnEnded` (reason `EndTurn`) hold `Subagent { state: Some(Running), .. }` and no `Done`. Then `until` a `Subagent { state: Some(Done), .. }` arrives.
  - `background_permission_outlives_the_turn` (Review focus 1):
    1. with `--subagent --background --ask-permission`, the events up to `TurnEnded` hold the `PermissionRequested` (with `subagent: Some(..)`), a `Subagent { state: Some(NeedsYou), .. }`, and no `PermissionResolved`;
    2. `answer_permission(id, Some("allow-once".into()))` is `Ok(())`;
    3. a `Subagent { state: Some(Done), .. }` follows.
- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent --test fake_agent subagent_ stop_leaves withdrawn_permission background_`
  - Expected: a compile error (`subagent` is not a field of `PermissionRequest`).
- [ ] **Step 3: Implement** the fields, the re-attribution in `park_permission` and `park_question`, the permission withdrawal watch, and the fake's flags.
- [ ] **Step 4: Run the tests.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent && cargo build -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: subagent permissions and questions show in their session"`

---

### Task 3: The chat model keeps the subagents; `current_tool` and late answers are fixed

**Files:**
- Modify: `crates/wai/src/chat/model.rs`, and the `ToolItem { .. }` literals the compiler names (`crates/wai/src/chat/edits.rs`, `crates/wai/src/chat/render.rs` tests)
- Test: `crates/wai/src/chat/model.rs` `mod tests`, `crates/wai/src/chat/view.rs` `mod tests`

**Interfaces:**
- **Consumes:** `SessionEvent::Subagent`, `SessionEvent::SubagentReply`, `SubagentState`, `ToolCallView.subagent` (Task 1), `PermissionRequest.subagent` and `QuestionRequest.subagent` (Task 2).
- **Produces:**
  - `ToolItem.subagent: Option<String>`, copied from `ToolCallView.subagent`.
  - `#[derive(Debug, Clone, PartialEq)] pub struct Subagent { pub id: String, pub title: String, pub state: SubagentState }`
  - `ChatModel.subagents: Vec<Subagent>`, in announcement order.
  - `ChatModel.in_turn: bool`: set by `push_user`, cleared by `on_turn_ended` and `on_exited`.
  - `ChatModel::subagent_tool(&self, id: &str) -> Option<&str>`: the title of the latest `Pending` or `InProgress` `ToolItem` whose `subagent` is `id`, and only while that subagent is `Running` or `NeedsYou`.
- **`apply(Subagent(patch))`:**
  - **First sight:** push `Subagent { id, title: patch.title or "Subagent", state: patch.state or Unknown }`, and a card `ChatItem::Tool(ToolItem { id: patch.id, title, kind: "subagent", status, subagent: None, .. })`.
  - **Later patches:** update the line and the card's title and status.
  - It returns the card's index.
  - The card's status from the state: `Running`, `NeedsYou` → `InProgress`; `Done`, `Ended` → `Completed`; `Stopped`, `Failed` → `Failed`; `Unknown` → `Pending`.
- **`apply(SubagentReply { id, text })`:** sets the card's `text` and returns its index.
- **`current_tool`:** skips items whose `subagent` is `Some`. The card itself has `subagent: None`, so the main card still shows a foreground subagent it waits on, as it showed the Agent call before.
- **`after_answer`:** `NeedsYou` while a card is open, else `Working` when `in_turn`, else `Idle`.
- **`on_turn_ended`:** clear `in_turn` first. The branches that set `Idle` then set `self.state = self.after_answer()` instead: `NeedsYou` while a subagent's card is still open, else `Idle`. The broker resolves the root's own cards before `TurnEnded` arrives, so only subagent cards can still be open (decision 8).
- **`on_permission` and `on_question`:** with `subagent: Some(id)` of a known subagent, prefix the request's `title` (a permission) or `message` (a question) with `{subagent title}: `.
- **`on_exited`:** every subagent that is `Running` or `NeedsYou` becomes `Unknown`, and so does its card's status.

- [ ] **Step 1: Write the failing model tests** in `model.rs`. Add a `subagent` field to the `tool` helper, and give the tools distinct titles.
  - `subagent_gets_a_card_and_a_line`:
    - applying `Subagent { id: "c", title: Some("Explore"), state: Some(Running) }` returns `Some(ix)`;
    - `items[ix]` is a `Tool` with id `"c"`, title `"Explore"` and status `InProgress`;
    - `subagents == [Subagent { id: "c", title: "Explore", state: Running }]`;
    - `SubagentReply { id: "c", text: "found it" }` sets the card's text;
    - then `Subagent { id: "c", title: None, state: Some(Done) }` makes the card `Completed` and keeps the title `"Explore"`.
  - `subagent_tools_stay_off_the_main_card` (Review focus 3): after `push_user`, an own call `Own` (`InProgress`) and then a subagent call `Sub` (`subagent: Some("c")`, `InProgress`, after `"c"` was announced running):
    - `current_tool() == Some("Own")`;
    - `subagent_tool("c") == Some("Sub")`;
    - `subagent_tool("d") == None`.
  - `answer_after_the_turn_returns_to_idle` (Review focus 1):
    - **Asked after the turn:** `push_user`, then `on_turn_ended(&EndTurn)`. `on_permission(PermissionRequest { subagent: Some("c".into()), ..perm() })` makes the state `NeedsYou`, and `on_permission_answered(id, "Yes")` makes it `Idle`.
    - **Asked during the turn:** `push_user`, then that `on_permission`, then `on_turn_ended(&EndTurn)` leaves the state `NeedsYou` (and so does `on_turn_ended(&Cancelled)`). The answer then makes it `Idle`.
    - **Answered during a turn:** the answer still gives `Working`.
  - `subagent_permission_names_the_subagent`: with `"c"` titled `Explore`, a request titled `Write notes.md` with `subagent: Some("c")` is stored with the title `Explore: Write notes.md`.
  - `exit_marks_live_subagents_unknown` (Review focus 5):
    - `"a"` is `Running` and `"b"` is `Done`;
    - after `on_exited(None)`, `"a"` is `Unknown` and its card is `Pending`, and `"b"` stays `Done`.
- [ ] **Step 2: Write the failing end-to-end test** `subagent_shows_in_the_chat` in `chat/view.rs` tests, following `sending_prompt_streams_reply`:
  1. use `fake_settings("--subagent --plan")`, `start_session`, then `send("hi", cx)`;
  2. `wait_until` the state is `Idle` and the subagent `<id>-sub` is `Done`.
  3. Assert that:
     - the card `<id>-sub` is `Completed`, titled `Explore the code`, with text `found it`;
     - the item `sub-read` has `subagent == Some("<id>-sub")`;
     - `model.plan.len() == 3` (the subagent's one-entry plan did not replace it);
     - the last agent text is `Hello from fake agent`.
- [ ] **Step 3: Run them and confirm they fail.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai subagent answer_after_the_turn exit_marks`
  - Expected: a compile error (no `subagents` and no `subagent_tool`).
- [ ] **Step 4: Implement** the model changes. Replace Task 1's temporary `=> None` arms.
- [ ] **Step 5: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 6: Commit.** `git commit -m "feat: subagent cards in the chat"`

---

### Task 4: The Agents tab lists each session's running subagents

**Files:**
- Modify: `crates/wai/src/agents_view.rs`

**Interfaces:**
- **Consumes:** `ChatModel.subagents` and `subagent_tool` (Task 3), `crate::sidebar::status_color` (existing).
- **Produces:**
  - `pub fn subagent_line(title: &str, state: SubagentState, tool: Option<&str>) -> Option<String>`:
    - `Running` gives `{title} · Working`;
    - `NeedsYou` gives `{title} · Needs you`;
    - either one adds ` · {tool}` when there is a tool;
    - every other state (`Done`, `Stopped`, `Failed`, `Ended`, `Unknown`) gives `None` (decision 9).
  - `summary` gains `subagents: &[String]`, between `plan` and `tool`, and appends `; {line}` for each line. There is no cap (decision 9). Update the existing callers and tests.
- **The card:** after the plan rows and before the current tool, one row per subagent whose `subagent_line` is `Some`, in announcement order.
  - Each row has a 6 px dot, coloured with `status_color` of `Working` for `Running` and `NeedsYou` for `NeedsYou`.
  - Then the line, truncated, in `text_xs`.
  - A session with no live subagent draws no rows, as today.

- [ ] **Step 1: Write the failing tests:**
  - `subagent_lines_show_only_live_ones`:
    - `subagent_line("Explore", Running, Some("Read lib.rs")) == Some("Explore · Working · Read lib.rs")`;
    - `subagent_line("Explore", NeedsYou, None) == Some("Explore · Needs you")`;
    - `Done`, `Stopped`, `Failed`, `Ended` and `Unknown` give `None`;
    - `summary("t", None, &[], &["a · Working".into()], None) == "t; a · Working"`.
  - `agents_tab_lists_running_subagents` (`#[gpui_kit::test]`, following `agents_tab_shows_needs_you_and_plan`):
    1. use `fake_settings("--subagent --background --ask-permission")`, then one session and a prompt;
    2. `wait_until` the turn has ended and the subagent is `NeedsYou` (it waits on its permission card, after the turn);
    3. render and click `tab-agents`. The label of `("agent-card", 0usize)` contains `; Explore the code · Needs you · Write notes.md` (its latest running call);
    4. answer the card through the session (`answer` with `allow-once`, as `permission_card_buttons_answer` does);
    5. `wait_until` the subagent is `Done`;
    6. render again. The label no longer contains `Explore the code`.
- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai subagent_lines agents_tab_lists_running`
  - Expected: a compile error (no `subagent_line`).
- [ ] **Step 3: Implement it.**
- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: subagents in the agents tab"`

---

### Task 5: Pin claude-agent-acp 0.103.0, check the real agents, update the docs

**Files:**
- Modify:
  - `crates/wai-settings/src/lib.rs` (the `claude` entry of `BUILTIN_AGENTS`; before the settings merge, `DEFAULT_CLAUDE_COMMAND`)
  - `crates/wai-agent/tests/real_claude.rs` (`COMMAND`, plus one ignored test)
  - `docs/user-guide.md`
  - `README.md` (only if it names the version)

**Interfaces:**
- **Consumes:** everything above.
- **Produces:** the Claude built-in command `npx -y @agentclientprotocol/claude-agent-acp@0.103.0`.

- [ ] **Step 1: Write the failing test.** In `wai-settings`'s `defaults_have_four_agents_and_no_default`, add `assert_eq!(s.agents["claude"].command, "npx -y @agentclientprotocol/claude-agent-acp@0.103.0");`. Before the merge, change the assertion in `defaults_when_no_files` instead.
- [ ] **Step 2: Run it and confirm it fails.**
  - Run: `cargo test -p wai-settings`
  - Expected: FAIL (`…@0.101.0` != `…@0.103.0`).
- [ ] **Step 3: Change the command.**
  - Change it in `BUILTIN_AGENTS` and in `real_claude.rs`'s `COMMAND`.
  - Update any other `0.101.0` in `crates/` (`grep -rn 'claude-agent-acp@0.101' crates`). The comment in `convert.rs` that names `0.101.0` for the AskUserQuestion form becomes `0.103.0`: its `elicitation.js` only gained helpers.
- [ ] **Step 4: Add `subagent_runs_as_a_child_session` to `real_claude.rs`** as `#[ignore = "needs network, npx and a Claude Code login"]`, in the file's style.
  1. Write `<dir>/hello.txt` containing `wai`.
  2. Prompt: `Use the Agent tool exactly once, with subagent_type general-purpose, to read hello.txt and report its content. Then reply with that content only.`
  3. Assert, until `TurnEnded`:
     - a `Subagent` patch with `state: Some(Running)` arrives, and a later one for the same id with `Done` or `Ended`;
     - at least one `ToolCall` has `subagent.is_some()`;
     - the turn ends `EndTurn`.
  4. Print the subagent's title.
- [ ] **Step 5: Run the real checks where a Claude Code login exists.**
  - Run: `cargo test -p wai-agent --test real_claude -- --ignored --nocapture`
  - If `crates/wai-agent/tests/real_agents.rs` exists (settings Task 11), also run `cargo test -p wai-agent --test real_agents -- --ignored`. That confirms Codex, Copilot and Antigravity still initialize with `subagents` advertised.
  - Expected: PASS.
  - If the machine has no login, add `real agents not run` to the commit body. The owner then runs these checks before merging.
- [ ] **Step 6: Update `docs/user-guide.md`:**
  - **`### Chatting`, the transcript:** each subagent shows as a card with its title, its status and its answer. Its tool calls stay in the transcript. Its own text and checklist are not shown.
  - **`### Permissions and questions`:** a subagent's card is titled `<subagent>: <request>`.
    - Stop cancels only the session's own cards.
    - A background subagent's card stays open, even after the turn ends, until you answer it, and the session reads `Needs you` meanwhile.
    - A subagent the turn was waiting on stops with the turn, and its card closes as `Cancelled`.
  - **`## The Agents tab`:** add "its running subagents, one line each: `title · Working` or `title · Needs you`, then the tool call in progress". A line goes away when its subagent finishes. A background subagent keeps its line after the turn ends, until it finishes.
  - **Commands:** every `claude-agent-acp@0.101.0` in the guide and in `README.md` becomes `@0.103.0`.
- [ ] **Step 7: Run the whole CI gate.**
  - Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace`
  - Expected: PASS.
- [ ] **Step 8: Commit.** `git commit -m "feat: claude-agent-acp 0.103.0 and subagent docs"`

---

## Not in this plan, on purpose

- **ACP v2.** Add it when v2 leaves draft, or when an agent wai needs speaks only v2. What it takes:
  - `unstable_protocol_v2`, and `Client::protocol_connector` with a `.v2()` client next to the `.v1()` one. The connector falls back to v1, on a fresh connection when needed.
  - A second actor loop for v2's command-only sessions, `state_update`, `tool_call_update`-only and `plan_update`.
  - wai's domain types in `types.rs` are protocol-neutral, so the UI would not change.
- **Stopping one subagent.** The adapter grants `capabilities.cancel` per child, so `session/cancel` with the child's id would stop it. Add a stop button on the line if the owner wants it.
- **Opening a subagent's own transcript** (its text, thinking and checklist) in a pane. The adapter forwards that text now; wai drops it in `Subagents::route`.
- **Finished subagents in the Agents tab.** The owner chose live ones only. A finished subagent stays visible as its card in the chat.
- **`null` clears and `session_message_chunk`** in subagent patches (`ponytail:` comments in Task 1).
- **A subagent's `cwd`** (`isolation: "worktree"`). R34's first-sight check resolves relative paths against the session's `cwd`. ACP paths are absolute, so it only matters for an agent that sends relative ones.
- **The flat `_meta.claudeCode.parentToolUseId` fallback**, for an adapter that ignores `subagents`. 0.101.0 already honoured the capability (`acp-subagents.js` is identical), so no pinned Claude adapter needs it.
