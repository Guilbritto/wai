# wai

A native, AI-first IDE for macOS and Windows. wai drives coding agents (Claude first) through the
[Agent Client Protocol](https://agentclientprotocol.com) and is extensible through out-of-process extensions.

Status: early development. See `docs/ESTUDO.md` (study, pt-BR) and `docs/superpowers/plans/` (plans).

## Build

Requires Rust (pinned in `rust-toolchain.toml`) and, for the Claude agent, Node.js and a signed-in Claude Code.

```sh
cargo run -p wai --release
```
