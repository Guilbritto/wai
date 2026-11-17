# wai-waycarbon

A wai extension for the WayCarbon team (0.6, extension API 1.6). It contributes:

- three session templates (front, BFF, Go service) that carry the team's conventions: `feat:`/`fix:`/`refactor:` commits without a scope, `Refs: EC-XXXX`, branches from `development`, PR titles ending with the EC key;
- a requirement on the `waycarbon` Claude plugin (`waycarbon/claude-code`): installing the extension asks before installing it;
- a **WayCarbon** panel with three tabs, **Work**, **Jira** and **PRs**. It is wide: its icon in the sidebar shows it in the main area's place, and opening a session brings the main area back;
- a bar item with a counter of what needs you, and a card with the count per group.

Each tab is a list beside a detail: clicking an item selects it (the first is selected at first, and the selection survives a refresh), and the detail's buttons open a session with the matching prompt pre-filled, not sent. A prompt carries only PR URLs or a Jira key.

## Work tab

An inbox of what needs you now:

- **Needs you**: your PRs with a merge conflict, failing CI, requested changes or an approval (even while the CI still runs or a check that is not required fails, such as SonarCloud; in a repo that does not require reviews, the reviews themselves decide);
- **Review requests**: the reviews requested of you;
- **Cards to update**: the cards whose PRs all merged, with **Deploy** (`/waycarbon:deploy`) and **Move to Done** (after a confirmation);
- **Waiting on others**: your PRs that wait for a review or the CI.

A PR's detail shows its badges, size, checks (failed first), reviews and open threads, and its action: Resolve conflict, Fix CI, Fix review (`/waycarbon:fix-review`), Merge, Recheck review (`/waycarbon:recheck-review`) or Review (`/waycarbon:code-review`). When someone else approves, requests changes on or comments on one of your PRs, a notification says who and which PR, at the next refresh (every 10 minutes, or Refresh); the first load after wai starts notifies nothing. If the `waycarbon` plugin is missing, the tab says so and offers Install plugin; signing in to GitHub happens here too.

## Jira tab

A board in the order of the Interstellar board: Tech Refinement, To Do, Ready For Dev, Development, Development Done, Code Review, Code Review Done, Ready For Test, Test, Test Done; statuses it does not know come after, and an empty column is a thin strip. **Sprint atual** shows your open cards in the current sprint, **Todos os meus** all your open cards. A card shows how many of your open PRs name it.

The detail shows the card's type, priority, parent, assignee, description, sub-tasks and the org's PRs that name it, with **Solve card** (`/waycarbon:solve-card`), **Move to** (any status of the board, after a confirmation) and **Open in Jira**.

**Search** finds any card of the project, Done included: by key (`EC-1234`, any case) or by text, as Jira's search box does (`jira issue list <text>`), at most 50. The results replace the board until **Clear**.

## PRs tab

Your PRs, the reviews requested of you and the team queue (the `review` label), by Jira card: the first EC key in a PR's title or branch. A card with two or more PRs has an item of its own above them; its detail lists them with **Review all N**, which opens one session whose prompt asks to run `/waycarbon:code-review` on each PR and then check they fit each other.

**Search** finds any PR of the org whose title has the text (`EC-1234` or words), open, merged or closed, at most 50, until **Clear**. An open one offers Review.

## Build and install

```sh
cd extensions/wai-waycarbon
go build -o bin/wai-waycarbon .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent: the permissions `process`, `panels`, `bar`, `sessions:control` and `notify`, and the `waycarbon` plugin to install.

## CLIs and tokens

It runs your own `gh` and `jira` (jira-cli) with argv only, never a shell: `gh api graphql` for the PRs, `jira me`, `jira issue list` / `jira issue view` / `jira issue move` for the cards. The extension never reads or stores the `gh` or `jira` tokens; the CLIs hold them. If one is missing or not logged in, the panel says so (`gh auth login` from the tab, `jira init`) and offers Refresh.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `WAI_WAYCARBON_ORG` | `waycarbon` | GitHub org whose PRs are searched |
| `WAI_WAYCARBON_DONE` | `Done` | Jira status the Work tab's "Move to Done" moves a card to |

## Jira MCP server

No MCP server is bundled: contributing one needs the `process` permission, and this extension assumes no local Jira MCP. To add yours, create a separate extension whose `wai-extension.toml` has `permissions = ["process"]` and a `[[contributes.mcp_servers]]` entry (`name`, `command`, `args`, `env`), then install it like this one.

## Tests

```sh
go vet ./... && go test ./...
```

They use a fake `jira` script on `PATH`, never the real CLI or the network.
