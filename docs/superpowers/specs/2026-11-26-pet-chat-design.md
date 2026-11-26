# Pet chat: `@<pet>` in the prompt box, answered from the dock

Roadmap step 3 of the Bichinho (memory: bichinho-roadmap), with most of step 5 folded in. Asked
for on 2026-11-26.

## Why

The owner wants the Bichinho to be "an assistant that helps with things inside wai", not a pet that
only reacts. In their words: it "can do anything it manages to do". They picked how to talk to it:
type `@<pet name> <message>` in a prompt box, and the pet answers from where it lives, the dock at
the foot of the sidebar.

## Goals

- `@Bolota quais PRs precisam de mim?` typed in any prompt box (a session's or the new-session
  draft's) goes to the pet, not to that session's agent. The prompt box clears and the session is
  left alone.
- The pet answers in its speech bubble in the dock. A long answer is cut in the bubble, and a click
  shows the whole conversation.
- The pet thinks and acts through a real agent session (Claude Code or the user's default agent),
  running in the background, with every tool that agent has: terminal, files, `gh`, the user's MCP
  servers.
- It also acts inside wai: it lists, reads, opens, prompts, shows and cancels sessions, and knows
  the user's PRs (part B).
- It never acts behind the user's back on anything the agent would ask about: when its session
  asks for a permission or a question, the pet says "preciso de você" and a click shows that
  session.

## Non-goals

- Acting unprompted (the pet notices a failing PR and fixes it without being asked). That stays
  roadmap step 5's remainder.
- Learning from how the owner uses wai (roadmap step 4).
- A chat panel or chat history UI of the pet's own. The background session *is* the history.
- Mentions in the middle of a message. Only a message that **starts** with the pet's mention goes
  to the pet.

## Anti-goals

- The pet answering another session's permission request or question for the user. It may read
  that a session waits, and show it; the user answers.
- Wai tools (part B) showing up in every session the user starts. They belong to the pet's
  session only.
- A message meant for the pet reaching the focused session's agent, or the other way round.

## Prior art

- **VS Code chat participants** (`@workspace`, `@terminal`): an extension contributes a name, the
  user types `@name` first in the chat box, and that participant answers instead of the default
  one. Copied here (memory: follow other IDEs), except the answer shows where the pet lives.
- **Slack bots**: `@bot message` in any channel, the bot answers.

## Approaches

1. **Background agent session plus wai tools over MCP (chosen by the owner).** The pet opens one
   background session and talks to it through the extension API wai already has
   (`sessions/open` `background`, `sessions/prompt`, `event/session_update`, `event/turn_ended`).
   Its hands inside wai are an MCP server the pet ships, passed only to the pet's session. The
   agent's own permission prompts protect the user. The most work: wai gets the mention routing
   and the scoped MCP server, the pet gets the chat and an MCP bridge.
2. **`claude -p` per message, as the pet's line pools do today.** Simpler, but there is no
   permission prompt: either it can do almost nothing or it does everything without asking, and the
   user cannot see what it did. Rejected: it fails "anything it can" safely.
3. **Approach 1 without wai tools.** It talks and uses terminal and `gh`, but cannot drive wai's
   sessions. Kept as the first delivery (part A) of approach 1, not as an alternative.

Delivered in two parts, each its own plan and release:

- **Part A:** the `@` mention in wai, and the pet's chat over its background session.
- **Part B:** the pet's wai tools over MCP, scoped to its own session.

## Design: part A

### wai: extension mentions (extension API 1.13)

- **Permission `mentions`** (*Be mentioned with @ in the prompt box*). Needs `process`.
- **`mentions/set`** request, extension → wai: `{"names": [{"id": string, "name": string}]}`
  replaces the extension's mentions; `[]` removes them. `name` is what the user types after `@`:
  1 to 32 characters, no blanks; a bad entry is an error naming it. The pet sends its pet's name
  with blanks removed ("Bolota Jr" → `BolotaJr`, agent's call), again whenever it is renamed or
  switched, and `[]` while it is an egg.
- **The `@` menu.** The query matches mentions like file names (starts with, then contains,
  ignoring case). Matching mentions list **above** the files, each with the extension's icon and
  its name, and the extension's name as muted detail. Two extensions may share a name: both list,
  and the row picked decides (agent's call).
- **Picking one** inserts a token as files do: text `@<name>`, label `<name>`, the extension's
  icon.
- **Typed in full counts too** (agent's call, from the anti-goal): a message whose first word is
  `@<name>` as plain text, matching a mention's name ignoring case, is routed as if it were the
  token. Otherwise `@Bolota oi` typed fast would reach the session's agent.
- **Only the chat prompt box** (a session's and the new-session draft's) routes mentions; the
  extension `prompt`/`prompt-side` panels are not prompt boxes.
- **Sending.** When the message starts with a mention (only blanks before it), wai does not send
  the message to the agent. It sends
  `mention/message` to the extension: `{"id", "text", "agent", "session"}`, where `text` is the
  message after the mention, other tokens as their text (`@path`, `[Image #N]`), trimmed, and
  `agent`/`session` are the prompt box's session (`null` on a draft). The prompt box clears; on a
  draft no session starts. An empty `text` is not sent and the box keeps the mention.
- **The send works in any state:** during a turn it is not queued or steered into the session.
- ↑ recall in that prompt box brings the message back like any sent message.
- A mention whose extension stopped (disabled, crashed) drops out of the menu, and a token of it
  already in a box goes to the agent as plain text.

### wai-bichinho 0.7.0

- **Permissions** grow to `process`, `panels`, `sessions:read`, `sessions:content`,
  `sessions:control`, `mentions`, `events:wai-waycarbon`. The README's privacy paragraph is
  rewritten: what you send it with `@` goes to the agent, the background session runs with your
  agent's rights, and the persona pools keep the old promise.
- **Its session.** The first `@` message opens a background session with the user's default agent
  (part B's tools need an agent that takes MCP servers, as Claude Code and Codex do; with another
  agent the pet chats without them, agent's call):
  `sessions/open {"background": true, "prompt": <brief + message>, "send": true}`. Later messages go
  to it with `sessions/prompt`. The pet keeps its `agent`/`session` in memory only; when that
  session is closed or wai restarts, the next message opens a new one.
- **Its permissions** (owner's choice): the session runs in the mode a new session of the user's
  would, so the agent asks before commands and edits as in the user's own sessions; "always allow"
  in that session lets it ask less over time. The extension API gets no mode parameter.
- **The brief** (first prompt only, before the user's message): who it is (name, form, level, mood
  from `pet.json`), that it is the user's assistant inside wai, to answer in Brazilian Portuguese,
  and to start every answer with one sentence of at most 140 characters that stands on its own,
  more detail after a blank line only when needed. It also gives the user's PRs from the last
  `prs.mine` event (URL and reason), as of when the message was sent.
- **Busy.** While its session runs a turn, new messages wait in a FIFO queue (at most 5; a sixth
  gets the bubble "calma, um de cada vez"). `event/turn_ended` sends the next.
- **Thinking.** From the send until the turn ends, the pet shows a "thinking" pose or animation and
  a sticky bubble "…".
- **The answer.** The pet notes the transcript's `total` when it sends. On `event/turn_ended`
  `completed` it reads the transcript from there (`sessions/read`) and takes the last `agent` item
  with text (agent's call). Its first line, cut to 140 characters with "…", becomes a sticky
  bubble. No such item: the line "terminei, dá uma olhada" whose click shows the session. A click on
  a bubble that was cut shows the session (`sessions/show`); a click on one that fit just closes
  it. A `sessions/show` refused by the 2 s limit is retried once 2 s later (agent's call).
- **Its own session is not a session it reacts to** (agent's call): its turns give no xp, and its
  states do not drive the mood, `needs_you` or error lines; the lines below cover it.
- **Needs you.** `event/permission_requested` or `event/question_requested` on its session gives the
  sticky bubble "preciso de você: <title or message>", cut to 140; a click shows the session. It
  goes when the request is resolved.
- **Failures.** A turn that ends `cancelled` or with an error, and a `sessions/open` error, give a
  short error line from the pool.
- **Speech queue.** The answer, "…" and "preciso de você" lines are priority 3 (urgent, as
  `needs_you`): they step in front of a sticky PR bubble, which comes back after.
- **An egg** has no mention (`mentions/set []`), so `@` never lists it.

## Design: part B (wai tools)

### wai: an MCP server for an extension's own sessions (API 1.14)

- `contributes.mcp_servers` gets `own_sessions = true`: wai passes that server only to sessions the
  extension opened with `sessions/open`, not to the user's sessions. Since the pet's session is not
  kept across restarts, wai does not need to remember which extension opened a loaded session.

### wai-bichinho 0.8.0

- **`wai-bichinho mcp`**: the pet's binary in MCP stdio mode, contributed with `own_sessions = true`.
  It forwards each tool call to the running pet process over a local socket in the pet's data
  folder, guarded by a random token in a file only the user can read, and the pet process makes the
  host call.
- **Tools:**
  - `list_sessions` (`sessions/list`): agent, session, title, state.
  - `read_session` (`sessions/read`): a page of a session's transcript.
  - `open_session` (`sessions/open`): with a prompt, sent or not, in a pane or in the background.
  - `prompt_session` (`sessions/prompt`), `show_session` (`sessions/show`), `cancel_session`
    (`sessions/cancel`).
  - `my_prs`: the last `prs.mine` list (URL, reason).
- No tool answers permissions or questions (anti-goal).
- The brief tells it that these tools exist and to prefer opening a new session for long work
  (fixing a PR) over doing it in its own session, so its own stays free to answer.

## Constraints

- Another session has uncommitted work in `crates/wai/src/chat/render.rs` and `view.rs` on the main
  checkout. Part A touches `chat/mentions.rs`, `chat/input.rs` and `chat/draft.rs`, and maybe
  `view.rs`: build it in the `pet-chat` worktree and rebase when that work lands.
- Order: each part's wai change ships in a wai release before the pet version that needs it is
  tagged and listed; part B is planned after part A is out.
- API numbers: whatever is next when each part merges (other sessions bump it too); the numbers
  above are as of today (1.12).
- wai commits `feat:`/`fix:`; extension commits `chore:`; no scope, no Co-Authored-By.

## Left to the builder

- The `mention/message` delivery as request or notification, and how wai shows a failed one.
- The "thinking" pose: a new art mood or a reuse of an existing one.
- The exact brief text and the error line pools.
- The socket and token details in part B, as long as they work on macOS, Linux and Windows.
