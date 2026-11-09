# wai-navegador

Gives your agents a browser. This extension has no code: its manifest contributes [Playwright's MCP server](https://github.com/microsoft/playwright-mcp) to every session wai starts:

```toml
[[contributes.mcp_servers]]
name = "playwright"
command = "npx"
args = ["--prefix=/", "-y", "@playwright/mcp@0.0.84"]
```

The version is pinned, so an update is a deliberate edit of `wai-extension.toml` (check the latest with `npm view @playwright/mcp version`).

The agent starts the server in the project folder, so `--prefix=/` keeps npx from reading that folder's `.npmrc` and from running a `node_modules/@playwright/mcp` a repository planted there.

## Install

In wai: Extensions, install from folder, pick this folder, and review the consent:

- `process`: the agent starts `npx` for each session;
- `network`: npx downloads the server from npm, and the browser goes online. wai cannot restrict network access in this version.

Nothing is enabled before you consent.

## Requirements

- Node.js, with `npx` on wai's `PATH`. wai looks a bare command up on `PATH` itself; if `npx` is not there, the server is left out and wai logs `wai: MCP server playwright: npx is not on PATH`.
- A browser Playwright can drive. By default the server uses Chrome; if it is missing, ask the agent to install one (the server has a tool for it).

The first session after install waits while npx downloads the package.
