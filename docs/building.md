# Building wai

Every macOS command below was run on macOS (Apple silicon). Windows commands are marked
**CI only**: they are not run on a developer machine, only on GitHub's `windows-latest`.

## Requirements

- Rust: the toolchain pinned in `rust-toolchain.toml` (rustup installs it, with the Windows target,
  on first use).
- macOS: the Xcode Command Line Tools are enough; full Xcode is not needed.
- To use the Claude agent: Node.js (`npx`) and a signed-in Claude Code.

## Build and run

```sh
cargo build -p wai            # target/debug/wai
cargo build --release -p wai  # target/release/wai
```

`wai [FOLDER]` opens FOLDER, else the last project, else the current directory. When the
current directory is the one a launcher picked (`/` from Finder, the Dock or `open`, or the
install folder from a Windows shortcut), it opens your home folder instead. It keeps its settings
and state in `~/.config/wai` (`%APPDATA%\wai` on Windows). To try a build without touching your
own config, give it a throwaway home:

```sh
mkdir -p /tmp/wai-demo && HOME=$(mktemp -d) target/debug/wai /tmp/wai-demo
```

Run the built binary this way, never `cargo run`: under a fake `HOME`, rustup installs a whole
toolchain into it.

## Test

The UI and agent tests start the fake agent, so build it first:

```sh
cargo build -p wai-agent --bin wai-fake-agent
cargo test --workspace
```

The rest of the CI gate:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

On Windows the build and test commands are the same (**CI only**, `.github/workflows/ci.yml`).

## Package

Packaging uses [cargo-packager](https://github.com/crabnebula-dev/cargo-packager). Its config is
`[package.metadata.packager]` in `crates/wai/Cargo.toml`, which is also the one place that sets the
bundle id (`app.wai.ide`, a placeholder). Only the `wai` binary is packaged.

```sh
cargo install cargo-packager --locked
```

### macOS: app and DMG

```sh
CI=true cargo packager --release
```

- It builds the release binary first, then writes `target/release/wai.app` and
  `target/release/wai_<version>_aarch64.dmg`.
- Always pass `--release`; the hook builds only the release binary.
- `CI=true` skips the create-dmg step that drives Finder through AppleScript to arrange the DMG
  window; that step needs permission to control Finder. Only the `CI=true` form has been verified.
- The first DMG build downloads the create-dmg script (pinned to a commit) into
  `~/Library/Caches/.cargo-packager`.
- The bundle is not signed or notarized: the binary carries only the linker's ad-hoc signature.

Check the bundle id and version:

```sh
plutil -extract CFBundleIdentifier raw target/release/wai.app/Contents/Info.plist
plutil -extract CFBundleShortVersionString raw target/release/wai.app/Contents/Info.plist
```

### Windows: NSIS installer (CI only)

```powershell
cargo install cargo-packager --locked
cargo packager --release
```

It writes `target\release\wai_<version>_x64-setup.exe` and downloads NSIS on first use. The
installer uses `assets/icon/icon.ico`; `wai.exe` itself has no embedded icon yet.

## Icon

`crates/wai/assets/icon/icon.svg` is the source. After editing it, regenerate `icon.png` and
`icon@2x.png` (from which cargo-packager builds the `.icns`) and `icon.ico` (the NSIS installer
icon). This needs macOS (`sips`):

```sh
crates/wai/assets/icon/generate.sh
```
