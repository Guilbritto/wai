# Remote access Specification

**Approved**: 2026-11-20 - user: "pode seguir"

## Problem Statement

wai's sessions only run and answer from the computer where wai is open. Once the owner walks away, a session that asks for permission or finishes a turn waits until they get back, and nothing can be sent to it. This feature lets the owner continue development from their phone: follow the sessions, send prompts, answer permission requests and cancel turns. A Flutter app talks to the `wai-remoto` extension through a Tailscale tunnel.

## Goals

- [ ] From an iPhone or an Android phone, the owner reads a session and sends it a prompt, while the computer stays where it is.
- [ ] A permission request answered on the phone unblocks the session in wai within 2 seconds.
- [ ] Nothing in wai can be reached from a network address without a paired device's token.

## Out of Scope

| Feature | Reason |
| ------- | ------ |
| Starting a new session from the phone | Not chosen for v1 (context.md, Scope) |
| Viewing files or diffs on the phone | Not chosen for v1 |
| A remote terminal | Not chosen for v1; it is the highest-risk capability |
| Listening on the LAN or through an internet relay | Loopback plus `tailscale serve` only (context.md, Network) |
| Push notifications to the phone | Out of v1 (context.md, Push notifications) |
| A browser or PWA client | The client is the Flutter app (context.md, Client) |
| Publishing to the App Store or Google Play | v1 installs from a local build |
| Installing or configuring Tailscale | The user runs `tailscale serve`; wai-remoto only names the command |
| Creating the `wai-mobile` GitHub repository | Only when the user asks |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --------------------- | -------------- | --------- | ---------- |
| v1 capabilities | List sessions, read a transcript, follow a turn live, send a prompt, answer a permission request, cancel a turn | User picked "Sessões (DESIGN.md)" | y |
| Network | wai-remoto binds `127.0.0.1` only; the phone arrives through `tailscale serve` | User picked "Localhost + túnel" and "Tailscale serve" | y |
| Client | A Flutter app for iOS and Android in `C:\Users\guilh\Project\wai-mobile` | User picked "App nativo / multiplataforma", "Flutter", "iOS e Android", "wai-mobile" | y |
| Delivery | Host API, wai-remoto and the app, end to end | User picked "Ponta a ponta" | y |
| Pairing | QR code with the tunnel URL and a one-time code; the app gets a per-device token | User picked "QR code + token" | y |
| Token lifetime | Expires after 30 days without use; each authenticated request renews it | User picked "30 dias, renova com uso" | y |
| Presence in wai | The wai-remoto panel shows the number of connected devices | User picked "Indicador discreto" | y |
| Push notifications | Out of v1 | User picked "Fora da v1" | y |
| One-time pairing code | 128 random bits, valid 5 minutes, single use; a new code replaces the previous one | Long enough that guessing is not a threat, so no per-address limit is needed behind `tailscale serve`, where every request comes from `127.0.0.1` | y - approved with the spec |
| Default port | `7717`, editable in the panel, 1024-65535 | Any free unprivileged port works; the user changes it on a clash | y - approved with the spec |
| Token storage | wai-remoto stores only a SHA-256 hash of each token, in its data folder (`0600` file); the app keeps the token in the Keychain / Keystore | DESIGN.md: credentials never stored in clear; a copy of the data file pairs nothing | y - approved with the spec |
| Live updates | Coalesced to at most 10 `event/session_update` per second per session | DESIGN.md asks for coalescing; 10/s follows a stream without flooding a phone link | y - approved with the spec |
| Limits | Request bodies 64 KB; 30 prompts per minute per device; 4 open event streams per device | Numbers from DESIGN.md; the transport is HTTP + Server-Sent Events (user picked "HTTP + SSE" at design), so DESIGN.md's WebSocket limits apply to request bodies and open streams | y - approved with the spec |
| Failed authentication | After 20 rejected tokens or pairing codes in one hour, wai-remoto sends one `notify` to the user | DESIGN.md's lockout notification, without locking out the owner's own paired phone | y - approved with the spec |
| Host API version | The new methods, events and the `listen` permission ship as extension API 1.10 | The API is 1.9 today (`crates/wai-ext/src/manifest.rs`) | y - approved with the spec |
| Agent questions (question cards) | Answered from the phone in this delivery, as P2 story "Answer an agent's question" | User: "vamos incluir na entrega de agora" | y |
| Permission for conversation content | A new permission `sessions:content` covers `sessions/read`, `event/session_update` and the permission and question events; `sessions:read` keeps meaning session states only | Extensions already granted `sessions:read` (wai-bichinho) do not gain transcripts without new consent. User picked option A at design | y |

**Open questions:** none - all resolved or logged above.

---

## User Stories

### P1: Pair a phone ⭐ MVP

**User Story**: As the owner, I want to pair my phone with wai by scanning a QR code so that only my devices can reach my sessions.

**Why P1**: Nothing else works without a paired, authenticated device.

**Acceptance Criteria**:

1. WHEN the user installs wai-remoto THEN wai SHALL list `listen` in the consent as "Listen for connections on a network port" (PAIR-01)
2. The wai-remoto panel SHALL show a port field, a tunnel URL field, a Start button and the command `tailscale serve --bg <port>` for the current port (PAIR-02)
3. IF the user starts with a port outside 1024-65535 THEN the panel SHALL show "Port must be between 1024 and 65535" and SHALL NOT listen (PAIR-03)
4. IF the user starts with a tunnel URL that does not begin with `https://` THEN the panel SHALL show "The tunnel URL must start with https://" and SHALL NOT listen (PAIR-04)
5. WHEN the user clicks Start with a valid port and tunnel URL THEN wai-remoto SHALL listen on `127.0.0.1:<port>` only and the panel SHALL show "Listening on 127.0.0.1:<port>" and a Stop button (PAIR-05)
6. IF the port is already in use THEN the panel SHALL show "Port <port> is in use" and SHALL keep the Start button (PAIR-06)
7. WHILE wai-remoto is not started it SHALL NOT listen on any port (PAIR-07)
8. WHEN the user clicks "Pair a device" THEN the panel SHALL show a QR code encoding the tunnel URL and a new one-time code, and the time left before it expires (PAIR-08)
9. WHEN the app sends a valid, unexpired, unused pairing code and a device name THEN wai-remoto SHALL answer with a new device token and SHALL mark the code used (PAIR-09)
10. IF the app sends a pairing code that is unknown, expired or already used THEN wai-remoto SHALL answer 401 and issue no token (PAIR-10)
11. WHEN a device pairs THEN the panel SHALL list it with its name and pairing date (PAIR-11)
12. WHEN the user clicks Stop THEN wai-remoto SHALL close the listener and every live connection within 1 second (PAIR-12)
13. WHEN the app is opened for the first time THEN it SHALL show the pairing screen with a "Scan QR code" button (PAIR-13)
14. WHEN the app scans a wai-remoto QR code and pairing succeeds THEN it SHALL store the URL and token in the platform's secure storage and show the session list (PAIR-14)
15. IF pairing fails THEN the app SHALL show "Pairing failed: scan a new code" and stay on the pairing screen (PAIR-15)

**Independent Test**: Start wai-remoto, pair the app through the QR code, and see the device in the panel; a reused code is refused.

---

### P1: Follow sessions ⭐ MVP

**User Story**: As the owner, I want to see my sessions and follow a turn as it runs so that I know what the agents are doing while I am away.

**Why P1**: Reading is the base of every remote action.

**Acceptance Criteria**:

1. WHEN an extension granted `sessions:content` calls `sessions/read` with an open session THEN wai SHALL return that session's transcript: user messages, agent messages and tool calls with their title and status, in order (HOST-01)
2. WHILE a session has a turn running wai SHALL send `event/session_update` with the new content to extensions granted `sessions:content`, at most 10 per second per session (HOST-02)
3. IF `sessions/read` names a session that is not open THEN wai SHALL answer with an error naming the session (HOST-03)
4. WHEN the app opens the session list THEN it SHALL show every open session with its title (or the agent's name when untitled), its agent and its state (idle, working, needs you, error) (REMO-01)
5. WHEN a session's state changes in wai THEN the app's session list SHALL show the new state within 2 seconds (REMO-02)
6. WHEN the app opens a session THEN it SHALL show the session's transcript, with agent output rendered as text, never as HTML (REMO-03)
7. WHILE an opened session's turn is running the app SHALL append its new content as it streams (REMO-04)
8. WHEN no session is open in wai THEN the app SHALL show "No open sessions in wai" (REMO-05)
9. IF the app cannot reach wai-remoto THEN it SHALL show "Can't reach wai. Check that wai-remoto is started and Tailscale is connected" with a Retry button (REMO-06)
10. IF wai-remoto answers 401 to the app's token THEN the app SHALL delete the stored token and show the pairing screen (REMO-07)

**Independent Test**: With a turn running in wai, open the session on the phone and watch new text arrive; stop wai-remoto and see the unreachable message.

---

### P1: Drive a session ⭐ MVP

**User Story**: As the owner, I want to send a prompt, cancel a turn and answer permission requests from my phone so that sessions keep moving while I am away.

**Why P1**: Without it remote access is read-only, which is not continuing development.

**Acceptance Criteria**:

1. WHEN an agent asks for permission THEN wai SHALL send `event/permission_requested` with the session, a request id, the tool call's title and the options, to extensions granted `sessions:content` (HOST-04)
2. WHEN an extension granted `sessions:control` calls `permission/answer` with a pending request id and one of its options THEN wai SHALL answer the agent with that option and close the request's card in wai as answered (HOST-05)
3. WHEN a permission request is answered, in wai or by an extension THEN wai SHALL send `event/permission_resolved` with the request id and the chosen option (HOST-06)
4. IF `permission/answer` names a request that is already answered or unknown THEN wai SHALL answer with an error and SHALL NOT change the agent's answer (HOST-07)
5. WHEN an extension granted `sessions:control` calls `sessions/cancel` with a session whose turn is running THEN wai SHALL cancel the turn as wai's own stop does (HOST-08)
6. IF an extension without the permission a method needs calls it THEN wai SHALL answer `-32001`, `Permission denied: <permission>` (HOST-09)
7. IF an extension is granted `sessions:read` but not `sessions:content` THEN wai SHALL NOT send it `event/session_update`, `event/permission_requested`, `event/permission_resolved`, `event/question_requested` or `event/question_resolved` (HOST-10)
8. WHEN the user installs an extension that asks for `sessions:content` THEN wai SHALL list it in the consent as "Read your sessions' conversations" (HOST-11)
7. WHEN the user sends a prompt from the app to an idle session THEN wai SHALL run a turn with that text, shown in wai as a user message (REMO-08)
8. WHILE the opened session is working the app SHALL disable sending and show a Cancel button (REMO-09)
9. WHEN the user taps Cancel THEN the app SHALL call cancel and the session SHALL end its turn with reason `cancelled` (REMO-10)
10. WHEN a permission request is pending in a session THEN the app SHALL show the tool call's title and one button per option (REMO-11)
11. WHEN the user taps a permission option in the app THEN the session SHALL continue with that answer within 2 seconds (REMO-12)
12. WHEN a permission request the app shows is answered in wai THEN the app SHALL replace its buttons with "Answered in wai: <option>" (REMO-13)
13. IF a device sends more than 30 prompts in one minute THEN wai-remoto SHALL answer 429 to the extra prompts and the app SHALL show "Too many prompts, wait a minute" (REMO-14)

**Independent Test**: Trigger a tool call that asks permission in wai, allow it from the phone, and see the session continue; send a prompt and cancel it.

---

### P2: Answer an agent's question

**User Story**: As the owner, I want to answer an agent's question from my phone so that a turn waiting on my answer keeps going while I am away.

**Why P2**: A turn stopped on a question blocks remote work like a permission request does; the user included it in this delivery.

**Acceptance Criteria**:

1. WHEN an agent asks a question THEN wai SHALL send `event/question_requested` with the session, a request id, the message and each field's key, title, description, required flag, kind and choices, to extensions granted `sessions:content` (QST-01)
2. WHEN an extension granted `sessions:control` calls `question/answer` with a pending request id and `accept` with values by field key, `decline` or `cancel` THEN wai SHALL answer the agent with it and close the question's card in wai as answered (QST-02)
3. WHEN a question is answered, in wai or by an extension THEN wai SHALL send `event/question_resolved` with the request id and the answer kind (`accept`, `decline` or `cancel`) (QST-03)
4. IF `question/answer` names a request that is already answered or unknown THEN wai SHALL answer with an error and SHALL NOT change the agent's answer (QST-04)
5. IF `question/answer` sends `accept` without a value for a required field, or a value that does not fit the field's kind or choices THEN wai SHALL answer with an error naming the field and keep the question pending (QST-05)
6. WHEN a question is pending in a session THEN the app SHALL show its message and one control per field: a single-choice list, a multiple-choice list, a text box, a switch or a number box, with Submit, Decline and Cancel buttons (QST-06)
7. WHILE a required field has no value the app SHALL disable Submit (QST-07)
8. WHEN the user taps Submit in the app THEN the session SHALL continue with the entered values within 2 seconds (QST-08)
9. WHEN the user taps Decline or Cancel in the app THEN wai SHALL answer the agent with `decline` or `cancel` (QST-09)
10. WHEN a question the app shows is answered in wai THEN the app SHALL replace its form with "Answered in wai" (QST-10)

**Independent Test**: Make an agent ask a question with a single-choice and a text field, answer it on the phone, and see the turn continue with those values.

---

### P2: Manage devices

**User Story**: As the owner, I want to see and revoke paired devices so that a lost phone stops working.

**Why P2**: The 30-day expiry bounds the risk; revoking closes it at once.

**Acceptance Criteria**:

1. WHILE wai-remoto is started the panel SHALL show "<n> devices connected", counting devices with a live connection (DEV-01)
2. WHEN the user clicks Revoke on a device THEN wai-remoto SHALL delete its token, close its connections within 1 second and remove it from the list (DEV-02)
3. WHEN the user clicks "Sign out all devices" THEN wai-remoto SHALL delete every token and close every connection within 1 second (DEV-03)
4. IF a device's token has not been used for 30 days THEN wai-remoto SHALL answer 401 to it and remove the device from the list (DEV-04)
5. WHEN a token authenticates a request THEN wai-remoto SHALL restart that token's 30-day expiry (DEV-05)
6. WHEN the app's user taps "Unpair" THEN the app SHALL ask wai-remoto to revoke its token, delete it locally and show the pairing screen (DEV-06)

**Independent Test**: Revoke the phone in the panel; the app returns to the pairing screen on its next request.

---

## Edge Cases

- IF a request carries an `Origin` header THEN wai-remoto SHALL answer 403 (EDGE-01)
- IF a request's `Host` is neither the tunnel URL's host nor `127.0.0.1:<port>` THEN wai-remoto SHALL answer 421 (EDGE-02)
- IF a request has no token or an unknown one THEN wai-remoto SHALL answer 401 (EDGE-03)
- IF a request body exceeds 64 KB THEN wai-remoto SHALL reject it with 413 (EDGE-04)
- IF a device opens a fifth event stream while four are open THEN wai-remoto SHALL refuse it with 429 (EDGE-05)
- WHEN 20 authentication failures happen within one hour THEN wai-remoto SHALL send one `notify` "wai-remoto: 20 failed sign-ins in the last hour" (EDGE-06)
- IF the user disables or uninstalls wai-remoto THEN wai SHALL stop its process and nothing SHALL listen on its port (EDGE-07)
- WHEN the app sends a prompt to a session that is not idle THEN wai-remoto SHALL answer 409 and the app SHALL show "The session is busy" (EDGE-08)
- The wai-remoto API SHALL answer every response with `Cache-Control: no-store` (EDGE-09)

---

## Implicit-Requirement Dimensions

| Dimension | Coverage |
| --------- | -------- |
| Input validation & bounds | PAIR-03, PAIR-04, EDGE-04, EDGE-05, REMO-14 |
| Failure / partial-failure states | PAIR-06, REMO-06, HOST-03, HOST-07 |
| Idempotency / retry / duplicate handling | PAIR-09/PAIR-10 (single-use code), HOST-07 and QST-04 (first answer wins), EDGE-08 (prompt to a busy session) |
| Auth boundaries & rate limits | PAIR-09, EDGE-01, EDGE-02, EDGE-03, HOST-09, HOST-10, REMO-14, EDGE-06 |
| Concurrency / ordering | HOST-05, HOST-06, HOST-07, REMO-13: wai and the phone race on one permission request |
| Data lifecycle / expiry | PAIR-08 (5-minute code), DEV-04, DEV-05, DEV-02, DEV-03 |
| Observability | EDGE-06; connected-device count DEV-01. Request logging is N/A because wai-remoto must not log tokens or prompts, and the extension log already records errors |
| External-dependency failure | REMO-06 (tunnel or wai-remoto down), REMO-07 (token rejected) |
| State-transition integrity | PAIR-05, PAIR-07, PAIR-12, REMO-09 |

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| -------------- | ----- | ----- | ------ |
| PAIR-01 | P1: Pair a phone | Design | Implementing |
| PAIR-02 | P1: Pair a phone | Design | Pending |
| PAIR-03 | P1: Pair a phone | Design | Pending |
| PAIR-04 | P1: Pair a phone | Design | Pending |
| PAIR-05 | P1: Pair a phone | Design | Pending |
| PAIR-06 | P1: Pair a phone | Design | Pending |
| PAIR-07 | P1: Pair a phone | Design | Pending |
| PAIR-08 | P1: Pair a phone | Design | Implementing |
| PAIR-09 | P1: Pair a phone | Design | Implementing |
| PAIR-10 | P1: Pair a phone | Design | Implementing |
| PAIR-11 | P1: Pair a phone | Design | Pending |
| PAIR-12 | P1: Pair a phone | Design | Pending |
| PAIR-13 | P1: Pair a phone | Design | Pending |
| PAIR-14 | P1: Pair a phone | Design | Pending |
| PAIR-15 | P1: Pair a phone | Design | Pending |
| HOST-01 | P1: Follow sessions | Design | Implementing |
| HOST-02 | P1: Follow sessions | Design | Implementing |
| HOST-03 | P1: Follow sessions | Design | Implementing |
| REMO-01 | P1: Follow sessions | Design | Implementing |
| REMO-02 | P1: Follow sessions | Design | Pending |
| REMO-03 | P1: Follow sessions | Design | Pending |
| REMO-04 | P1: Follow sessions | Design | Pending |
| REMO-05 | P1: Follow sessions | Design | Pending |
| REMO-06 | P1: Follow sessions | Design | Pending |
| REMO-07 | P1: Follow sessions | Design | Pending |
| HOST-04 | P1: Drive a session | Design | Implementing |
| HOST-05 | P1: Drive a session | Design | Implementing |
| HOST-06 | P1: Drive a session | Design | Implementing |
| HOST-07 | P1: Drive a session | Design | Implementing |
| HOST-08 | P1: Drive a session | Design | Implementing |
| HOST-09 | P1: Drive a session | Design | Implementing |
| HOST-10 | P1: Drive a session | Design | Pending |
| HOST-11 | P1: Drive a session | Design | Implementing |
| REMO-08 | P1: Drive a session | Design | Implementing |
| REMO-09 | P1: Drive a session | Design | Pending |
| REMO-10 | P1: Drive a session | Design | Implementing |
| REMO-11 | P1: Drive a session | Design | Pending |
| REMO-12 | P1: Drive a session | Design | Implementing |
| REMO-13 | P1: Drive a session | Design | Pending |
| REMO-14 | P1: Drive a session | Design | Implementing |
| QST-01 | P2: Answer an agent's question | Design | Implementing |
| QST-02 | P2: Answer an agent's question | Design | Implementing |
| QST-03 | P2: Answer an agent's question | Design | Implementing |
| QST-04 | P2: Answer an agent's question | Design | Implementing |
| QST-05 | P2: Answer an agent's question | Design | Implementing |
| QST-06 | P2: Answer an agent's question | Design | Pending |
| QST-07 | P2: Answer an agent's question | Design | Pending |
| QST-08 | P2: Answer an agent's question | Design | Implementing |
| QST-09 | P2: Answer an agent's question | Design | Pending |
| QST-10 | P2: Answer an agent's question | Design | Pending |
| DEV-01 | P2: Manage devices | Design | Pending |
| DEV-02 | P2: Manage devices | Design | Implementing |
| DEV-03 | P2: Manage devices | Design | Implementing |
| DEV-04 | P2: Manage devices | Design | Implementing |
| DEV-05 | P2: Manage devices | Design | Implementing |
| DEV-06 | P2: Manage devices | Design | Implementing |
| EDGE-01 | Edge cases | Design | Implementing |
| EDGE-02 | Edge cases | Design | Implementing |
| EDGE-03 | Edge cases | Design | Implementing |
| EDGE-04 | Edge cases | Design | Implementing |
| EDGE-05 | Edge cases | Design | Pending |
| EDGE-06 | Edge cases | Design | Implementing |
| EDGE-07 | Edge cases | Design | Pending |
| EDGE-08 | Edge cases | Design | Implementing |
| EDGE-09 | Edge cases | Design | Implementing |

**Coverage:** 65 total, 0 mapped to tasks, 65 unmapped ⚠️

---

## Sources

- `extensions/wai-remoto/DESIGN.md` - security model, threat model, flows, and the host additions this spec turns into HOST-01..HOST-11. Its cookie, `SameSite` and Origin rules are replaced by device tokens, because the client is a native app.
- `docs/extensions.md` - the current extension API (1.9), permissions and data folder.
- `.specs/features/remote-access/context.md` - the user's decisions.

---

## Success Criteria

- [ ] With wai on the computer and the phone on mobile data through Tailscale, the owner pairs the phone in under 1 minute.
- [ ] A permission request answered from the phone unblocks the session in wai within 2 seconds.
- [ ] A revoked or 30-day-idle device gets 401 on every request.
