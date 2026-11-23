# Remote access Context

**Gathered:** 2026-11-20
**Spec:** `.specs/features/remote-access/spec.md`
**Status:** Ready for design

---

## Feature Boundary

Continue development away from the computer running wai: from a phone, drive wai's sessions through the `wai-remoto` extension, built end to end on the design in `extensions/wai-remoto/DESIGN.md`.

---

## Implementation Decisions

### Scope of v1

- Sessions as in DESIGN.md: list sessions, read a transcript, follow a turn as it streams, send a prompt, answer a permission request, cancel a turn.
- Not chosen: starting a new session, viewing files or diffs, a remote terminal.
- User: picked "Sessões (DESIGN.md)".

### Network

- wai-remoto listens on loopback only (`127.0.0.1`/`::1`); the phone reaches it through a tunnel the user runs (Tailscale, SSH).
- No LAN mode, no internet relay.
- User: picked "Localhost + túnel".

### Client

- A phone app is the client. User: picked "Web app no celular (PWA)" with the note "Vamos fazer um app e usar esse APP pra acessar".
- Clarified: a separate native / cross-platform app installed on the phone, talking to an API served by wai-remoto. Not a PWA. User: picked "App nativo / multiplataforma".
- Consequence for DESIGN.md: the cookie, `SameSite` and Origin rules were written for a browser; a native client needs its own credential scheme.
- Framework: Flutter. User: picked "Flutter".
- Platforms in v1: iOS and Android. User: picked "iOS e Android".
- Code lives in a separate repository with its own release cycle. User: picked "Repo separado".
- Repository: `C:\Users\guilh\Project\wai-mobile`, local `git init`; a GitHub repo only when the user asks. User: picked "wai-mobile, ao lado do wai".

### Tunnel

- The flow assumes `tailscale serve`, which publishes `https://<machine>.<tailnet>.ts.net` to the user's own devices and forwards to `127.0.0.1:<port>`. The user types that URL in the panel; it goes in the QR code. User: picked "Tailscale serve".

### Device token lifetime

- 30 days without use; each use renews it; revocable in the panel at any time. User: picked "30 dias, renova com uso".

### Pairing and auth

- The wai-remoto panel shows a QR code holding the URL and a one-time code; the app scans it and receives a token for that device, revocable in the panel. No password. User: picked "QR code + token".

### Push notifications

- Out of v1: the app shows state while it is open. User: picked "Fora da v1".

### Presence in wai

- The wai-remoto panel shows how many devices are connected; sessions look the same. User: picked "Indicador discreto".
- A permission request answered on either side: the first answer wins and the other side shows it answered (DESIGN.md).

### Delivery

- End to end: the host API DESIGN.md asks for (`listen`, `sessions/read`, `event/session_update`, permission request and answer, `sessions/cancel`) plus a working wai-remoto with login and the phone client.
- User: picked "Ponta a ponta".

### Agent questions

- Answering an agent's question card (ACP elicitation: single choice, multiple choice, text, boolean, number; Submit, Decline, Cancel) from the phone is in this delivery. User: "vamos incluir na entrega de agora".

### Where the specs live

- `.specs/` is tracked in the wai repository: each task's commit carries its `tasks.md` mark. User: "vai pro git do wai".

### Transport (design)

- The app calls wai-remoto with JSON over HTTP and receives live updates as Server-Sent Events; wai-remoto uses Go's standard library only for it. EDGE-04 and EDGE-05 were reworded from WebSocket messages and connections to request bodies and open event streams. User: picked "HTTP + SSE".

### Permission for conversation content (design)

- New permission `sessions:content` for `sessions/read`, `event/session_update` and the permission and question events; `sessions:read` stays states only. HOST-01, HOST-02, HOST-04 and QST-01 now name `sessions:content`; HOST-10 and HOST-11 added. User: "A".

### Agent's Discretion

None yet.

### Declined / Undiscussed Gray Areas → Assumptions

None yet.

---

## Specific References

- `extensions/wai-remoto/DESIGN.md`: security model, threat model, flows, host additions.

---

## Deferred Ideas

- Starting new sessions, read-only files and diffs, remote terminal: not in v1.
- LAN mode and an internet relay: not in v1.
- Push notifications to the phone: not in v1.
