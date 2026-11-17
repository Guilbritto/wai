# Composer: images in order and `@` file mentions (research)

Research from 2026-11-16. Nothing here is designed or approved: it feeds a brainstorm to run after the
"Campo" visual redesign.

## What the owner wants

- A pasted image should be referenced at its place in the text, so the agent gets text and images in
  the order they were written.
- Typing `@` should mention a file, like a citation.

## How wai sends a prompt today

- Two composers share the code: the chat's (`crates/wai/src/chat/input.rs`) and the new chat's
  (`crates/wai/src/chat/draft.rs`). Both use `Textarea` with `.appearance(false)`.
- Files dropped, pasted (`paste_into`) or picked with `+` (`pick_paths`) become an `Attachment`
  (`input.rs`). A png, jpeg, gif or webp up to 5 MB becomes `PromptBlock::Image`. Anything else
  becomes `PromptBlock::Resource { path }`. A pasted image is first written to a temporary file.
- `blocks()` in `input.rs` builds the prompt as one `Text` holding the whole message, then one block
  per attachment in the order they were attached. To an agent without image support, an image goes as
  a `Resource`. The same `blocks()` serves a prompt, a steered message and a queued one.
- `content_block` (`crates/wai-agent/src/convert.rs`) maps `Text` to an ACP `text`, `Image` to an ACP
  `image`, and `Resource` to an ACP `resource_link` with the file name and a `file://` URI.
- The transcript shows the text plus a line `Attached: a, b` (`Message::shown`).

So the agent gets all of the text first and the images after it. Nothing ties an image to a spot in the
text.

## What Claude does with the blocks

Read in `@agentclientprotocol/claude-agent-acp` 0.103.0, `dist/prompt-to-claude.js`
(`promptToClaude`). It walks the ACP blocks in order and keeps that order:

| ACP block | What Claude gets, in place |
|---|---|
| `text` | the text |
| `image` (base64) | an image block |
| `resource_link` (`file://`) | the text `[@name](file:///path)`; Claude opens the file with its own tools if it wants |
| `resource` with text | `[@name](uri)` in place, plus `<context ref="uri">…</context>` appended after the whole prompt |
| `resource` with an image or PDF blob | `[@name](uri)` then the image or document block |

Interleaving therefore needs no protocol change: `[text "compare this", image, text "with this", image]`
reaches Claude in that order. The other agents (Codex and the rest) were not checked.

## What gpui-component 0.9 offers

The three text states share one engine (`gpui-base-0.9.0/src/input/base/kind.rs`):

- `InputState`: a single line.
- `TextareaState`: plain multi-line text. This is what the composer uses.
- `EditorState`: source code, with language features.

None of them is rich text. None draws an image or an atomic chip inside the text, and there is no
API for inline elements.

Only `EditorState` has the two pieces that `@` would need:

- **Completions.** The `CompletionProvider` trait (`input/editor/lsp/completions.rs`):
  - `completions(text, offset, trigger)` opens a menu at the caret;
  - `CompletionMenuOptions` configures that menu;
  - match highlighters are provided: `highlight_prefix`, `highlight_contains`, `highlight_characters`.
- **Decorations.** `TextDecoration` collections (`input/editor/decorations.rs`) give a style to a byte
  range, such as a color or a background. That is enough to paint `@src/main.rs` or `[Image #1]` as a
  token.

`TextareaState` has neither.

A spike would need to check that an `EditorState` can pass for the current composer:

- no gutter or line numbers;
- soft wrap and auto-grow;
- a placeholder and `.appearance(false)`;
- the same paste hook;
- no code look.

## How others do it

- **Claude Code (CLI):**
  - Pasting an image inserts `[Image #N]` at the cursor, a reference by position. Users report this,
    but the official docs don't describe it.
  - A pasted path to an image file also becomes `[Image #N]`.
  - `@` opens a fuzzy search over files and folders and inserts `@path`.
- **Zed (agent panel):**
  - `@` mentions files, directories, symbols, earlier threads, skills, diagnostics, branch diffs and
    URLs to fetch.
  - All context lives as mentions inside the message editor, drawn as inline pills.
  - Pasted images and dropped files and folders work as well.
  - Code copied from a buffer becomes a mention with its file context, and ⌘⇧V pastes raw text.
  - Zed is an ACP client too; claude-agent-acp handles its `zed://` URIs.
- **VS Code (Copilot Chat):**
  - `#` references files, folders and symbols, and `#codebase` searches the workspace.
  - An "Add Context" picker does the same.
  - `@` is for chat participants, not files.
  - Pasted or dropped images become attachments above the input, not inline.

## Options for wai (not decided)

1. **Images in order, the Claude Code way.** This is the smallest option.
   - Pasting, dropping or picking an image inserts `[Image #N]` at the caret. The chip above the text
     stays.
   - On send, the text is split at the markers and the blocks are interleaved.
   - It works with today's `Textarea`; no component change.
2. **`@` mentions.**
   - `@` opens a list of the project's files. Listing them can use wai-project's gitignore-aware walker
     (`crates/wai-project/src/walk.rs`). There is no fuzzy matcher in the tree yet.
   - Choosing a file inserts `@relative/path`, painted as a token.
   - On send, each `@path` that resolves becomes a `resource_link` at its place, so Claude sees
     `[@name](file://…)` exactly there. Folders work the same way.
   - It needs the composer on `EditorState` (completion menu plus decorations), or a wai-made popup
     over the `Textarea`. The popup would have to find the caret's position.
3. **Inline pills, the Zed way.** Out: it needs a new text component with atomic inline elements,
   which is far larger than 1 and 2.

## Open questions for the brainstorm

- Marker syntax: `[Image #1]` as in Claude Code, or something like `@image-1`?
- If the marker is deleted, does the attachment go too? If the attachment is removed, does its marker go?
- The scope of `@`: only files and folders, or also sessions, URLs and symbols as in Zed?
- Should a mentioned file go as a `resource_link` (the agent reads it) or as an embedded `resource`
  (wai reads it and Claude gets a `<context>` block)? If embedded, how large can it be?
- How the transcript shows a sent message with markers and mentions (today it adds `Attached: …`).
- What other agents (Codex) do with interleaved blocks and `resource_link`.

## Sources

- claude-agent-acp 0.103.0, `dist/prompt-to-claude.js` (local npx cache).
- Zed, Agent Panel: https://zed.dev/docs/ai/agent-panel.html
- VS Code, Add context to chat: https://code.visualstudio.com/docs/copilot/chat/copilot-chat-context
- Claude Code `[Image #N]` at the cursor (user report, not official docs):
  https://claudeissues.com/issue/38562-docs-pasted-images-now-insert-an-image-n-chip-at-cursor-docs-dont-describe-this
- Claude Code `@` mentions: https://www.promptlayer.com/glossary/claude-code-at-mention
