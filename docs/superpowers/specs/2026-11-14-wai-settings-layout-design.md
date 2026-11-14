# Settings: independent scroll and aligned rows

Approved 2026-11-14 from the mockup `.superpowers/brainstorm/36090-1794656036/content/settings-v1.html`.

## Why

Scrolling the Settings modal drags the whole screen, nav included. The rows are hard to read: on a wide modal the label sits 1000 px from its control, and each kind of row (agent, version, model, advanced) lines up differently.

## Cause of the scroll

Two things. The gpui-component `Dialog` puts `.child()` content in a scroll area as tall as the content, so the whole modal scrolled. And the row holding the nav and the page is an `h_flex`, which centers its children: each was as tall as its content, so their own `overflow_y_scroll` never had anything to scroll.

## Design

- The view goes in the dialog's `content` slot (as tall as the dialog, no scroll), and the nav/page row stretches its children (`items_stretch`), so the nav and the page scroll on their own.
- Content: a sticky page header (title, description, actions, search) above a scrolling body; both use one column of at most 760 px, centered.
- Rows: one grid everywhere. Label and optional description on the left, the control in a fixed 240 px column on the right, controls end on the same edge.
- Rows are grouped in cards (border, glass fill, dividers between rows), with an optional uppercase section label above.
- Agent: a card. Header row: radio, name, Default pill, adapter line under the name; status and Test on the right. Under it, indented sub-rows on the same grid: warning, install hint, Default model, Advanced (folded) and Command.
- Version: the status line goes in the row's description; the button(s) take the control column.
- Nav: group label aligned with the items' text, counts in a right column, "Soon" as small amber text; right border and a faint fill.
- Short descriptions under rows that have none today (Theme, Processing, Shell, Automatic updates, Diagnostics, Repository).

## Approaches

1. **Restyle in place** (chosen): keep `SettingsView`'s structure, fix the root, add a shared row/card helper, re-lay the header. Smallest change; tests keep their selectors.
2. Replace the modal with a full window or pane: more room, but a different interaction and much more work; not asked for.

## Non-goals

No new settings, no global search across sections, no change to what each section shows or emits.

## Left to the builder

Exact spacing and colors follow `aurora` helpers; light theme uses the theme tokens as today.
