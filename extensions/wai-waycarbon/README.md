# wai-waycarbon

A wai extension for the WayCarbon team (0.5, extension API 1.5). It contributes:

- three session templates (front, BFF, Go service) that carry the team's conventions: `feat:`/`fix:`/`refactor:` commits without a scope, `Refs: EC-XXXX`, branches from `development`, PR titles ending with the EC key;
- a requirement on the `waycarbon` Claude plugin (`waycarbon/claude-code`): installing the extension asks before installing it;
- a **WayCarbon** panel with four tabs, **Work**, **Jira**, **PRs** and **Help**;
- a bar item with a counter of what needs you, and a card with the count per group.

## Work tab

An inbox of what needs you now: your PRs with a merge conflict, failing CI, requested changes or an approval (even while the CI still runs; in a repo that does not require reviews, the reviews themselves decide), the reviews requested of you, and the cards whose PRs all merged (Move to Done, after a confirmation). Clicking one opens a session with the matching prompt pre-filled (not sent): `/waycarbon:code-review`, `/waycarbon:fix-review`, `/waycarbon:recheck-review`, `/waycarbon:solve-card`, `/waycarbon:deploy`, or a plain sentence for conflicts, CI and merging. A prompt carries only PR URLs or a Jira key. When someone else approves, requests changes on or comments on one of your PRs, a notification says who and which PR, at the next refresh (every 10 minutes, or Refresh); the first load after wai starts notifies nothing. If the `waycarbon` plugin is missing, the tab says so and offers Install plugin; signing in to GitHub happens here too.

## Jira tab

Your open Jira cards, grouped by status, each with its first open PR. Clicking a card works as on the Work tab.

**Search** finds any card of the project, Done included: by key (`EC-1234`, any case) or by text, as Jira's search box does (`jira issue list <text>`), at most 50. The results replace the statuses until **Clear**; clicking a card that is not yours opens `/waycarbon:solve-card`.

## PRs tab

Your open PRs, the reviews requested of you and the team queue (the `review` label), each grouped by Jira card: the first EC key in a PR's title or branch, or "No card". A card with two or more PRs has **Review all N**, which opens one session whose prompt asks to run `/waycarbon:code-review` on each PR and then check they fit each other.

## Help tab

Colleagues send each other plain-text help requests; replies can go back into the session that asked. It needs the relay (see [relay/README.md](relay/README.md)), which WayCarbon does not host yet: until it does, the tab says so. Set `WAI_WAYCARBON_RELAY` to a relay URL to use one. Sign-in is GitHub's device flow, and only members of the GitHub org can use it.

## Build and install

```sh
cd extensions/wai-waycarbon
go build -o bin/wai-waycarbon .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent: the permissions `process`, `panels`, `bar`, `sessions:control`, `sessions:read`, `notify` and `network` (for the relay), and the `waycarbon` plugin to install.

## CLIs and tokens

It runs your own `gh` and `jira` (jira-cli) with argv only, never a shell: `gh api graphql` for the PRs, `jira me` and `jira issue list` / `jira issue move` for the cards. The extension never reads or stores the `gh` or `jira` tokens; the CLIs hold them. If one is missing or not logged in, the panel says so (`gh auth login` from the tab, `jira init`) and offers Refresh. The only token it stores is the relay's, in its data folder.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `WAI_WAYCARBON_ORG` | `waycarbon` | GitHub org whose PRs are searched |
| `WAI_WAYCARBON_DONE` | `Done` | Jira status "Move to Done" moves a card to |
| `WAI_WAYCARBON_RELAY` | empty | Relay URL for the Help tab |

## Jira MCP server

No MCP server is bundled: contributing one needs the `process` permission, and this extension assumes no local Jira MCP. To add yours, create a separate extension whose `wai-extension.toml` has `permissions = ["process"]` and a `[[contributes.mcp_servers]]` entry (`name`, `command`, `args`, `env`), then install it like this one.

## Tests

```sh
go vet ./... && go test ./...
```

They use a fake `jira` script on `PATH`, never the real CLI or the network.
