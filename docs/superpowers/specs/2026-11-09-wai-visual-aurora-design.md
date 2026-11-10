# wai visual review: "Aurora" (based on the Central de IA)

> 2026-11-09. The owner asked for a visual review of the IDE, taking the Central de IA WayCarbon's look as the base ("esse design é bonito"), and skipped the design questions. Every choice below is the builder's call unless marked as the owner's.

## Intent

Today wai uses gpui-component's default dark theme: flat black, square edges, white buttons. The Central de IA (`central-ia-waycarbon-claude/central/www/app.css`, identity "aurora viva") looks the way the owner wants:
- an indigo night background with a soft aurora (teal, violet, pink);
- dark glass cards with 16–22 px corners and a faint violet glow;
- pill chips, a status pill in the top bar, a rounded display font;
- violet for "active" and teal for "go/working".

wai gets that identity natively (GPUI), without becoming a web page and without giving up its performance goals.

Reference screenshots: `central-ia-waycarbon-claude/docs/img/{escritorio-desktop,chat-celular,permissao-celular}.png`, `macos-state/lado-desktop.png`.

## Goals

1. **Aurora theme**: a gpui-component theme "wai Aurora" built from the Central palette becomes the dark theme (the default).
2. **Backdrop**: a static aurora behind everything: a dark base gradient plus three soft color blobs.
3. **Cards**: sidebar, each pane and the terminal float as rounded dark-glass cards separated by a small gap, instead of panels split by 1 px lines.
4. **Top bar**: brand, project, a status pill counting working agents, and the global actions, in the macOS title bar area (traffic lights inline).
5. **Components**:
   - segmented sidebar tabs;
   - pill editor tabs;
   - chat bubbles (violet for you, glass for the agent, with a "✦ <agent>" header);
   - permission and question cards in the Central's style;
   - a pill prompt box with a round send button;
   - pill notices.
6. **Display font**: a rounded font for the brand and titles, bundled so macOS and Windows look the same.

## Non-goals

- Animation: no aurora drift, no pulsing dots, no 3D office or characters. Idle CPU must stay < 0.5%.
- Translating the UI to Portuguese; the strings stay as they are, apart from the few new ones below.
- An Aurora light theme. Light mode keeps gpui-component's default light theme and simply gets no backdrop and plain cards.
- User theme files or a theme picker.
- CPU/RAM pill (the Central's), mobile layout, the Central's key strip (Esc/Tab/arrows).

## Anti-goals (a failure even if it works)

- Idle CPU above 0.5% or streaming CPU above 10% (Phase 1–4 targets).
- Light mode becoming unreadable (dark-only colors leaking into light).
- Losing window dragging, double-click zoom or the traffic lights on macOS.
- Breaking any existing test id (`tab-*`, `sidebar-*`, `new-session`, `send`, `stop`, permission/question button ids).

## Palette (from the Central's `:root`)

| Token | Value | Use in wai |
|---|---|---|
| noite | `#070a18` | window background |
| noite-2 | `#0d1230` | popovers, dialogs, menus |
| noite-3 | `#141a3d` | secondary surfaces |
| boreal | `#36e2c0` | primary, focus ring, "working" dot, send button |
| lilas | `#8d72ff` | active tab, selection, focused card, user bubble |
| magenta | `#ff6ec7` | aurora blob, syntax accents |
| ceu | `#5bb8ff` | info, links |
| brasa | `#ff8f4f` | agent header star, eyebrows, numbered badges |
| ok | `#46e39a` | success |
| ambar | `#ffc15a` | warning, "needs you" |
| erro | `#ff6b8b` | danger |
| tinta / tinta-2 | `#eceffe` / `#c4cbeb` | text / secondary text |
| nevoa / nevoa-2 | `#8f99c2` / `#626c98` | muted text / faint text |
| vidro / vidro-forte | white 5.5% / 9% | hover, chips, inputs |
| borda / borda-forte | white 10% / 16% | borders |
| card | `rgba(8,11,28,0.84)` | card fill (`.painel-cartao`) |

The shapes and effects come from the Central's CSS:
- cards: radius 22 in the Central, 16 in wai for density; 1 px `borda`; shadow `0 40 80 -40 black .95` and glow `0 0 60 -30 lilas .45`;
- card bar: top gradient white 5% → 1.5% and bottom line white 6%;
- active tab: lilas 16% fill, lilas 45% border, glow;
- primary button: boreal (the Central uses a gradient);
- chips and pills: fully rounded, `vidro` fill, `borda` border.

## Decisions (builder's calls)

- **Theme mechanism:** a gpui-component `ThemeSet` JSON (`crates/wai/assets/theme/aurora.json`, embedded) registered in the `ThemeRegistry` and set as `Theme::dark_theme`. Every view that already reads theme tokens picks it up for free. Only colors the theme has no slot for (brasa, lilas tints, card fill, glow) live as constants in a new `aurora.rs`.
- **Backdrop without animation:** GPUI has linear gradients and blurred box shadows but no radial gradients. The blobs are divs drawn only through a large blurred shadow. If that shows banding or clipping, fall back to one pre-rendered PNG of at most 300 KB. It is static, so idle CPU is unaffected.
- **Title bar:** on macOS, gpui-component's `TitleBar` (transparent title bar, inline traffic lights, drag and double-click handled). On Windows and Linux, the native title bar stays and the top bar is an ordinary row below it. Windows has never run wai, so this avoids custom-drawn caption buttons there.
- **Display font:** Nunito Bold (OFL 1.1, the rounded face the Central's own font stack lists), one static weight, embedded and registered at startup. The body text stays the system font and the mono stays as is.
- **Sidebar header buttons move to the top bar:** templates, extensions and new session, keeping their element ids. The project name moves there too, to avoid showing it twice.
- **Terminal colors:** `DEFAULT_FG`/`DEFAULT_BG` and the 16 ANSI colors are retuned to the palette. The terminal stays dark in light mode, as today.
- **New strings:**
  - "No agent working" / "1 agent working" / "N agents working", plus " · N needs you" / " · N need you";
  - "No messages yet.";
  - "PERMISSION REQUESTED" and "QUESTION" eyebrows;
  - "Start a session".

## Left to the builder

Exact paddings and font sizes within the ranges above; how the 1 px resize handle line is hidden (it must not show at rest; it may show violet while hovered or dragged); syntax highlight colors (keywords lilas, strings boreal, numbers ambar, functions ceu, types `#cbbcff`, comments nevoa-2 is the starting point).
