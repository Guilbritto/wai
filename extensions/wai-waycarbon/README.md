# wai-waycarbon

A wai extension for the WayCarbon team (0.2, extension API 1.5). It contributes:

- three session templates (front, BFF, Go service) that carry the team's conventions: `feat:`/`fix:`/`refactor:` commits without a scope, `Refs: EC-XXXX`, branches from `development`, PR titles ending with the EC key;
- the `beehive-waycarbon` Claude plugin, and a requirement on the `waycarbon` Claude plugin (`waycarbon/claude-code`): installing the extension asks before installing it;
- a **WayCarbon** panel with two tabs, **Work** and **Help**;
- a bar item with a counter of what needs you, and a card with the count per group.

## Work tab

Shows your open PRs, review requests and Jira cards, and flags what needs you: merge conflicts, failing CI, requested changes, PRs ready to merge, cards with every PR merged. Clicking one opens a session with the matching prompt pre-filled (not sent): `/waycarbon:code-review`, `/waycarbon:fix-review`, `/waycarbon:recheck-review`, `/waycarbon:solve-card`, `/waycarbon:deploy`, or a plain sentence for conflicts, CI and merging. A prompt carries only a PR URL or a Jira key. A card can be moved to Done after a confirmation. If the `waycarbon` plugin is missing, the tab says so and offers Install plugin.

## Help tab

Colleagues send each other plain-text help requests; replies can go back into the session that asked. It needs the relay (see [relay/README.md](relay/README.md)), which WayCarbon does not host yet: until it does, the tab says so. Set `WAI_WAYCARBON_RELAY` to a relay URL to use one. Sign-in is GitHub's device flow, and only members of the GitHub org can use it.

## Build and install

```sh
cd extensions/wai-waycarbon
go build -o bin/wai-waycarbon .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent (the permissions it asks for and the `waycarbon` plugin to install).

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
