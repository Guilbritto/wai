# The pet speaks: speech bubble and a pet agent

## Intent

The bichinho shows a mood word under its drawing, but it does not talk. The owner wants it to talk: short lines in a speech bubble that react to what happens in wai (a session needs you, a PR got approved, it levelled up, it is hungry). The owner also wants the pet to be an agent from the start, not a list of canned lines: its personality comes from its stats, and it remembers what it went through.

The look was settled with a mockup (`.superpowers/brainstorm/25625-1794923604/content/pet-speech-v2.html`, gitignored): option A, a bubble in the pet's own column, above the drawing, in the app's normal fonts (the retro font was tried and dropped).

## Goals

- A bubble above the pet with one line at a time, animated: pop in, typewriter, hold with a shrinking timer line, fade out.
- The bubble never gets in the way: it waits while the mouse is on it, dims and waits while the user types in the prompt box, and closes on click.
- Urgent lines first: a session that needs you or an error jumps the queue; "needs you" stays until answered, shrunk to a bouncing "!".
- A personality from the pet's four stats (Focus, Calm, Chaos, Charm), its species, rarity, shininess and level, written by Claude.
- A memory: a short diary of what the pet lived through, fed back to Claude so lines change over time.

## Non-goals

- A chat with the pet. It talks, the user does not answer it in text.
- Lines written live for each event. Lines come from pools written ahead of time, so speaking is instant and costs nothing per event.
- Changing the status word under the drawing (it stays English, as today).
- A bubble in other extensions' trees: the `bubble` node is generic, but only the bichinho uses it now.

## Anti-goals

- A pet that nags. Idle chatter at most once every 15 minutes, nothing while it sleeps, and the user's own typing always wins.
- A pet that leaks work. Claude gets the pet's own data only: stats, level, mood, counts and the diary. No session titles, prompts, file names or PR URLs.
- A pet that breaks without Claude. With no `claude` on the machine, or when a call fails, it speaks fixed lines.

## Constraints

- The extension API is 1.8 on `development` and not released yet (`v0.19.0` has 1.7). The `bubble` node joins 1.8. If a release ships 1.8 before this lands, it moves to 1.9.
- Extension trees are untrusted: the new node is parsed like every other (`crates/wai-ext/src/panel.rs`), and a broken one is `Unsupported`.
- The pet panel is `prompt-side`, 160 px wide, `frame = false`: it is anchored to the prompt box's bottom and grows upward over the conversation. A bubble on top of the column needs no layout change.
- GPUI (gpui-fast 0.1.13) cannot scale or rotate a `div`, only sprites and SVGs. So the pop is a fade with a small rise and overshoot instead of the mockup's scale, and the tail is a triangle drawn with `canvas`, not a rotated square.
- wai already needs a logged-in `claude` CLI. The extension uses it, not the Anthropic SDK, so the user needs no API key.
- Extension commits are `chore:` and ship on their own tag (`wai-bichinho-v0.3.0`). wai commits are `feat:`/`fix:`.

## Approaches

**Where the bubble lives** (decided with the mockup): A, in the pet's column above the drawing (chosen); B, a floating bubble beside the column; C, a status-line ticker. A needs no new layout and stays inside the panel's own box.

**Who animates it:**

1. *Host node* (chosen). A new `bubble` node; wai runs the pop, typewriter, hold, pause on hover, dim while typing, and fade. The extension sends only the text. Reason: the pause on hover and the dim while typing need the host. An extension only learns about the mouse through clicks, and never sees the prompt box.
2. *Extension only.* Typewriter by re-sending `text` every 500 ms frame. Choppy (2 characters per second), no hover pause, no dim, no fade. Dropped.

**Where the lines come from:**

1. *Pools written by Claude* (chosen). In the background, at hatch, at level up and every 6 hours, the extension asks Claude for about 6 lines per event, in character, and keeps them in `pet.json`. Speaking picks one. Instant, cheap, works offline from the last pools.
2. *A Claude call per line.* Each line fits the moment, but it lags by seconds and costs a call per event. Dropped.
3. *Fixed lines only.* No personality. Kept as the fallback.

## The host: `bubble` node (API 1.8)

```json
{"type": "bubble", "id": "say", "text": "Subi de nível!", "tone": "success", "sticky": false}
```

| Field | Rules |
|---|---|
| `id` | Required. Joins the tree's id set (buttons, form fields, sections, tabs) |
| `text` | Required, one line, at most 140 characters (cut beyond) |
| `tone` | Optional; colours the border, the tail and the timer line. Neutral uses the theme's border |
| `sticky` | Optional bool. A sticky bubble never times out |

Its life, on wai's clock:

| Phase | Behaviour |
|---|---|
| Pop | 200 ms: opacity 0→1 and a 6 px rise with overshoot (ease-out-back) |
| Typewriter | One character every 30 ms, starting with the pop |
| Hold | After the last character: 2.5 s + 50 ms per character, at most 7 s. A 2 px timer line shrinks along the bottom. The hold time does not count while the mouse is on the bubble or while the prompt box has text |
| Leave | 150 ms: fades out and rises 4 px |
| Sticky | No timer line. After 6 s of hold time it shrinks to a "!" that bounces; hovering shows the text again |

- **While the prompt box has text,** the bubble is drawn at 25% opacity.
- **Clicking it** starts the leave.
- **When the leave ends,** wai sends `panel/action` with the bubble's `id` as `action_id` and `values: {"bubble": "click"}` or `{"bubble": "timeout"}`, instead of the form's values. That is how the extension learns the line is over.
- **When the extension sends a different `text` for the same `id`,** a new bubble pops.
- **When the extension removes the node,** the bubble disappears at once, without the leave.
- **When the same text comes back after a removal,** it pops again.

"The prompt box has text" is a new global in wai, `PromptTyping`. The session or draft that holds the prompt slots sets it when its box goes from empty to typed, or back. The workspace recomputes it when the focused pane changes.

## The extension: speech rules

- **One line at a time.** About 250 ms pass between lines: the next goes out on the first frame after the gap.
- **Priority.**
  - 3 (urgent): `needs_you`, `error`. Jumps the queue and replaces a non-sticky line on screen.
  - 2: PR approved, PR to fix, level up, new egg, hatch.
  - 1: everything else.
- **Dedupe.** A line whose text is already on screen or in the queue is dropped.
- **Queue.** At most 3 lines; when full, the lowest-priority, oldest line goes.
- **Sticky `needs_you`.** It is removed when no session needs you any more.
- **Chatter** (priority 1, not from a click): at most one every 15 minutes, and none while asleep.
- **Click answers.** Lines from a click (Pet, Feed, Hatch) always speak.
- **Focus switch.** It drops a non-sticky line on screen; a sticky one stays.
- **Safety net.** A non-sticky line not over after 30 s is dropped, for when the slot is hidden and wai never times it out.
- **The mouth.** While the line types (30 ms per character plus 200 ms), a calm face (mood `idle`) becomes mood `talk`, whose mouth opens and closes each frame.

The events and their fallback lines (pt-BR, `{name}` is the pet's name):

| Kind | When | Prio | Fallback |
|---|---|---|---|
| `needs_you` | Any session enters `needs_you` | 3, sticky | "Ei! Precisam de você." |
| `error` | The focused session enters `error` | 3 | "Ih… deu erro." |
| `pr_fix` | A PR needs a fix (from `prs.mine`) | 2 | "Tem PR pedindo conserto." |
| `pr_approved` | A PR got approved | 2 | "PR aprovado! Bora dar merge?" |
| `level_up` | Level up after a turn | 2 | "Subi de nível!" |
| `egg` | A new egg after a turn | 2 | "Achei um ovo!" |
| `hatch` | Hatched (click) | 2 | "Oi! Eu sou {name}." |
| `pet` | Pet (click) | 1 | "Hehe ♥" |
| `feed` | Fed (click) | 1 | "Nham!" |
| `full` | Fed while full (click) | 1 | "Tô cheio." |
| `turn_done` | A turn ends normally | 1, chatter | "Pronto!" |
| `hungry` | Hunger crosses 60 | 1, chatter | "Tô com fome…" |
| `lonely` | Affection drops under 25 | 1, chatter | "Cadê você?" |
| `wake` | The first event after sleeping | 1, chatter | "Hm? Acordei." |

## The extension: the agent

**Persona.** It is built from the pet, as Claude's system prompt. Each stat at 7 or more adds its high trait, and at 3 or less its low trait:

| Stat | High | Low |
|---|---|---|
| Focus | terse, on-task | easily distracted, drifts into random thoughts |
| Calm | soothing, unbothered even by errors | dramatic: small things are big events |
| Chaos | mischievous: puns, jokes, playful denial ("Quebrou tudo. Não fui eu.") | orderly, likes things tidy |
| Charm | affectionate and warm | grumpy and sarcastic, never mean |

Then:

- **Species and rarity.** "You are a {species}, a {rarity} creature from Brazilian fauna or folklore; let its nature show." Epic adds "proud"; Legendary adds "grandiose, speaks of itself as a legend".
- **Shiny.** It adds "a little vain about your shine".
- **Level.**
  - Levels 1–2: baby talk, 1 to 3 words.
  - Levels 3–5: short simple sentences.
  - Level 6 and up: full short sentences that may recall the diary.
- **Rules.**
  - Brazilian Portuguese.
  - At most 60 characters per line.
  - No emoji but ♥, and no quotes.
  - Never mention being an AI, never give coding advice.
  - Keep it friendly.

**The request.**

- **Prompt.** The user message carries the pet's name, species, level, stats, its mood now, and the last 15 diary notes with dates. It asks for 6 lines for each event kind in the table, each with its one-line meaning.
- **Output.** A JSON schema returns an object with one array of strings per kind.
- **Validation.** Lines over 80 characters or empty are dropped. A kind left with no lines falls back to the fixed lines.

**The call.** The extension spawns the CLI headless, with the data folder as working directory and a 90 s timeout:

```
claude -p --safe-mode --model claude-opus-5-5 --effort low --output-format json
  --json-schema <schema> --system-prompt <persona> --tools "" --no-session-persistence
  --max-budget-usd 0.10 <prompt>
```

- `--safe-mode` keeps out CLAUDE.md, skills, plugins, hooks, MCP servers and custom commands, while keeping the user's login.
- `--bare` would do the same, but it needs an API key.
- The result is read from `structured_output` when present, else by decoding `result` as JSON.
- `claude` is found on `PATH`, else in `~/.local/bin/claude`. Without it there is no agent and only the fixed lines are used.

**When.** A frame starts one generation, in the background, when:

- there is a pet;
- `claude` is found;
- none is running;
- the last failure was over 30 minutes ago;
- and either the pools belong to another pet (another `Born`), or were written at another level, or are over 6 hours old.

The result comes back to the serve loop on a channel, so all state stays on one goroutine.

**Memory.** `pet.json` gains:

| Field | Holds |
|---|---|
| `lines` | The pools, kind → lines |
| `lines_born` | `Born` of the pet the pools belong to |
| `lines_level` | The level they were written at |
| `lines_at` | When they were written |
| `diary` | Notes `{at, text}`, the last 30, in English |

A note is added on hatch, level up, a new egg, a PR approved, a PR to fix, a feed while starving, and on turning lonely. Hatching a new pet keeps the diary (the new pet "inherits the house") but empties the pools.

## Calls made without the owner (check these)

- **Placement A.** The owner said "Perfeito" to the mockup without picking one explicitly.
- **Model.** Claude Opus 5.5 (`claude-opus-5-5`) at low effort, the current default. Haiku 5.5 (`claude-haiku-5-5`) is cheaper and a one-line change.
- **Lines in pt-BR, always,** to match the Brazilian pets and the mockup. The status word stays English.
- **Privacy.** What goes to Claude is the pet's data and the diary only. The README says so.
- **Pop.** A fade with a rise instead of a scale, because GPUI cannot scale a `div`.

## Left to the builder

- Exact colours: the bubble background is the theme's popover colour, and the border uses the theme's border or the tone colour.
- Padding, text size (`text_xs`) and corner radius, following the panel's existing cards.
- The diary's English wording.
- The persona's exact sentences, within the table above.
