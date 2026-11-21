# wai-remoto

Follow and drive wai's sessions from your phone. wai-remoto serves an HTTP API on `127.0.0.1` that the wai-mobile app (iOS and Android) reaches through a Tailscale tunnel. From the phone you can:

- list the open sessions and their state;
- read a session's transcript and follow a turn live;
- send a prompt to an idle session, and cancel a running turn;
- answer an agent's permission request or question.

Nothing listens until you click **Start** in the **Remoto** panel. The design and the threat model are in [DESIGN.md](DESIGN.md).

## Build and install

```sh
cd extensions/wai-remoto
go build -o bin/wai-remoto .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent: `process`, `panels`, `sessions:read`, `sessions:content` (*Read your sessions' conversations*), `sessions:control`, `notify` and `listen` (*Listen for connections on a network port*). It needs wai's extension API 1.10.

## Tailscale

The phone never reaches the port directly: wai-remoto binds `127.0.0.1` only, and Tailscale carries the traffic over HTTPS inside your tailnet.

1. Install Tailscale on the computer and on the phone, signed in to the same tailnet, with HTTPS certificates enabled for the tailnet (admin console, DNS, HTTPS Certificates).
2. In the Remoto panel, set the **Port** (default `7717`, 1024 to 65535) and the **Tunnel URL**: your computer's tailnet name, such as `https://my-mac.tail0123ab.ts.net`. It must start with `https://`.
3. Click **Start**. The panel shows `Listening on 127.0.0.1:<port>`, or `Port <port> is in use` (pick another port).
4. Run the command the panel shows:

   ```
   tailscale serve --bg <port>
   ```

   Checked against Tailscale's documentation (https://tailscale.com/kb/1242/tailscale-serve) on 2026-11-21: `--bg` keeps it running in the background, a bare port number is a local target, and HTTPS on port 443 is the default. Add `off` to the end of the same command to stop it.

wai-remoto accepts requests whose `Host` is the tunnel URL's host or `127.0.0.1:<port>`, so it works whichever of the two `tailscale serve` passes on.

## Pairing

1. With wai-remoto started, click **Pair a device**. The panel shows a QR code and the time left: the code works once, for 5 minutes, and a new click replaces it.
2. In wai-mobile, tap **Scan QR code** and scan it. The QR carries `wai-remoto:pair?url=<tunnel URL>&code=<code>`.
3. The app sends the code and the phone's name; wai-remoto answers with a token for that device, and the device shows in the panel with its pairing date.

A used, expired or unknown code is refused. The phone keeps its token in the Keychain or Keystore; wai-remoto keeps only its SHA-256, in `state.json` in its data folder.

## Devices

- The panel shows `<n> devices connected` while started: the devices with a live event stream.
- **Revoke** deletes a device's token and closes its connections at once. **Sign out all devices** does it for every device.
- A token unused for 30 days stops working and its device leaves the list. Every request with it restarts the 30 days.
- **Unpair** in the app revokes that phone's own token.
- **Stop** closes the listener and every connection within a second. Disabling or uninstalling the extension stops its process, so nothing listens.

## Limits

- Request bodies: 64 KB (`413`).
- Prompts: 30 per minute per device (`429`). A prompt to a session that is not idle answers `409`.
- Event streams: 4 open per device (`429`).
- Requests with an `Origin` header answer `403`; an unknown `Host`, `421`; a missing or unknown token, `401`.
- After 20 refused tokens or pairing codes within an hour, wai shows one notification: `wai-remoto: 20 failed sign-ins in the last hour`.
- Every response carries `Cache-Control: no-store`.

## HTTP API

All JSON. Every route but `/v1/pair` needs `Authorization: Bearer <token>`.

| Method and path | Result |
|---|---|
| `POST /v1/pair {code, device}` | `200 {token}`, `401` |
| `POST /v1/unpair` | `204` |
| `GET /v1/sessions` | `200 [{agent, session, title, state}]` |
| `GET /v1/sessions/{agent}/{session}?from=&limit=` | `200 {items, total}`, `404` |
| `POST /v1/sessions/{agent}/{session}/prompt {text}` | `204`, `409` busy, `429` |
| `POST /v1/sessions/{agent}/{session}/cancel` | `204` |
| `POST /v1/sessions/{agent}/{session}/permissions/{request} {option}` | `204`, `409` already answered |
| `POST /v1/sessions/{agent}/{session}/questions/{request} {answer, values?}` | `204`, `409`, `422 {field}` |
| `GET /v1/events` | Server-Sent Events: `session_state`, `turn_ended`, `session_update`, `permission_requested`, `permission_resolved`, `question_requested`, `question_resolved`, each with wai's event as `data`; a `: ping` comment every 15 s |

A refusal is `{"error": "<why>"}`.

## Tests

```sh
go vet ./... && go test ./...
```
