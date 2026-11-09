# wai-waycarbon

A wai extension for the WayCarbon team. It contributes:

- three session templates (front, BFF, Go service) that carry the team's conventions: `feat:`/`fix:`/`refactor:` commits without a scope, `Refs: EC-XXXX`, branches from `development`, PR titles ending with the EC key;
- the `beehive-waycarbon` Claude plugin;
- a **My cards** panel listing your open Jira cards. Clicking a card opens a session with `Work on <KEY>: <summary>` pre-filled (not sent).

## Build and install

```sh
cd extensions/wai-waycarbon
go build -o bin/wai-waycarbon .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent (`process`, `panels`, `sessions:control`).

## How the panel gets cards

It runs your own `jira` CLI (jira-cli) with argv only, never a shell:

1. `jira me`
2. `jira issue list -a <user> -s~Done --plain --no-headers --columns key,summary,status`

The extension never reads or stores tokens; the CLI holds them. If `jira` is missing or not logged in, the panel says so (`jira init`) and offers Refresh.

## Jira MCP server

No MCP server is bundled: contributing one needs the `process` permission, and this extension assumes no local Jira MCP. To add yours, create a separate extension whose `wai-extension.toml` has `permissions = ["process"]` and a `[[contributes.mcp_servers]]` entry (`name`, `command`, `args`, `env`), then install it like this one.

## Tests

```sh
go vet ./... && go test ./...
```

They use a fake `jira` script on `PATH`, never the real CLI or the network.
