# wai-remoto: design

## Goal

From a phone, see wai's sessions, follow a turn as it streams, send a prompt, cancel a turn, and answer an agent's permission request or question, while the computer running wai stays where it is. The client is the native wai-mobile app (Flutter, iOS and Android), not a web page.

Out of scope for the first version: starting a session, viewing files or diffs, the terminal, push notifications, listening on the LAN or through an internet relay.

## Why this is dangerous

Controlling a session is controlling an agent, and agents run tools on the computer: shell commands, file writes. Anyone who can drive wai-remoto can run code as the user. The design treats every request as hostile until a paired device's token proves otherwise.

## Security model

### Where it listens

- **Loopback only:** `127.0.0.1:<port>`, port 1024 to 65535 (default 7717). There is no LAN mode. The phone arrives through `tailscale serve`, which terminates HTTPS inside the user's tailnet and forwards to the loopback port.
- **Nothing listens until the user clicks Start** in the panel, with a valid port and a tunnel URL starting with `https://`. Stop closes the listener and every connection at once. Disabling or uninstalling the extension kills its process, so nothing listens.
- **wai itself never listens.** The HTTP server lives in the extension's process; wai only talks to it over stdio.

### Pairing

- **QR code + one-time code.** "Pair a device" makes a code of 16 random bytes (base64url), valid 5 minutes, used once; a new code replaces the previous one. The QR code carries `wai-remoto:pair?url=<tunnel URL>&code=<code>`.
- **Codes live in memory only.** A restart forgets the live code.
- **Guessing is not a threat:** 128 bits in a 5-minute window. Behind `tailscale serve` every request comes from `127.0.0.1`, so a per-address limit would mean nothing; failures are counted globally instead (see Rate limits).

### Device tokens

- **One token per device:** 32 random bytes (base64url), returned once by `POST /v1/pair`, sent as `Authorization: Bearer <token>` on every other request.
- **Stored as SHA-256 only**, in `state.json` in the extension's data folder, written `0600` through a temporary file. A copy of the file pairs nothing. Tokens and prompts are never logged. Lookups compare hashes in constant time.
- **The app keeps the token** in the Keychain (iOS) or the Keystore (Android).
- **Expiry:** 30 days without use. Each authenticated request restarts the 30 days; an expired device is removed.
- **Revocation:** Revoke in the panel deletes one device's token and closes its event streams at once. "Sign out all devices" deletes every token and closes every stream. Unpair in the app revokes its own token.

### Every request

In this order, before any route:

1. **No `Origin` header**, whatever its value: `403`. The client is a native app, which sends none; a browser always does on the requests that matter, so a web page cannot reach the API.
2. **Host check:** the `Host` must be the tunnel URL's host or `127.0.0.1:<port>`, else `421`. This defeats DNS rebinding.
3. **Body cap:** 64 KB, else `413`.
4. **Bearer token** (every route but `/v1/pair`): unknown, revoked or expired is `401`.

Every response carries `Cache-Control: no-store`. An unknown route answers `404` only to a paired device; others get `401`.

### Rate limits

- **Failed sign-ins:** after 20 refused tokens or pairing codes within an hour, wai notifies the user once (`wai-remoto: 20 failed sign-ins in the last hour`), and at most once an hour. The owner's own phone is never locked out.
- **Prompts:** at most 30 per minute per device (`429`).
- **Event streams:** at most 4 open per device (`429`). A stream too slow to keep up is closed; the app reconnects and reads the session again.

### What it never does

It never reads, stores, forwards or asks for any provider's credentials, and never sends a tool call's output to the phone: transcript items carry a tool's title, kind and status only.

## Threat model

| Who | Can | Stopped by |
|---|---|---|
| A web page open in the user's browser | Send requests to `127.0.0.1` (CSRF, DNS rebinding) | Any `Origin` refused, Host check, no cookies: a Bearer token the page does not have |
| Another user or process on the same computer | Connect to the loopback port | Device token, failed sign-in notification |
| Someone in the user's tailnet | Reach the tunnel URL | Device token; pairing needs a code shown only in wai's panel |
| Someone guessing tokens or pairing codes | Many requests | 256-bit tokens, 128-bit single-use codes valid 5 minutes, failed sign-in notification |
| A lost phone or a stolen token | Act as that device | Revoke, "Sign out all devices", 30-day idle expiry |
| A copy of `state.json` | Read the devices | It holds SHA-256 hashes only; a hash is not a token |

Not covered:
- A computer that is already compromised.
- A malicious extension: in v1 any extension granted `process` runs with the user's powers anyway, and `listen` is not enforced (like `network`).
- The tunnel's own security: Tailscale's.

## Transport

HTTP JSON for actions and Server-Sent Events for live updates, standard library only.

| Method and path | Auth | Result |
|---|---|---|
| `POST /v1/pair {code, device}` | code | `200 {token}` / `401` |
| `POST /v1/unpair` | token | `204` |
| `GET /v1/sessions` | token | `200 [{agent, session, title, state}]` |
| `GET /v1/sessions/{agent}/{session}?from=&limit=` | token | `200 {items, total}` |
| `POST /v1/sessions/{agent}/{session}/prompt {text}` | token | `204` / `409` busy / `429` |
| `POST /v1/sessions/{agent}/{session}/cancel` | token | `204` |
| `POST /v1/sessions/{agent}/{session}/permissions/{request} {option}` | token | `204` / `409` already answered |
| `POST /v1/sessions/{agent}/{session}/questions/{request} {answer, values?}` | token | `204` / `409` / `422 {field}` |
| `GET /v1/events` | token | SSE: one `event:` and `data:` per wai event; `: ping` every 15 s |

wai's errors map to statuses: `No open session …` is `404`, `Request <id> is already answered or unknown` is `409`, `Field <key>: <why>` is `422 {"field": "<key>"}`.

A prompt to a session that is not idle answers `409`, from the states wai-remoto keeps from `event/session_state` and `sessions/list`: wai drops such a prompt silently. A race between that cache and wai remains; the transcript shows the truth.

## Flows

1. **Install.** The consent lists `process`, `panels`, `sessions:read`, `sessions:content`, `sessions:control`, `notify` and `listen`. Nothing listens.
2. **Start.** The user sets the port and the tunnel URL and clicks Start; the panel shows `Listening on 127.0.0.1:<port>` and the `tailscale serve --bg <port>` command.
3. **Pair.** "Pair a device" shows the QR code; the app scans it, sends `POST /v1/pair`, and stores the token.
4. **Use.** The app lists sessions (`sessions/list`), reads one (`sessions/read`), follows `GET /v1/events`, prompts (`sessions/prompt`), cancels (`sessions/cancel`), answers (`permission/answer`, `question/answer`). The first answer wins, whether given in wai or on the phone.
5. **Revoke.** Revoke, "Sign out all devices", Unpair or 30 idle days.
6. **Stop.** Stop closes the listener and every connection; disabling the extension kills the process.

## What wai provides (extension API 1.10)

- The `listen` permission, shown in the consent as *Listen for connections on a network port*, not enforced.
- The `sessions:content` permission: `sessions/read` and the events `session_update`, `permission_requested`, `permission_resolved`, `question_requested`, `question_resolved`. `sessions:read` alone does not carry transcripts.
- Under `sessions:control`: `sessions/cancel`, `permission/answer`, `question/answer`.
- A private data folder per extension (`WAI_EXT_DATA`), where `state.json` lives.
