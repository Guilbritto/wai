# Composer: inline files and images, and `@` mentions

Follows the research in `docs/NOTES-composer-mentions.md`. The owner asked on 2026-11-17 to take it
from design to release without asking, so every choice below is the agent's call, made by copying
Zed (memory: follow other IDEs).

## Why

- A pasted image should reach the agent at its place in the text: "compare this [img] with this
  [img]" must arrive in that order, not as all the text and then the images.
- Typing `@` should mention a project file, as in Claude Code and Zed.

## What changed since the research

The research said gpui-component 0.9 has no inline elements. It does: `gpui-base` 0.9 has **atomic
inline tokens** on `TextareaState`, the composer's own state (`input/base/inline_tokens.rs`):

- `InlineToken::new(id, text).with_label(label)`: a byte range of the text drawn as one pill;
  the caret skips it and Backspace deletes it whole. Undo brings it back.
- `replace_with_token` / `replace_range_with_token`, `tokens()`, `content()`; `set_value` takes an
  `InputContent` (text plus tokens).
- The styled `Textarea` draws a token as `InputToken` (a pill with an optional icon), or as what
  `.token(render)` returns.

So the Zed way (pills inside the message), which the research ruled out as too big, now costs less
than markers: nothing to parse, and deleting the pill deletes the attachment.

## Approaches

1. **Text markers** (`[Image #N]` typed into plain text, chips stay above): the marker and the chip
   are two things to keep in sync (delete one, what happens to the other?), and the text has to be
   parsed on send.
2. **Inline tokens, the Zed way (chosen).** Every attachment is a pill at the caret; the pill is the
   attachment. One source of truth, no parsing, and it is what Zed does.
3. **A completion menu at the caret (`EditorState`)**: swaps the composer's engine for the code
   editor's to get its completion popup. The `/` menu already shows how wai lists choices above the
   prompt box, which needs no caret position, so this buys nothing.

## Design

### Attachments become tokens

- Pasting an image or files, dropping files, or picking them with `+` inserts one token per file
  at the caret, each followed by a space. The chips row above the text goes away.
- A token's `id` is the file's absolute path. A pasted image is written to its temporary file first,
  as today, and that path is its id.
- Its `text` (what the transcript shows and what ↑ recalls):
  - an image that goes as an image (png, jpeg, gif, webp up to 5 MB): `[Image #N]`, `N` one more
    than the highest number already in the box (Claude Code's marker);
  - any other file or folder: `@` and its path relative to the session's folder, or its name when it
    is outside; a folder ends with `/`.
- Its `label` (what the pill shows, with an image, file or folder icon): `Image #N` for a pasted
  image, else the file's name.

### `@` mentions

- Typing `@` at the start of the text or after a blank, then a query without blanks, opens a menu
  above the prompt box (where the `/` menu opens) listing the project's files and folders.
- The list comes from the session's folder (the draft's folder on the new-session screen), walked
  with wai-project's gitignore-aware walker, without `.git`, at most 20 000 entries, off the UI
  thread. Each new `@` lists again, keeping the last list meanwhile.
- Matching ignores case: names that start with the query first, then names that contain it, then
  paths that contain it; 50 rows at most. No fuzzy matcher (none in the tree).
- ↑/↓ move, Enter or Tab replace `@query` with the entry's token, Esc closes the menu until the text
  changes. A click on a row picks it. With no entry matching, the menu is closed.
- It works the same in a running chat and in the new-session draft.

### Sending

- The box's content (text plus tokens), trimmed, is the message. It goes out as blocks in their
  order: the text between tokens as `Text` (blank runs dropped), each token as its file:
  `Image` when it is an image and the agent takes images, else `Resource` (a `resource_link`, which
  Claude shows as `[@name](file://…)` in place and opens with its own tools).
- The transcript shows the text as typed, the tokens' text in place. The `Attached: …` line goes.
- Queued messages, messages taken back from the queue, and what goes back to a draft whose session
  failed to start keep their tokens; joined messages are a blank line apart, as today.

## Non-goals

- Mentions of sessions, symbols, URLs, diagnostics or branches (Zed has them).
- Embedding a mentioned file's content (`resource` with text): the agent reads the link itself.
- Opening a file by clicking its pill; recalling a sent message with its pills (↑ gives its text).
- A fuzzy matcher, file-list caching across `@`s, a watcher-driven list.

## Left to the builder

- Where the shared mention code lives and how the two composers plug into it.
- The menu rows' look: reuse the `/` menu's.
