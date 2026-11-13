# wai-usage

Shows your Claude plan's usage in wai's bottom bar: the session and week percentages (`16% · 59%`), amber at 80% and red at 100%. Hover the item for a card with every window the account reports (session, week, per-model weeks), each with a progress bar and its reset time. Click the item to refresh; otherwise it updates every 5 minutes, and soon after a window resets.

## Build and install

```sh
cd extensions/wai-usage
go build -o bin/wai-usage .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent (`process`, `bar`, `network`).

## Your Claude login

wai-usage reads Claude Code's login on every fetch: the macOS Keychain item `Claude Code-credentials`, else `.credentials.json` in `$CLAUDE_CONFIG_DIR` or `~/.claude`. It only reads it: it never refreshes the token and never writes a credentials file, so it cannot log you out of Claude Code. The token goes only to `api.anthropic.com`, in the request header, and is never logged or shown. If the login expired, the item says so; use Claude Code once.

macOS may ask once to let wai-usage's `security` call read the Keychain item.

## Notes

- The usage endpoint (`/api/oauth/usage`) is undocumented and may change; the item shows `Usage ?` if the response is not understood.
- After an error the card keeps the last good values and says when the update failed. On a 429 it waits for `Retry-After`, else 15 minutes.
- wai's own `Usage ok` item stays; hide it with a right click if you prefer this one.

## Tests

```sh
go vet ./... && go test -race ./...
```

They use a local test server and temporary folders, never your Keychain, your `~/.claude` or the real endpoint.
