# wai-remoto: design

Status: **design only.** Nothing here is built, and nothing listens. The skeleton manifest has no process. Building it needs the owner's security review of this document first.

## Goal

From a phone, see wai's sessions, follow a turn as it streams, send a prompt, and answer an agent's permission request, while the computer running wai stays where it is. The phone uses a small web app (a PWA) served by the extension.

Out of scope for the first version: editing files, the terminal, starting agents that are not configured, and anything that reaches wai from the public internet without a tunnel the user set up.

## Why this is dangerous

Controlling a session is controlling an agent, and agents run tools on the computer: shell commands, file writes. Anyone who can drive wai-remoto can run code as the user. The design treats every request as hostile until it proves otherwise.

## Security model

This reuses the security model of the old Central: its ideas and behaviour, never its code.

### Where it listens

- **Localhost only by default:** `127.0.0.1` (and `::1`) on a port the user picks in the panel. A phone reaches it through a tunnel the user runs (Tailscale, an SSH forward).
- **LAN only on explicit opt-in:** binding a non-loopback address takes a setting the panel names plainly ("Anyone on this network can reach the login page"). The extension then serves only HTTPS, with a certificate the user provides; it never falls back to plain HTTP off loopback.
- **Nothing listens until the user clicks Start** in the panel, and only after a password exists. Disabling the extension stops it, since wai kills the process and its group.

### Password

- **No default password.** The user sets one in the panel on first use, at least 12 characters.
- **Stored as PBKDF2-HMAC-SHA256:** a random 16-byte salt per install and at least 600,000 iterations (OWASP's 2023 figure), with the parameters stored next to the hash so they can be raised later.
- **The password itself is never stored or logged.** Comparison is constant-time.
- **The file is private:** the extension's data file (hash, salt, cookie key) is created `0600`, in a `0700` folder.

### Session cookie

- After a correct password, the server sets one cookie holding a random session id, the time it was issued and its expiry (12 h), signed with HMAC-SHA256 under a random 32-byte key made at setup.
- **Flags:** `HttpOnly`, `SameSite=Strict`, `Path=/`, plus `Secure` whenever it serves HTTPS.
- **Checked on every request:** a bad signature, an expired cookie or a revoked id answers 401.
- **Logout** revokes the id server-side.
- **"Sign out all devices"** in the panel, and any password change, rotate the signing key, so every cookie stops working at once.

### Origin and Host

- **Exact Origin on every state-changing request:** every `POST` and every WebSocket upgrade must carry an `Origin` equal to the configured origin, scheme, host and port compared exactly. No prefix, suffix or wildcard match. A missing `Origin` is rejected.
- **Host check:** the `Host` header must be one of the configured names. This defeats DNS rebinding, where a web page in the user's own browser resolves its name to `127.0.0.1`.
- **No `GET` request changes anything.** Together with `SameSite=Strict` and the Origin check, this is the CSRF defence.

### Rate limits

- **Login:** 5 attempts per minute per client address, doubling the wait after each failure. After 20 failures in an hour the login is locked for every client until the user unlocks it in the panel, and wai notifies the user (`notify`).
- **Prompts:** at most 30 per minute per signed-in device.
- **Input caps:** requests are capped at 64 KB, and WebSocket messages at 64 KB in and 4 MB out.
- **Connections:** at most 4 live connections per device.

### Responses

- **Headers:** `Content-Security-Policy: default-src 'self'` with no inline scripts, `frame-ancestors 'none'`, `Referrer-Policy: no-referrer`, and `Cache-Control: no-store` on the API.
- **Escaping:** the web app renders agent output as text, never as HTML.

### What it never does

It never reads, stores, forwards or asks for any provider's credentials. If an agent is not signed in, the phone shows the same message wai shows.

## Threat model

| Who | Can | Stopped by |
|---|---|---|
| A web page open in the user's browser | Send requests to `127.0.0.1` (CSRF, DNS rebinding) | Exact Origin, Host check, `SameSite=Strict`, no state change on `GET` |
| Another user or process on the same computer | Connect to the loopback port | Password, cookie, rate limits |
| Someone on the same network (LAN opt-in only) | Reach the login page, sniff traffic | HTTPS only off loopback, password, rate limits, lockout |
| Someone guessing passwords | Many logins | PBKDF2 cost, per-address backoff, global lockout with a notification |
| A lost phone or a stolen cookie | Act until the cookie expires | 12 h expiry, logout, "Sign out all devices" |
| A copy of the data file | Offline guessing | PBKDF2 with salt; rotating the key and password is a click |

Not covered:
- A computer that is already compromised.
- A malicious extension: in v1 any extension granted `process` runs with the user's powers anyway.
- The tunnel's own security.

## Flows

1. **Install.**
   - The consent lists `process`, `sessions:read`, `sessions:control` and `listen`, with the usual line that wai cannot restrict files or network access.
   - Nothing starts listening.
2. **Set up.**
   - The panel shows a form: password, confirmation, port.
   - Saving writes the hash and a new cookie key, then shows the URL and a Start button.
3. **Start.**
   - The extension binds the address.
   - The panel shows `Listening on http://127.0.0.1:<port>` and a Stop button.
4. **Log in.**
   - The phone opens the URL and gets the login page.
   - `POST /login`, with Origin, Host and rate limit checked, sets the cookie.
5. **Use.**
   - **List sessions:** `sessions/list`.
   - **Open one** and follow its stream: needs a new host event, see below.
   - **Send a prompt:** `sessions/prompt`.
   - **Answer a permission request:** needs a new host method.
   - **Cancel a turn:** needs a new host method.
6. **Sign out.**
   - Logout from the phone revokes its session id.
   - "Sign out all devices" in the panel rotates the key.
7. **Stop.**
   - The Stop button closes the listener.
   - Disabling or uninstalling the extension kills the process.

## What wai must add first

1. **A `listen` permission.** It must be parsed, shown in the consent ("listen for connections on a network port") and stored like the others. Like `network`, v1 cannot enforce it without a sandbox. An enforceable variant would be for wai to open the socket itself, bound to loopback, and hand it to the extension.
2. **Session content for extensions.** Today wai sends only `event/session_state` and `event/turn_ended`. The phone needs:
   - the transcript of a session (`sessions/read`, under `sessions:read`);
   - its updates as they stream (`event/session_update`, coalesced like the UI's and capped per second).
3. **Permission requests.**
   - An `event/permission_requested` with the tool call and its options.
   - A `permission/answer` method, under `sessions:control`. The first answer wins: whichever of wai and the phone answers first, the other sees it answered.
4. **Cancel.** A `sessions/cancel` method, under `sessions:control`.
5. **A private data folder per extension** (`<config>/extension-data/<id>/`, `0700`), so the hash and key do not live in the extension's install folder, which an update replaces.

## Open questions for the owner

- **Tunnel:** which one to recommend (Tailscale, SSH, Cloudflare Tunnel), or none?
- **LAN mode:** worth having at all, or loopback plus a tunnel only?
- **Push notifications** to the phone: they need HTTPS and a push service (VAPID). Is that in scope?
- **Ownership:** who signs off on the security review before any code is written?
