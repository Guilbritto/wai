# Escritório (delivery 1: map, avatars, presence) Specification

**Approved**: 2026-11-26 - user: "aprovado"

## Problem Statement

The team works remotely and has no sense of who is around, who is free and who is deep in work. Gather solves this with a 2D office where avatars walk and talk by proximity, but it lives in a browser tab apart from where the work happens. wai is where the team's agents run, so the office belongs inside wai, showing each person's agent activity on their avatar.

This is delivery 1 of 4 (see `context.md`, Slicing): a shared map with walking avatars, a self-hosted Go server, a presence panel and agent status. Audio/video, rooms and desks, and chat follow in their own specs.

## Goals

- [ ] Up to 50 teammates see each other on one office map inside wai, and see a move within 200 ms on the same network.
- [ ] Anyone can see at a glance who is online, who is near them and whose agents are working, without leaving wai.
- [ ] Any wai extension can draw a colour scene and read the keyboard through the public extension API.

## Out of Scope

| Feature | Reason |
| ------- | ------ |
| Proximity audio/video, mic and camera | Delivery 2 (`escritorio-av`) |
| Private rooms and desks | Delivery 3 (`escritorio-salas`) |
| Text chat, manual status (available/busy/away) | Delivery 4 (`escritorio-chat`) |
| Map editor; more than one map | One built-in map covers a team; an editor is its own feature |
| Click-to-walk and pathfinding | Keyboard walking covers moving; add when asked |
| Screen sharing | Not asked |
| GitHub/Google login | Token chosen for now; OFC-27 keeps the swap to one place |
| The pet following the avatar | Offered, not picked (Deferred Ideas) |
| A browser or phone client | The office is native in wai (user: "Nativo dentro do wai") |
| Hosting the server for the team | The team runs the binary; this delivers the binary and its README |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --------------------- | -------------- | --------- | ---------- |
| Where it runs | Native in wai, through new extension API capabilities | User: "Nativo dentro do wai", "Extensão + API nova" | y |
| Server | Self-hosted Go server, in this repo next to the extension | User: "Servidor Go próprio" | y |
| Joining | Name + server address + team token, no external account | User: "Nome + servidor + token" | y |
| Slicing | Four deliveries; this spec is delivery 1 | User: "4 entregas em sequência" | y |
| Agent status and presence panel are in delivery 1 | Both here | User picked "Status pelos agentes" and "Painel de presença" | y |
| Extension id and name | `wai-escritorio`, "Escritório" | First-party extensions use Portuguese names (bichinho, rotinas, navegador) | y - approved with the spec |
| Extension UI language | English | wai's own UI and messages are English | y - approved with the spec |
| The map | One built-in office map of original pixel art, about 40 × 30 tiles, shipped with the server | Gather's art is not copied; one map covers a team | y - approved with the spec |
| Avatars | A pick of 8 original pixel-art avatars at setup | Gather lets you pick a look; 8 is enough to tell people apart | y - approved with the spec |
| Movement | Arrow keys and WASD, one tile per press, repeating while held | Gather's keyboard movement | y - approved with the spec |
| People overlapping | Avatars may share a tile; only walls and furniture block | Gather lets avatars pass through each other | y - approved with the spec |
| "Nearby" radius | Within 3 tiles (Chebyshev distance) | Gather's default bubble is a few tiles; delivery 2 reuses it for audio/video | y - approved with the spec |
| Where the office opens | A `main` pane beside the sessions | Same place as other main panels; the map needs room | y - approved with the spec |
| Where the presence panel lives | A sidebar tab | Visible next to the sessions list | y - approved with the spec |
| Last position | Kept in the server's memory only; a server restart puts everyone at the spawn point | No database for delivery 1 | y - approved with the spec |
| Agent status shared | Only "Focused" or "Needs you", never session titles, prompts or agent names | Teammates should not see what you are working on | y - approved with the spec |
| Same name twice | The newest connection wins; the older one is closed with "Signed in elsewhere" | A second machine or a restart must not lock you out | y - approved with the spec |
| Capacity | 50 people per server | Delivery 2's peer-to-peer audio connects only nearby people, so 50 fits | y - approved with the spec |
| Transport | WebSocket; plain only on loopback, TLS (cert and key in the server's config) elsewhere | The token must not cross a network in clear text | y - approved with the spec |

**Open questions:** none - all resolved or logged above (required before the spec is confirmed).

---

## User Stories

### P1: Enter the office and walk ⭐ MVP

**User Story**: As a teammate, I want to open the office in wai and walk my avatar around the map so that the team sees where I am.

**Why P1**: Without the map and walking there is no office.

**Acceptance Criteria**:

1. WHEN the user opens the office with no saved setup THEN the extension SHALL show a form asking for name, server address, team token and one of 8 avatars, and SHALL NOT connect before the form is submitted.  <!-- OFC-01 -->
2. IF the submitted name is empty or longer than 32 characters THEN the extension SHALL keep the form and show "Name must be 1 to 32 characters".  <!-- OFC-02 -->
3. IF the server refuses the team token THEN the extension SHALL keep the form and show "The server refused the team token".  <!-- OFC-03 -->
4. IF the server cannot be reached THEN the extension SHALL show "Cannot reach <address>" and retry every 5 seconds while the extension runs.  <!-- OFC-04 -->
5. WHEN the extension connects THEN the office pane SHALL show the map with every connected person's avatar and name label at their position.  <!-- OFC-05 -->
6. WHILE the office pane has the keyboard focus, WHEN the user presses an arrow key or W, A, S or D THEN the user's avatar SHALL move one tile in that direction.  <!-- OFC-06 -->
7. IF the tile in that direction is a wall, furniture or outside the map THEN the avatar SHALL stay on its tile.  <!-- OFC-07 -->
8. WHEN a person moves THEN every other connected client SHALL show the new position within 200 ms on the same network.  <!-- OFC-08 -->
9. WHEN a person connects THEN their avatar SHALL appear at their last position on this server run, or at the map's spawn point when there is none.  <!-- OFC-09 -->
10. WHEN a person's connection ends (wai quits, the extension stops, the network drops) THEN their avatar SHALL disappear for everyone else within 10 seconds.  <!-- OFC-10 -->
11. WHEN the user's avatar comes within 3 tiles of the visible area's edge THEN the office pane SHALL scroll the map to keep the avatar in view.  <!-- OFC-11 -->
12. WHEN the same name connects a second time THEN the server SHALL keep the new connection and close the older one, whose extension SHALL show "Signed in elsewhere" and SHALL NOT retry.  <!-- OFC-12 -->
13. The extension SHALL save name, server address, avatar and token in its data folder, with the file readable only by the user (mode 0600).  <!-- OFC-13 -->
14. WHEN the user chooses "Change setup" THEN the extension SHALL disconnect and show the setup form filled with the saved values.  <!-- OFC-14 -->

**Independent Test**: With a server on localhost and two wai processes set up with different names, moving one avatar with the arrow keys moves it on the other's map; quitting one removes its avatar from the other within 10 s.

---

### P1: Run the office server

**User Story**: As the teammate who hosts the office, I want one Go binary with a config file so that the team has a server without other infrastructure.

**Why P1**: The office has no shared state without it.

**Acceptance Criteria**:

1. WHEN the server starts with a config naming a listen address, a team token and an optional TLS certificate and key THEN it SHALL accept office connections on that address.  <!-- OFC-20 -->
2. IF the team token in the config is shorter than 32 characters THEN the server SHALL refuse to start and print "token must be at least 32 characters".  <!-- OFC-21 -->
3. IF the listen address is not a loopback address and the config has no TLS certificate and key THEN the server SHALL refuse to start and print "TLS is required off loopback".  <!-- OFC-22 -->
4. IF a client presents a wrong token THEN the server SHALL close the connection with the refusal OFC-03 shows, and log the client address without the token.  <!-- OFC-23 -->
5. IF a client sends more than 30 messages in one second THEN the server SHALL drop the messages past 30 in that second.  <!-- OFC-24 -->
6. IF a client sends a message larger than 16 KiB THEN the server SHALL close that connection.  <!-- OFC-25 -->
7. IF a 51st person connects THEN the server SHALL refuse them with "The office is full" and the extension SHALL show it.  <!-- OFC-26 -->
8. The server SHALL decide who a connection is in one function, and identify avatars by an id it assigns, never by the token.  <!-- OFC-27 -->
9. IF a client sends a move to a blocked tile or one more than one tile away THEN the server SHALL ignore the move and send that client its current position.  <!-- OFC-28 -->
10. The server SHALL log each connect, disconnect and refusal on one line to stderr, without the token.  <!-- OFC-29 -->

**Independent Test**: Start the server with a short token and see it refuse; start it right and connect with `websocat` using a wrong token, then the right one, and check the log lines.

---

### P1: Scene and keyboard in the extension API

**User Story**: As an extension author, I want to draw a colour tile map with moving sprites in a panel and receive the keys the user presses there so that extensions can build spatial UIs such as the office.

**Why P1**: The office runs native in wai through the public API (user: "Extensão + API nova"); without these the map cannot be drawn or walked.

**Acceptance Criteria**:

1. WHERE an extension declares API 1.10 or later, the system SHALL draw a scene node: a grid of colour tiles and sprites placed on tiles, each sprite with an optional name label and badge.  <!-- OFC-30 -->
2. WHEN an extension renders a scene in which a sprite's tile changed THEN the system SHALL move that sprite to its new tile within 150 ms.  <!-- OFC-31 -->
3. WHILE a panel with a scene has the keyboard focus, WHEN the user presses a key THEN the system SHALL send the extension the panel id, the scene id and the key's name.  <!-- OFC-32 -->
4. WHEN the user clicks a scene THEN the system SHALL give that panel the keyboard focus.  <!-- OFC-33 -->
5. IF a scene is larger than its panel THEN the system SHALL show the part around the tile the extension marks as the view's centre.  <!-- OFC-34 -->
6. IF a scene node breaks the node rules (missing field, too many tiles or sprites) THEN the system SHALL show "Unsupported element" in its place, as for other nodes.  <!-- OFC-35 -->
7. The extension guide (`docs/extensions.md`) SHALL document the scene node, the key event, their limits and API 1.10.  <!-- OFC-36 -->

**Independent Test**: A test extension renders a 10 × 10 scene with one sprite and moves the sprite on each key it receives; the sprite follows the arrow keys while the pane has the focus, and nothing is sent once another pane has it.

---

### P2: Presence panel

**User Story**: As a teammate, I want a sidebar tab listing who is in the office and who is near me so that I know who is around without opening the map.

**Why P2**: The office works without it, but it is how people notice each other while working.

**Acceptance Criteria**:

1. WHILE the extension is connected, the presence panel SHALL list every connected person by name, with a "Nearby" badge on each person within 3 tiles of the user.  <!-- OFC-40 -->
2. WHEN the user clicks "Enter office" in the presence panel THEN the system SHALL open the office pane.  <!-- OFC-41 -->
3. WHILE the extension has no saved setup, the presence panel SHALL show the setup form of OFC-01.  <!-- OFC-42 -->
4. WHILE the extension is not connected, the presence panel SHALL show "Offline" and the reason from OFC-03, OFC-04, OFC-12 or OFC-26.  <!-- OFC-43 -->
5. WHILE the user is the only person connected, the presence panel SHALL show "Nobody else is in the office".  <!-- OFC-44 -->

**Independent Test**: With two clients connected, the panel of each lists the other; walking within 3 tiles adds "Nearby"; stopping the server turns both panels to "Offline".

---

### P2: Agent status on the avatar

**User Story**: As a teammate, I want my avatar to show when my agents are working or waiting for me so that others know when I am focused.

**Why P2**: It is the wai-specific value over Gather, and it needs the map first.

**Acceptance Criteria**:

1. WHILE at least one of the user's wai sessions is `working` and none is `needs_you`, the user's avatar SHALL show a "Focused" badge to everyone.  <!-- OFC-50 -->
2. WHILE at least one of the user's wai sessions is `needs_you`, the user's avatar SHALL show a "Needs you" badge to everyone.  <!-- OFC-51 -->
3. WHILE none of the user's sessions is `working` or `needs_you`, the user's avatar SHALL show no status badge.  <!-- OFC-52 -->
4. The extension SHALL send the server only the derived status (none, Focused, Needs you), never session titles, prompts, agent names or session ids.  <!-- OFC-53 -->
5. WHEN the derived status changes THEN every connected client SHALL show the new badge within 1 second.  <!-- OFC-54 -->

**Independent Test**: Start a turn in a wai session and watch the avatar on a second client gain "Focused", then "Needs you" when the agent asks a permission, then nothing when the turn ends; the server log and traffic carry no session title.

---

## Edge Cases

- IF the server restarts THEN every extension SHALL reconnect within 5 seconds of the server accepting connections again, and everyone SHALL start at the spawn point.  <!-- OFC-60 -->
- IF the map file is missing or has no spawn point THEN the server SHALL refuse to start and print which.  <!-- OFC-61 -->
- WHEN the office pane is narrower than the map THEN it SHALL keep the user's avatar in view (OFC-11, OFC-34).
- IF two people stand on the same tile THEN both avatars SHALL be drawn and both names readable.  <!-- OFC-62 -->
- IF a name holds characters outside printable Unicode (control characters, newlines) THEN the extension SHALL refuse it with the message of OFC-02.  <!-- OFC-63 -->

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| -------------- | ----- | ----- | ------ |
| OFC-01 | P1: Enter the office and walk | Design | Pending |
| OFC-02 | P1: Enter the office and walk | Design | Pending |
| OFC-03 | P1: Enter the office and walk | Design | Pending |
| OFC-04 | P1: Enter the office and walk | Design | Pending |
| OFC-05 | P1: Enter the office and walk | Design | Pending |
| OFC-06 | P1: Enter the office and walk | Design | Pending |
| OFC-07 | P1: Enter the office and walk | Design | Pending |
| OFC-08 | P1: Enter the office and walk | Design | Pending |
| OFC-09 | P1: Enter the office and walk | Design | Pending |
| OFC-10 | P1: Enter the office and walk | Design | Pending |
| OFC-11 | P1: Enter the office and walk | Design | Pending |
| OFC-12 | P1: Enter the office and walk | Design | Pending |
| OFC-13 | P1: Enter the office and walk | Design | Pending |
| OFC-14 | P1: Enter the office and walk | Design | Pending |
| OFC-20 | P1: Run the office server | Design | Pending |
| OFC-21 | P1: Run the office server | Design | Pending |
| OFC-22 | P1: Run the office server | Design | Pending |
| OFC-23 | P1: Run the office server | Design | Pending |
| OFC-24 | P1: Run the office server | Design | Pending |
| OFC-25 | P1: Run the office server | Design | Pending |
| OFC-26 | P1: Run the office server | Design | Pending |
| OFC-27 | P1: Run the office server | Design | Pending |
| OFC-28 | P1: Run the office server | Design | Pending |
| OFC-29 | P1: Run the office server | Design | Pending |
| OFC-30 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-31 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-32 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-33 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-34 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-35 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-36 | P1: Scene and keyboard in the extension API | Design | Pending |
| OFC-40 | P2: Presence panel | Design | Pending |
| OFC-41 | P2: Presence panel | Design | Pending |
| OFC-42 | P2: Presence panel | Design | Pending |
| OFC-43 | P2: Presence panel | Design | Pending |
| OFC-44 | P2: Presence panel | Design | Pending |
| OFC-50 | P2: Agent status on the avatar | Design | Pending |
| OFC-51 | P2: Agent status on the avatar | Design | Pending |
| OFC-52 | P2: Agent status on the avatar | Design | Pending |
| OFC-53 | P2: Agent status on the avatar | Design | Pending |
| OFC-54 | P2: Agent status on the avatar | Design | Pending |
| OFC-60 | Edge cases | Design | Pending |
| OFC-61 | Edge cases | Design | Pending |
| OFC-62 | Edge cases | Design | Pending |
| OFC-63 | Edge cases | Design | Pending |

**Coverage:** 45 total, 0 mapped to tasks, 45 unmapped ⚠️ (Tasks not written yet)

---

## Implicit requirements sweep

| Dimension | Covered by |
| --------- | ---------- |
| Input validation & bounds | OFC-02, OFC-21, OFC-25, OFC-28, OFC-35, OFC-63 |
| Failure / partial-failure states | OFC-04, OFC-10, OFC-43, OFC-60, OFC-61 |
| Idempotency / retry / duplicates | OFC-04 retry, OFC-12 same name twice, OFC-60 reconnect |
| Auth boundaries & rate limits | OFC-03, OFC-21, OFC-22, OFC-23, OFC-24, OFC-26, OFC-27 |
| Concurrency / ordering | OFC-28 (the server is the authority on positions), OFC-62 (shared tiles) |
| Data lifecycle / expiry | OFC-09 (positions live for one server run), OFC-13 (setup file, 0600) |
| Observability | OFC-29 (server log); the extension logs to stderr as every extension does |
| External-dependency failure | OFC-04, OFC-43, OFC-60: the office goes offline, the rest of wai is unaffected |
| State-transition integrity | Connection states: no setup → connecting → online → offline (retrying) or refused (back to the form) or signed in elsewhere (no retry): OFC-01, OFC-03, OFC-04, OFC-12 |

---

## Sources

- `context.md` - the user's answers on where it runs, server, slicing, joining, mic/camera and architecture.
- `docs/extensions.md` - API 1.9: panels draw only one-colour `pixels`, clicks carry no coordinates, panels get no keyboard input; the reason for OFC-30 to OFC-36.
- Gather (gather.town) - non-binding - user: "quero literalmente fazer bem parecido com eles" - mechanics only; art, name and look are original.

---

## Success Criteria

- [ ] Two teammates on different machines see each other walk on the map, with moves arriving within 200 ms on the same network.
- [ ] A teammate whose agent is running shows "Focused" to the others without doing anything.
- [ ] The server refuses to start insecurely (short token, no TLS off loopback).
