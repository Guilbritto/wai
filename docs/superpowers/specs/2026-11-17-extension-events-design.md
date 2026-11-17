# Extension events (API 1.7)

## Why

An extension that already knows something (wai-waycarbon knows the user's PRs) should tell the rest
of wai, so no other extension reimplements it. The first consumer is wai-bichinho, which will alert
the user when a PR is approved or needs attention, without its own GitHub code.

## Goals

- An extension emits named events with a JSON payload.
- Another extension, with the user's consent, receives the events of that emitter.
- Nothing breaks when the emitter is missing, older or silent: the listener just hears nothing.

Non-goals: request/response between extensions; replaying past events; wai validating payloads.

Anti-goals: an extension reading another's events without the user seeing it at install; one noisy
extension flooding the others.

## Approaches

1. **Host-routed events** (picked). The extension sends `events/emit`; wai stamps the source and
   forwards it to the extensions granted `events:<source>`. Fits the existing ext→host calls and
   host→ext `event/*` notifications, and the existing consent (permissions are shown at install).
2. **Extensions call each other** (an extension exposes methods, another calls them and waits for
   the answer). More powerful, but adds request routing, timeouts and dependency order between
   processes. Not needed while the emitter re-emits its state on every poll.
3. **Shared files** (the emitter writes a JSON file, the listener watches it). No wai change, but no
   consent, no source guarantee, and every extension reinvents the format and the watching.

## Design

**Emit.** Any running extension may send the notification

```json
{"jsonrpc":"2.0","method":"events/emit","params":{"name":"pr.review","data":{...}}}
```

- `name`: `[a-z0-9][a-z0-9._-]{0,63}`; anything else is an `INVALID_PARAMS` error if it was a
  request, dropped and logged if a notification.
- `data`: any JSON, optional (`null`); at most 16 KiB serialized, otherwise dropped and logged.
- No permission needed: emitting reveals nothing to anyone the user did not approve.
- At most 20 events per second per extension; excess dropped and logged, like `notify`.

**Listen.** An extension asks for `events:<extension-id>` in `permissions`, one per source, e.g.
`permissions = ["process", "events:wai-waycarbon"]`. The install consent lists it like `fs:<path>`.
A bad id is an invalid manifest. The source need not be installed.

**Deliver.** wai sends each granted, running extension (never the emitter itself)

```json
{"jsonrpc":"2.0","method":"event/ext","params":{"source":"wai-waycarbon","name":"pr.review","data":{...}}}
```

`source` is set by wai, never by the emitter. Extensions that are not running miss it; there is no
queue. Listeners ignore names they do not know.

**wai's own events** (`event/session_state`, `event/turn_ended`, `event/session_focused`,
`event/file_saved`) stay as they are: they already are wai's side of this bus.

**Version.** `api = "1.7"` for an extension that emits or listens, so an older wai refuses it with
the "needs a newer wai" message instead of an unknown-permission error.

**Contract.** The emitter documents its event names and payloads in its README. wai does not check
them.

## Follow-ups (separate work)

- wai-waycarbon (private repo) emits PR events, e.g. `pr.review {repo, number, title, url, state}`
  with `state` `approved` / `changes_requested`, and `pr.checks {..., state}` with `failing` /
  `passing`.
- wai-bichinho asks for `events:wai-waycarbon` and turns those events into an alert mood and a
  `notify`.

## Left to the builder

- Where the rate limit and size check live (`wai-ext` host or `ExtHost`), following `notify`.
- Tests: manifest parsing of `events:<id>` (good and bad id); routing between two fake extensions
  (granted gets it with the right `source`, ungranted and the emitter do not); size and rate
  limits; docs in `docs/extensions.md` (permission table, method table, `api` line).
