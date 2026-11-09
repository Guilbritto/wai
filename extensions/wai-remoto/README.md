# wai-remoto

**Not functional yet.** This folder holds the design for using wai's sessions from your phone, plus a skeleton manifest. Installing it runs nothing and listens nowhere: the manifest has no process.

- [DESIGN.md](DESIGN.md): what it will do, the security model, the threat model, the flows, and what wai must add before it can be built.
- `wai-extension.toml`: the skeleton. It asks only for `sessions:control`. The finished extension will also need `process`, `sessions:read` and `listen`, a permission wai does not have yet.

Building it needs the owner's security review of DESIGN.md first.
