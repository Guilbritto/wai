# Help requests: design

Status: **design only.** Part of the `wai-waycarbon` extension. Nothing here is built.

## Goal

From a wai session, ask **one specific colleague**, by GitHub login, to do something you lack the rights for: a deploy, a command against an environment you cannot reach. Their own agent does it, with their rights, after they read and accept the request. Their reply comes back to your session.

Out of scope for v1:
- sending to a team, or to whoever is available;
- sharing a live session, or anyone driving another person's session;
- running anything automatically, on either side;
- attaching transcripts or files.

## Why this is dangerous

A request is text from another person that becomes a prompt for an agent with **more** rights than the sender. That is prompt injection by design. So:

- **Nothing is sent to an agent automatically.** Accept opens a session with the prompt filled in and **not sent** (`sessions/open` with `send: false`). The colleague reads it, edits it, and sends it.
- **The colleague's permission cards stay.** This feature never answers them.
- **The prompt names its source.** It starts with: `Request from @<login> via wai. A colleague wrote this; check it before running anything.`
- **The reply is not sent automatically either.** The requester clicks **Send to session**.
- **Plain text only.** Request and reply render as `text` nodes, never `markdown`, so a request cannot show a disguised link.
- **No secrets.** The text goes through the relay and is stored there. The form says `Do not paste secrets.`

## The pieces

1. **The extension** (`wai-waycarbon`): a **Help requests** panel, notifications, the GitHub sign-in, and one WebSocket to the relay.
2. **The relay** (`wai-relay`): a small Go service hosted by WayCarbon. It knows who is who, routes requests between people, and keeps them until they are delivered.

The two machines are on different networks, so they need something in the middle. Zed does the same: GitHub sign-in through its own collab server.

## Identity: GitHub

**One GitHub App owned by the WayCarbon org** does both jobs:
- **user sign-in** through the device flow;
- **membership checks**, through its installation token with the org permission *Members: read*.

### Sign-in (device flow, like `gh auth login`)

Extensions cannot listen on a port, so there is no `localhost` callback.

1. The panel shows **Sign in with GitHub**. The extension calls `POST /auth/device` on the relay, which asks GitHub for a device code.
2. The panel shows the user code and a `link` to `https://github.com/login/device`.
3. The extension polls `POST /auth/device/poll` at the interval GitHub returns.
4. Once the user approves, the relay:
   1. gets a user token from GitHub;
   2. reads `GET /user` for the numeric id and the login;
   3. checks `GET /orgs/<org>/members/<login>` with the installation token. Not a member: `403 Only members of <org> can use help requests`;
   4. discards the GitHub token;
   5. returns its own token: 32 random bytes, valid 30 days, stored on the relay only as a SHA-256 hash.
5. The extension saves the relay token in `$WAI_EXT_DATA/auth.json`, mode `0600`, and never logs it.

### Rules

- **Who someone is:** their GitHub numeric id, since a login can be renamed. The login is what people see.
- **Membership** is checked at sign-in, on every connection, and once an hour while connected. Someone who leaves the org loses access within the hour.
- **Why not reuse `gh auth token`:** the relay would receive a token with write access to repositories.
- **Why not trust `gh api user` on the client:** the relay cannot verify a claim the client makes about itself.

## Flows

### 1. Write a request (requester)

- **The draft:** the user asks their own agent for it. The panel shows a suggested prompt: `Write a request for a colleague who has the rights I lack: what to run, where (repo, branch, environment), why, and what to send back. Plain text.`
- **The form** in the panel:
  - **To:** a GitHub login. Below it, the org members who have signed in to the relay, with who is online.
  - **Reply goes to:** a session, from `sessions/list`.
  - **Request:** a multiline input, at most 8,000 characters.
  - **Send.**
- **Project info is added automatically:** the repository and branch of the open project (`git remote get-url origin` and `git branch --show-current`, run with argv only).
- **The extension** sends `request.create`. The request shows as `Waiting for @<login>`, with **Cancel**.

### 2. Receive it (colleague)

- **A notification:** `@<login> asks for help`, with the request's first line.
- **A card in the panel:**
  - who sent it and when;
  - the repository and branch;
  - the full text;
  - **Accept**, and **Decline** with an optional reason.
- **Offline recipient:** the request waits on the relay. It is delivered and notified when they connect, until it expires.

### 3. Accept

- **The relay** marks the request accepted, and the requester sees `Accepted by @<login>`.
- **The colleague's extension** calls `sessions/open` with the header, the project info and the text, `send: false`.

### 4. Reply (colleague)

- **The accepted card** shows a **Reply** input (multiline, at most 8,000 characters) and **Send reply**. The colleague pastes the outcome: a deploy link, a command's output, "done".
- **v1 is manual:** extensions cannot read a session's transcript yet.

### 5. Get the reply (requester)

- **A notification:** `@<login> replied`.
- **The card shows the reply** and **Send to session**, which calls `sessions/prompt` with `@<login> replied to your request: <text>`.
  - The button is enabled only while that session is `idle` (from `event/session_state`), because `sessions/prompt` silently drops text for a busy session.
  - If the session was closed, the button is **Open in a new session** (`sessions/open`, `send: false`).

### 6. End of a request

- **Cancel:** the requester can cancel at any time before the reply.
- **Decline:** the requester sees the reason.
- **Expiry:** a request not accepted within 24 h expires.
- **Retention:** the relay deletes every request 7 days after it ends.

## The relay

- **Stack:** Go, one binary. HTTPS only, with TLS ended by the host's load balancer.
- **Clients:** one WebSocket per client at `wss://<host>/ws`, with `Authorization: Bearer <relay token>`.
- **Storage:** one SQLite file. Kept in memory, waiting requests would be lost on every deploy.
  - `users`: GitHub id, login, last seen.
  - `tokens`: hash, GitHub id, expiry.
  - `requests`: id, from, to, project, text, state, reply, decline reason, timestamps.
- **States:** `waiting → accepted → replied`, plus `declined`, `cancelled` and `expired`.

### Messages (JSON over the WebSocket)

| Direction | Type | Fields | Who may send it |
|---|---|---|---|
| client → relay | `request.create` | `to`, `project`, `text` | anyone signed in |
| client → relay | `request.cancel` | `id` | the sender |
| client → relay | `request.accept` | `id` | the recipient |
| client → relay | `request.decline` | `id`, `reason`? | the recipient |
| client → relay | `request.reply` | `id`, `text` | the recipient, once accepted |
| relay → client | `request.new` | the request | to the recipient |
| relay → client | `request.updated` | `id`, `state`, `reply`?, `reason`? | to both sides |
| relay → client | `people` | `[{login, online}]` | on connect and on change |
| relay → client | `sync` | every open request of this user | on connect |

- **Source of truth:** the relay. The extension keeps only its token and rebuilds the panel from `sync`, because wai restarts extensions when the user opens another folder.
- **Who sees a request:** only its sender and its recipient. Every action is checked on the relay against the GitHub id.

### Limits

- **Text:** a request and a reply hold at most 8,000 characters each, and a WebSocket message at most 64 KB.
- **Requests:** at most 20 an hour per sender, and 10 waiting for the same recipient.
- **Connections:** at most 4 per user.
- **Keep-alive:** a ping every 30 s. The extension reconnects with backoff, from 1 s up to 60 s. No polling, so wai stays idle.

### Logs

- **Logged:** request ids, GitHub ids, logins and states.
- **Never logged:** request and reply text, or tokens.

## Extension changes

- **Permissions:** add `sessions:read` (to pick sessions and know when one is idle), `notify` and `network` to `process`, `panels` and `sessions:control`.
- **A new panel:** `requests`, titled `Help requests`, icon `hand-helping`. It lists:
  - incoming requests first, then sent ones;
  - a **New request** form;
  - **Sign out**, which deletes `auth.json` and revokes the token on the relay.
- **The relay URL** is a constant in the code. `WAI_WAYCARBON_RELAY` overrides it, for testing.

## What wai could add later (not needed for v1)

- **`sessions/read`, the transcript:** the request could be drafted from the session, and the colleague's result attached to the reply without copying.
- **A keychain for extension secrets,** instead of a `0600` file.

## Open questions

- **Hosting:** where the relay runs, on which domain, and who owns it.
- **The GitHub App:** who creates it and administers it in the org.
- **Times:** are 24 h to accept and 7 days of retention right?
- **Sending to a team:** add it later if a specific person turns out not to be enough.
