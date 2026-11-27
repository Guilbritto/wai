# Pet chat, part A: `@<pet>` mentions and the pet's background session — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `@Bolota <message>` typed first in a session's or the draft's prompt box goes to the pet extension instead of the agent, and wai-bichinho answers it in its dock bubble through a background agent session of its own.

**Architecture:** wai (extension API 1.13) lets an extension register names with `mentions/set`. The `@` menu lists them above the files. A prompt that starts with one is sent to the extension as `mention/message` and never reaches the agent. wai-bichinho 0.7.0 keeps one background session (`sessions/open` `background`), queues messages while that session runs a turn, reads the answer with `sessions/read` on `event/turn_ended`, and says its first line in a sticky bubble.

**Tech Stack:** Rust (wai, wai-ext, gpui-kit 0.9), Go (wai-bichinho).

**Spec:** `docs/superpowers/specs/2026-11-26-pet-chat-design.md`, section "Design: part A". Part B is out of scope.

## Global Constraints

- Work in the worktree `.claude/worktrees/pet-chat`, branch `feat/pet-chat`. Build in its own `target/`: never set `CARGO_TARGET_DIR` to `wai/target`.
- `cargo test -p wai` needs `cargo build --workspace --bins` first, because the fake agent and fake extension are binaries.
- Commit messages:
  - wai and wai-ext changes use `feat:`/`fix:`.
  - Extension changes (`extensions/**`) use `chore:`.
  - Plans and specs use `docs:`.
  - No scope, no `Co-Authored-By` line.
- Do not touch the main checkout. Another session has uncommitted work in its `crates/wai/src/chat/render.rs` and `view.rs`. Keep `view.rs` edits small: one enum variant, one field, tests at the end of its test module.
- API values:
  - `wai_ext::API = "1.13"`.
  - The pet's manifest `api = "1.13"` and `version = "0.7.0"`.
- Mention names: 1 to 32 characters, no blanks.
- Bubble text: at most 140 characters, cut with `…`.
- The pet's queue holds at most 5 waiting messages. A sixth gets the line "calma, um de cada vez".
- The pet's own texts:
  - "…" while it thinks.
  - "preciso de você: <title>" when its session asks for something.
  - "terminei, dá uma olhada" when the turn left no text answer.
- Never quit or kill the running wai. Never use bare `git stash`. Never pass `model: opus` to a subagent.

## Review Focus

1. **A mention sent while the session is mid-turn, starting, or in an error state.** It still goes to the extension, and nothing is queued or steered into the session. Pinned by `mention_skips_a_running_turn` (Task 4).
2. **A mention token left in the box after its extension stopped or was disabled.** It goes to the agent as the plain text `@Bolota …`, not as a file block of a bogus path. Pinned by `prompt_blocks_keep_mention_tokens_as_text` (Task 4).
3. **The pet's session was closed by the user, or wai restarted.** The next `@` opens a new session, brief included, instead of waiting forever on "…". Pinned by `TestClosedSessionOpensANewOne` (Task 5).
4. **A permission request in the pet's session while "…" is up.** The needs-you line shows at once and is not stuck behind the sticky "…", and "…" comes back after it is resolved. Pinned by `TestNeedsYouOnItsSession` (Task 5).
5. **The user's own sessions must not be disturbed.** A file named like the pet still lists below the mention. Typing `@Bolota` in the middle of a message sends it to the agent unchanged. Pinned by `addressed_by_token_or_first_word` and `at_lists_extension_mentions_first` (Tasks 3 and 4).

---

### Task 1: wai-ext — the `mentions` permission, `mentions/set`, `mention/message`, API 1.13

**Files:**
- Modify: `crates/wai-ext/src/manifest.rs` (`Permission` ~221, `Display`, `parse_permission` ~340, `API`).
- Modify: `crates/wai-ext/src/host.rs` (`ExtEvent` ~43, `ExtHandle` methods ~479–512, `call()` ~656, params structs ~832).
- Modify: `crates/wai/src/extensions_view.rs` (`permission_text` ~32).
- Modify: `docs/extensions.md`.
- Test: `crates/wai-ext/tests/manifest.rs` and `crates/wai-ext/tests/host.rs`.

**Interfaces:**
- Produces:
  - `Permission::Mentions`, written `"mentions"`. No extra manifest rule: like `sessions:*`, it is only usable by a process.
  - `pub struct Mention { pub id: String, pub name: String }` in `host.rs`. It derives `Debug, Clone, PartialEq, Eq, Deserialize` and is re-exported from the crate root like `ExtEvent`.
  - `ExtEvent::Mentions { names: Vec<Mention> }`, returned as `Call::Done`, so the extension gets `null`.
  - `ExtHandle::mention_message(&self, id: &str, text: &str, agent: Option<&str>, session: Option<&str>)`:
    - It sends the **request** `mention/message` with `{"id", "text", "agent", "session"}`; agent and session are `null` when `None`.
    - It sends only when `Permission::Mentions` was granted.
    - wai ignores the answer, as it does for `panel/action`. A failed delivery shows nothing (ruling on "left to the builder").
  - `API = "1.13"`.

- [ ] **Step 1: Write the failing tests**
  - `tests/manifest.rs`:
    - Add `mentions_permission_parses`, a copy of the shape of `listen_and_sessions_content_parse`:
      - `"process", "mentions"` parses to `[Permission::Process, Permission::Mentions]`.
      - `Permission::Mentions.to_string() == "mentions"`.
      - `"process", "mention"` is `ManifestError::Unknown`.
    - Rename `api_is_1_12` to `api_is_1_13`, with `API == "1.13"` and `with_api("1.13")` ok.
  - `tests/host.rs`, using the fake's `--call` flag and the `granted_call_gets_reply` pattern:
    - `mentions_set_reaches_the_host`:
      - Granted `[Process, Mentions]`, the call `mentions/set {"names":[{"id":"pet","name":"Bolota"}]}` yields `ExtEvent::Mentions { names: vec![Mention { id: "pet".into(), name: "Bolota".into() }] }`.
      - With only `[Process]`, the `reply` report carries an error.
    - `mentions_set_refuses_bad_names`:
      - `"Bolota Jr"` gets a reply error whose message contains `"Bolota Jr"`.
      - So do 33 × `"a"` and `""`.
      - `[]` gives `ExtEvent::Mentions { names: vec![] }`.
    - `mention_message_needs_the_permission`:
      - Granted `[Process, Mentions]`, `mention_message("pet", "oi", Some("claude"), Some("s1"))` is reported as `mention/message` with the body `{"id":"pet","text":"oi","agent":"claude","session":"s1"}`.
      - `mention_message("pet", "oi", None, None)` gives `"agent": null, "session": null`.
      - Without the grant nothing is sent: after it, `run_command("x")` is the next report.

- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p wai-ext mention api_is_1_13`
  - Expected: compile errors for `Permission::Mentions`, `Mention` and `mention_message`.

- [ ] **Step 3: Implement**
  - Add the permission.
  - Add a `"mentions/set"` arm in `call()`:
    - `need(Permission::Mentions)?`.
    - Parse `{"names": Vec<Mention>}`.
    - Check each name: `(1..=32).contains(&name.chars().count()) && !name.chars().any(char::is_whitespace)`.
    - A bad name is `RpcError::new(rpc::INVALID_PARAMS, format!("bad mention name {name:?}: 1 to 32 characters, no blanks"))`.
  - Add `mention_message`, next to `panel_action`.
  - Set `API` to 1.13.
  - `permission_text`: `Permission::Mentions => "Be mentioned with @ in the prompt box".into()`.
  - `docs/extensions.md`:
    - The header says 1.13.
    - The `api` field row: "to `"1.13"` when you use the `mentions` permission".
    - A permissions table row: `mentions` covers `mentions/set` and receiving `mention/message`, is shown as *Be mentioned with @ in the prompt box*, API 1.13.
    - A `mention/message` row in Host → extension, with this text: "request; `{"id", "text", "agent", "session"}`; the user sent a prompt starting with your mention; `text` is the rest, trimmed, never empty; `agent`/`session` are the prompt box's session, `null` on a new-session draft; wai ignores the answer. Needs `mentions`".
    - A `mentions/set` row in Extension → host: `{"names": [{"id", "name"}]}` → `null`; it replaces your mentions, `[]` removes them; a `name` is 1 to 32 characters without blanks, a bad one is an error naming it.
    - A short paragraph under the method notes on how the `@` menu lists names and routes a message (spec "Sending").
    - The "API 1.1 to 1.12" line becomes 1.13.

- [ ] **Step 4: Run them and see them pass**
  - Run: `cargo test -p wai-ext`
  - Expected: all pass. Also run `cargo build -p wai` and expect it to compile, since `permission_text` matches every variant.

- [ ] **Step 5: Commit**
  - Message: `feat: let extensions be mentioned with @ (API 1.13)`.

---

### Task 2: wai — the host keeps each extension's mentions and publishes them

**Files:**
- Modify: `crates/wai/src/ext_host.rs` (`Running` ~141, `apply` ~506, `ExtHost` methods, tests).
- Modify: `crates/wai/src/chat/mentions.rs` (the new global).
- Modify: `crates/wai/src/chat/mod.rs` (re-export it).
- Modify: `crates/wai/src/workspace.rs` (a field next to `prompt_slots` ~257, and the `observe_in(&ext_host…)` callback ~422).

**Interfaces:**
- Consumes (Task 1): `ExtEvent::Mentions { names }`, `Mention`, `ExtHandle::mention_message`.
- Produces:
  - In `ext_host.rs`, `#[derive(Debug, Clone, PartialEq)] pub struct ExtMention { pub ext: ExtId, pub id: String, pub name: String, pub ext_name: String, pub icon: Option<PanelIcon> }`.
    - `ext_name` is the manifest's `name`.
    - `icon` is the first of the extension's panels for which `panel_icon` gives `Some`; for wai-bichinho that is `card`'s `paw-print`.
  - `ExtHost::mentions(&self) -> Vec<ExtMention>`: the live extensions' mentions, in the extensions' list order, then each extension's own order.
  - `ExtHost::mention_message(&self, ext: &ExtId, id: &str, text: &str, agent: Option<&str>, session: Option<&str>)`:
    - It forwards to the running handle.
    - Under `#[cfg(test)]` it also pushes `(ext.clone(), id.to_string(), text.to_string())` to `pub(crate) mentioned: RefCell<Vec<(ExtId, String, String)>>`, whether or not the extension runs. This mirrors `sent`.
  - In `chat/mentions.rs`, `#[derive(Clone, Default, PartialEq)] pub struct ExtMentions(pub Vec<ExtMention>);` with `impl Global`. The workspace sets it only when its own last value (field `ext_mentions: Vec<ExtMention>`) differs.

- [ ] **Step 1: Write the failing test**
  - Add `mentions_follow_the_extension` in `ext_host.rs` tests, set up like `sessions_open_in_background`.
  - The fake extension is granted `process` and `mentions`, with `--call mentions/set {"names":[{"id":"pet","name":"Bolota"}]}`.
  - Wait until `host.mentions()` is a single `ExtMention` with `name == "Bolota"`, `id == "pet"` and `ext_name` the fake's name.
  - Disable the extension: `mentions()` is empty.

- [ ] **Step 2: Run it and see it fail**
  - Run: `cargo build --workspace --bins && cargo test -p wai mentions_follow_the_extension`
  - Expected: compile error, `no method named mentions`.

- [ ] **Step 3: Implement**
  - `Running.mentions: Vec<Mention>`, stored in `apply` on `ExtEvent::Mentions` while live, then `cx.notify()`.
  - A status that is not live clears it, as `items` is cleared.
  - Add `mentions()` and `mention_message()`.
  - Add the global and the workspace field.
  - In the existing ext_host observer, after `sync_contributions`: when `host.mentions() != this.ext_mentions`, store it and `cx.set_global(ExtMentions(..))`.

- [ ] **Step 4: Run it and see it pass**
  - Run: `cargo test -p wai mentions_follow_the_extension`
  - Expected: PASS.

- [ ] **Step 5: Commit**
  - Message: `feat: keep the mentions each extension registers`.

---

### Task 3: wai — mentions in the `@` menu

**Files:**
- Modify: `crates/wai/src/chat/mentions.rs`.
- Test: the same file's `mod tests`, and `crates/wai/src/chat/view.rs` tests (append after `at_mentions_a_file` ~3551).

**Interfaces:**
- Consumes (Task 2): `ExtMention` and the `ExtMentions` global, read with `cx.try_global::<ExtMentions>()`.
- Produces:
  - `pub(crate) const MENTION_TOKEN: &str = "wai-mention:";`
  - `pub(crate) fn mention_token(m: &ExtMention) -> InlineToken`, which returns `InlineToken::new(format!("{MENTION_TOKEN}{}:{}", m.ext, m.id), format!("@{}", m.name)).with_label(m.name.clone())`. Ruling: the token carries no icon, because `InlineToken` has none (file tokens are built the same way in `input.rs:188`); the menu row shows the icon.
  - `pub(crate) fn is_mention_token(token: &InlineToken) -> bool`, true when its id starts with `MENTION_TOKEN`.
  - `fn matching_mentions(mentions: &[ExtMention], query: &str) -> Vec<ExtMention>`: names that start with the query, then names that contain it, ignoring case, as `matching` does for files.
  - Rows become `enum Row { Mention(ExtMention), File(Entry) }`. The matching mentions come first, then the files, `ROWS` in all.
  - The menu also opens with mention rows before the folder listing ends.

- [ ] **Step 1: Write the failing tests**
  - Unit test `mentions_match_like_file_names`:
    - With `[Bolota, Bob]` from two extensions, `"bo"` gives both.
    - `"LOT"` gives `[Bolota]`.
    - `"x"` gives `[]`.
    - Two extensions that share the name `"Bolota"` give both rows.
  - View test `at_lists_extension_mentions_first`:
    - Set up as `at_mentions_a_file`, with the file `bolo.txt` in the folder.
    - `cx.set_global(ExtMentions(vec![bolota]))`, where `bolota` is `ext` `wai-bichinho`, `id` `pet`, `name` `Bolota` and `ext_name` `Bichinho`.
    - `cx.simulate_input("@bo")`, then wait for `"mentions"`. The row `("mention", 0)` holds the text `Bolota` and row 1 holds `bolo.txt`.
    - Press `enter`: `typed == "@Bolota "`, with one token at `0..7` whose id is `wai-mention:wai-bichinho:pet`.

- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p wai mentions_match_like_file_names at_lists_extension_mentions_first`
  - Expected: compile error or FAIL, since no mention rows exist yet.

- [ ] **Step 3: Implement**
  - In `refresh`, rows are the matching mentions plus the file matches, the latter only when files are listed.
  - In `complete`, `Row::Mention` puts in `mention_token(m)`, then `" "`.
  - The mention row's icon is drawn as `workspace.rs:3572` draws a `PanelIcon`, at 12 px. With `None` it is `Lucide::AtSign`.
  - The row shows the name, then `ext_name` as muted detail where a file shows its parent.

- [ ] **Step 4: Run them and see them pass**
  - Run: `cargo test -p wai mentions at_mentions_a_file at_lists_extension_mentions_first`
  - Expected: PASS, and `at_mentions_a_file` still passes.

- [ ] **Step 5: Commit**
  - Message: `feat: list extension mentions in the @ menu`.

---

### Task 4: wai — route a prompt that starts with a mention

**Files:**
- Modify: `crates/wai/src/chat/mentions.rs` (`addressed`).
- Modify: `crates/wai/src/chat/input.rs`: `send` ~357, `prompt_blocks` ~105, `sent` and `recall` ~717.
- Modify: `crates/wai/src/chat/view.rs`: `ChatEvent` ~46 gets one variant, `ChatSession` gets one field.
- Modify: `crates/wai/src/chat/draft.rs`: `clear`, next to `fail` ~850.
- Modify: `crates/wai/src/start.rs`: `send_draft` ~87.
- Modify: `crates/wai/src/workspace.rs`: the `add_session` subscription ~1316.
- Test: `input.rs` or `mentions.rs` unit tests, `view.rs` tests, `start.rs` tests.

**Interfaces:**
- Consumes:
  - From Task 2: `ExtMentions`, `ExtHost::mention_message`, and `ExtHost::mentioned` in tests.
  - From Task 3: `is_mention_token` and `MENTION_TOKEN`.
- Produces:
  - `pub(crate) fn addressed<'a>(message: &Message, live: &'a [ExtMention]) -> Option<(&'a ExtMention, String)>`. `message` is already trimmed. It returns the mention and the rest, trimmed:
    1. A token spanning from 0 whose id is `wai-mention:<ext>:<id>` of a mention in `live` routes.
    2. Otherwise, the first word (up to the first whitespace) routes when it is `@<name>` and `name` equals a live mention's name ignoring case (`to_lowercase`). The first match in `live` order wins.
    3. Anything else is `None`.
    - The rest is the message text after the mention, with other tokens as their text.
  - `ChatEvent::Mention { ext: ExtId, id: String, text: String }`.
  - `ChatSession.sent_mentions: Vec<(usize, String)>`: each mention message sent from this box, as typed, with the number of agent messages `sent()` counted at that moment.
  - `fn sent<'a>(items: &'a [ChatItem], mentions: &'a [(usize, String)]) -> Vec<&'a str>` in `input.rs` (today `sent(items)` at ~165): a mention `(n, text)` comes right after the n-th agent message, in send order.
  - `DraftView::clear(&mut self, window, cx)` empties the box, calls `composer::set_typing`, and emits `DraftEvent::Changed`.

- [ ] **Step 1: Write the failing tests**
  - Unit test `addressed_by_token_or_first_word`, with a live `Bolota`:
    - `"@Bolota oi"` gives `("Bolota", "oi")`.
    - `"@bolota   quais PRs?"` gives `"quais PRs?"`.
    - `"@Bolota"` gives `""`.
    - `"oi @Bolota"`, `"@Bolotas oi"` and `"@Bolota"` with `live` empty all give `None`.
    - A mention token at `0..7`, then `" veja @src/a.rs"` with a file token, gives `"veja @src/a.rs"`.
  - Unit test `prompt_blocks_keep_mention_tokens_as_text`: a message `"@Bolota oi"` with a mention token at `0..7` gives `[PromptBlock::Text("@Bolota oi")]`.
  - View test `mention_message_goes_to_the_extension`:
    - Set the global to a live `bolota`, type `"@Bolota quais PRs?"`, press `enter`.
    - `typed == ""`.
    - `s.model.items` is empty.
    - `ext_host.mentioned == [(wai-bichinho, "pet", "quais PRs?")]`.
    - Then `"@Bolota"` + `enter` sends nothing more and the box keeps `"@Bolota"`.
  - View test `mention_skips_a_running_turn`:
    - With the session in a turn (`s.model.in_turn = true`, set as other tests force state), `"@Bolota oi"` + `enter` is recorded in `ext_host.mentioned`.
    - `s.queued` is empty and nothing is steered.
  - View test `recall_brings_back_a_mention_message`:
    - Send `"first"` to the agent and wait for it to show.
    - Then send `"@Bolota oi"`.
    - `up` gives `"@Bolota oi"`; `up` again gives `"first"`.
  - `start.rs` test `draft_mention_goes_to_the_extension_without_a_session`:
    - Open a draft with the global set and type `"@Bolota oi"` + `enter`.
    - `ext_host.mentioned == [(wai-bichinho, "pet", "oi")]`.
    - The draft's text is `""` and no session was started (the workspace's sessions are unchanged).

- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo build --workspace --bins && cargo test -p wai addressed prompt_blocks_keep mention_message_goes mention_skips recall_brings_back_a_mention draft_mention`
  - Expected: compile errors, then FAILs.

- [ ] **Step 3: Implement**
  - In `ChatSession::send`, the mention check comes **before** the starting and idle gate:
    - Trim the message and call `addressed`, with `live` from the global.
    - An empty rest returns.
    - Otherwise clear the box, call `set_typing`, push to `sent_mentions`, emit `ChatEvent::Mention`, and return.
  - In the workspace's `add_session` handler, `ChatEvent::Mention` calls `ext_host.read(cx).mention_message(ext, id, text, Some(agent), session)`:
    - The values are the session's agent and id.
    - The session is `None` while it has no id yet.
  - At the top of `send_draft`, before `can_send`:
    - Call `addressed(&d.message(cx), live)`.
    - An empty rest returns.
    - Otherwise call `mention_message(.., None, None)`, `draft.clear(window, cx)`, and return.
  - In `prompt_blocks`, skip mention tokens, so their text stays in the text run.
  - `recall` calls `sent(&self.model.items, &self.sent_mentions)`, so it walks mention messages in send order too.

- [ ] **Step 4: Run them and see them pass**
  - Run: `cargo test -p wai > /tmp/pet-chat-wai.log 2>&1; tail -5 /tmp/pet-chat-wai.log`
  - Expected: `test result: ok` for every suite.
  - Also run `cargo clippy --workspace --all-targets -- -D warnings` and expect it to be clean.

- [ ] **Step 5: Commit**
  - Message: `feat: send a prompt that starts with a mention to its extension`.

---

### Task 5: wai-bichinho 0.7.0 — chat over a background session

**Files:**
- Create: `extensions/wai-bichinho/chat.go`, which holds the chat state, brief, answer extraction and mention name.
- Create: `extensions/wai-bichinho/chat_test.go`.
- Modify: `extensions/wai-bichinho/main.go`:
  - The `message` struct gets an `Error` field.
  - The `server` gets the fields `chat`, `pending`, `reqID` and `mention`.
  - Changes in `handle`, `render`, `tick` and `mood`.
- Modify: `extensions/wai-bichinho/speech.go`: `line.Show`, and the step-aside rule.
- Modify: `extensions/wai-bichinho/lines.go`: three kinds.
- Modify: `extensions/wai-bichinho/wai-extension.toml` and `extensions/wai-bichinho/README.md`.

**Interfaces:**
- Consumes (wai, Tasks 1–4):
  - The pet calls `mentions/set`, and receives `mention/message` as a request it answers with `null`.
  - Session calls:
    - `sessions/open` returns `{"agent","session"}`.
    - `sessions/read` returns `{"items","total"}`; an error means "no open session".
    - `sessions/prompt` and `sessions/show` return `null`; `sessions/show` gets an error from the 2 s limit.
  - Events: `event/permission_requested` `{agent, session, request, title}`, `event/question_requested` `{agent, session, request, message}`, and the `*_resolved` events `{agent, session, request}`.
- Produces, inside the package:
  - `type chat struct { agent, session string; busy bool; queue []string; from int; request string; showAt time.Time }`, held as `server.chat`.
  - `const maxWaiting = 5`.
  - `s.request(method string, params any, p pendingCall)`:
    - It writes a request with id `s.reqID` (incremented) and stores `pending[id] = p`.
    - `type pendingCall struct{ method, text string; retry bool }`.
    - The existing `sessions/list` call goes through it too, and `case ""` dispatches on `pending[id].method`.
  - `func mentionName(p *Pet) string`: `""` for nil (an egg), else `strings.Join(strings.Fields(p.Name), "")` cut to 32 runes.
  - `func firstLine(answer string) (text string, cut bool)`: the trimmed answer's first line, trimmed. Over 140 runes it becomes 139 runes + `"…"`. `cut` is true when `text` differs from the whole trimmed answer.
  - `func (s *server) brief() string`, an English prompt with:
    - "You are <name>, a <form name> at level <n>, feeling <mood>: the user's pet and assistant inside wai, an editor for AI agent sessions."
    - "Answer in Brazilian Portuguese."
    - "Start every answer with one sentence of at most 140 characters that stands on its own; add detail only after a blank line, and only when needed."
    - "The user's open pull requests: <url> (<reason>) per line", or "unknown" while `s.prs` is nil.
    - It ends with "The user's message:".
  - `line.Show bool`: a click shows the pet's session.
  - `speech.say` rule: a priority-3 line also steps in front of a current line whose `Kind == "chat_wait"`, which is requeued first, as a sticky lower line is today.
  - Kinds in `lines.go`:
    - `{"chat_done", 3, true, false, "the pet finished what the user asked; a click shows its session"}`, with fallback `"Terminei, dá uma olhada."`.
    - `{"chat_error", 3, false, false, "the pet's own session failed or was cancelled"}`, with fallback `"Ih, não consegui. Tenta de novo?"`.
    - `{"chat_busy", 3, false, false, "the user sent the pet more messages than it can hold"}`, with fallback `"Calma, um de cada vez."`.
  - Fixed lines, not persona lines, built directly:
    - `chat_wait`: `"…"`, priority 3, sticky.
    - `chat_answer`: priority 3, sticky, `Show = cut`.
    - `chat_needs`: `"preciso de você: " + title` (or `message`) through `firstLine`, priority 3, sticky, `Show: true`.

**Flow, as the pet's code follows it:**
- `mention/message {text}`:
  - Answer `null`.
  - If busy: queue the text when the queue holds fewer than 5, else say `chat_busy`.
  - If not busy: run `chatSend(text)`.
- `chatSend`:
  - Set busy and say `chat_wait`.
  - With no session, send `sessions/open {"background": true, "prompt": brief() + "\n\n" + text, "send": true}`, using the default agent.
  - Otherwise send the probe `sessions/read {agent, session, "limit": 1}`.
- Results:
  - The open result stores `agent`/`session` and sets `from = 0`.
  - The probe result sets `from = total`, then sends `sessions/prompt {agent, session, text}`.
  - A probe error clears the session and runs `chatSend(text)` again.
  - An open or prompt error runs `chatFailed()`.
- `event/turn_ended` on its own session:
  - When not busy, ignore it (the user typed into it).
  - With `end_turn` or `max_tokens`, send `sessions/read {agent, session, from}`.
  - Otherwise run `chatFailed()`.
  - It gives no xp and no `turn_done`.
- The read result:
  - The last item with `kind == "agent"` and non-empty `text` becomes the answer.
  - Drop `chat_wait`, say the `chat_answer` line, then `chatNext()`.
  - With no such item, say `chat_done` (with `Show`), then `chatNext()`.
- `chatFailed`: drop `chat_wait`, say `chat_error`, then `chatNext()`.
- `chatNext`: clear busy, and pop the queue into `chatSend`.
- A permission or question requested on its own session:
  - Store `request` and say `chat_needs`.
  - The matching `*_resolved` drops `chat_needs` and clears `request`.
- Bubble clicks:
  - `panel/action` with `values.bubble == "click"` on the current line with `Show` sends `sessions/show {agent, session}`, before `talk.done`.
  - On a `sessions/show` error where `!retry`, set `chat.showAt = now + 2s`. `tick()` sends it once more with `retry: true`.
- Its own session never reaches the pet's own reactions: `event/session_state` on it is ignored, so it gives no `needs_you` or `error` lines and does not drive the mood.
- `mood()` returns `"working"` while `chat.busy`. That is the thinking pose: the existing working animation (ruling on "left to the builder").
- `render()`, once ready: when `mentionName(s.st.Pet)` differs from the last name sent (nil pointer: never sent), call `mentions/set` with `{"names": []}` or `{"names": [{"id": "pet", "name": name}]}`.

- [ ] **Step 1: Write the failing tests**
  - Put them in `chat_test.go`, using `start`, `h.send`, `bubbleOf` and `sticky` from `main_test.go`.
  - `TestMentionFollowsThePet`:
    - An egg sends `mentions/set` with `names: []` once.
    - Hatching then renaming to `"Bolota Jr"` sends `[{"id":"pet","name":"BolotaJr"}]`.
    - A render with no change sends nothing.
  - `TestFirstMessageOpensABackgroundSession`:
    - `mention/message {"id":"pet","text":"oi"}` writes `sessions/open` with `background` and `send` true.
    - Its `prompt` contains the pet's name, `"Brazilian Portuguese"`, and ends with `"oi"`.
    - The bubble is `"…"` and sticky.
  - `TestAnswerBecomesTheBubble`:
    - After the open result `{"agent":"claude","session":"s1"}`, `event/turn_ended {claude, s1, end_turn}` writes `sessions/read` with `from: 0`.
    - The read result `[user, agent "Tudo certo.\n\nDetalhes"]` gives the bubble `"Tudo certo."`.
    - The pet's XP is unchanged.
    - A click on it writes `sessions/show {claude, s1}`.
  - `TestShortAnswerClickJustCloses`: the answer `"Oi!"`; a click writes no `sessions/show`.
  - `TestLongFirstLineIsCut`: `firstLine` of 200 × `"a"` is 140 runes ending in `"…"`, with `cut` true.
  - `TestNoAnswerSaysDone`: items with no `agent` give the `chat_done` fallback, and a click shows the session.
  - `TestBusyQueuesUpToFive`:
    - While busy, 5 messages queue and a sixth gives `"Calma, um de cada vez."`.
    - The turn's end sends the next as a probe (`sessions/read` with `limit: 1`).
    - Its result `{"total": 4}` then writes `sessions/prompt` with that text.
    - The next answer read uses `from: 4`.
  - `TestClosedSessionOpensANewOne`: a probe error writes `sessions/open` again, with the brief.
  - `TestOwnSessionIsIgnored`:
    - `event/session_state` `needs_you` on `claude/s1` says nothing.
    - The same on another session says the `needs_you` line.
  - `TestNeedsYouOnItsSession`:
    - While `"…"` shows, `event/permission_requested {claude, s1, request "r1", title "Run gh pr list"}` shows `"preciso de você: Run gh pr list"` at once.
    - `event/permission_resolved {claude, s1, request "r1"}` brings `"…"` back.
  - `TestFailedTurnSaysError`:
    - `turn_ended` `cancelled` gives the `chat_error` fallback and the next queued message is sent.
    - A `sessions/open` error does the same.
  - `TestShowRetriesOnce`:
    - A `sessions/show` error, then a tick 2 s later, writes `sessions/show` again.
    - A second error writes no third one.

- [ ] **Step 2: Run them and see them fail**
  - Run: `cd extensions/wai-bichinho && go test ./...`
  - Expected: FAIL, with undefined `firstLine` and `mentionName`.

- [ ] **Step 3: Implement as the Interfaces and Flow above say.**
  - Manifest:
    - `version = "0.7.0"`, `api = "1.13"`.
    - `permissions = ["process", "panels", "sessions:read", "sessions:content", "sessions:control", "mentions", "events:wai-waycarbon"]`.
    - The description mentions that it answers `@<name>`.
  - README:
    - Rewrite the privacy paragraph: what you send with `@` goes to your default agent; its background session runs with your agent's rights and asks as your own sessions do; the persona line pools keep the old promise.
    - A short "Talk to it" section: `@<name> <message>` in any prompt box, the answer in the bubble, and a click shows the session.

- [ ] **Step 4: Run them and see them pass**
  - Run: `cd extensions/wai-bichinho && go test ./... && go vet ./... && gofmt -l .`
  - Expected: `ok`, no vet output, and no files listed.

- [ ] **Step 5: Commit**
  - Message: `chore: let the pet answer @ mentions through a background session`.

---

### Task 6: Release (stop and ask the owner first)

- [ ] **Step 1:** Rebase `feat/pet-chat` on `origin/development`. Run `cargo build --workspace --bins && cargo test --workspace` and `go test ./...` in the pet. Both are green.
- [ ] **Step 2:** **Ask the owner** before any merge or push. With their go:
  - Merge into `development`.
  - Run `git log --oneline -5 development` and check that only merged work goes out.
  - Run `git branch -f main development` (only when `main` is an ancestor) and push both.
  - Watch the release with `gh run watch <id> --exit-status`, using `GH_TOKEN=$(gh auth token -u Guilbritto)`.
  - The expected result is the wai minor release with API 1.13.
- [ ] **Step 3:** After that release is out, ship the pet as the 0.6.0 release was shipped:
  - Tag `wai-bichinho-v0.7.0` and push it.
  - Watch its workflow.
  - List it in `extensions/index.toml` as `chore: list wai-bichinho 0.7.0 in the official index`.
  - Push again.
- [ ] **Step 4:** Update the memory `bichinho-roadmap.md`: step 3 part A is done, with the versions; part B comes next.
