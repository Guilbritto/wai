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
cargo install cargo-packager --version 0.11.8 --locked
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
cargo install cargo-packager --version 0.11.8 --locked
cargo packager --release
```

It writes `target\release\wai_<version>_x64-setup.exe` and downloads NSIS on first use. The
installer uses `assets/icon/icon.ico`; `wai.exe` itself has no embedded icon yet.

## Release signing secrets

Pushing a tag `v<version>` runs `.github/workflows/release.yml`. The tag must equal the workspace
version, else the run stops. The workflow packages both platforms and creates a **draft** release
with `SHA256SUMS.txt`. Each signing step runs only when its secrets exist; without them, the packages
are unsigned. The repository secrets are:

- **`APPLE_CERTIFICATE`:** a *Developer ID Application* `.p12`, base64 (`base64 -i cert.p12`). If
  codesign reports a certificate chain error, re-export the `.p12` with its chain.
- **`APPLE_CERTIFICATE_PASSWORD`:** the `.p12` password. It must not contain `"` or `\`. When a
  command fails, cargo-packager logs the whole command line, password included. GitHub masks only the
  exact secret value, and that line is escaped, so such a password would appear in the log.
- **`APPLE_ID` and `APPLE_TEAM_ID`:** the notarization account and team.
- **`APPLE_APP_PASSWORD`:** an app-specific password for that Apple ID, not the account password.
- **`WINDOWS_CERTIFICATE`:** a `.pfx` with its private key, base64.
- **`WINDOWS_CERTIFICATE_PASSWORD`:** the `.pfx` password.

macOS signs with both certificate secrets. It also notarizes only when `APPLE_ID`, `APPLE_TEAM_ID` and
`APPLE_APP_PASSWORD` are all set.

Windows code-signing certificates issued since June 2023 keep their key on hardware and cannot be
exported as a `.pfx`. For those, use a cloud signer instead:

1. Set `windows = { sign-command = "<tool> … %1" }` in `[package.metadata.packager]`.
2. Remove the workflow's certificate import step.

After the first signed release, check the downloaded DMG and the app inside it:

```sh
spctl -a -t open --context context:primary-signature -v wai_<version>_aarch64.dmg
xcrun stapler validate wai_<version>_aarch64.dmg
spctl -a -t exec -vv /Volumes/<volume>/wai.app       # expect: accepted, Notarized Developer ID
xcrun stapler validate /Volumes/<volume>/wai.app
```

The workflow notarizes and staples the app, not the DMG. So the two DMG lines show a signed DMG
without a ticket until the DMG itself is notarized too (`notarytool submit --wait` and
`stapler staple` on it). The two app lines must pass.

## Icon

`crates/wai/assets/icon/icon.svg` is the source. After editing it, regenerate `icon.png` and
`icon@2x.png` (from which cargo-packager builds the `.icns`) and `icon.ico` (the NSIS installer
icon). This needs macOS (`sips`):

```sh
crates/wai/assets/icon/generate.sh
```
