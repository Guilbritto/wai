# wai: automatic releases and self-update

> 2026-11-12. Asked for by the owner: "a release process on GitHub so every new version is up for
> download, and automatic updates in wai, like the ones DevHub has today". Reference implementation:
> DevHub in `waycarbon-ecosystem-local` (`.github/workflows/release-devhub.yml`,
> `.github/release-notes-devhub.md`, `devhub/internal/service/selfupdate/`,
> `devhub/internal/controller/update.go`, `devhub/relocate.go`). Read those before planning.

## Intent

Today a wai version is cut by hand: a `chore: release X` commit that bumps `Cargo.toml`, a `vX` tag,
and a push of the tag. That push runs `.github/workflows/release.yml`, which builds the DMG and the
Windows installer but leaves the GitHub release as a **draft**, so nobody can download it until
someone publishes it by hand. Another session also cuts versions, and the two have collided. Inside
the app, `crates/wai/src/update.rs` only *notices* a newer release. It is off by default, its
repository is unset (`updates.repo` was a placeholder until the repo went public), and it never
downloads anything.

The owner wants both ends automatic, the way DevHub works. A push to `main` publishes a release
anyone can download, with no manual step. A running wai finds the release, downloads it in the
background, and offers **Restart now**. After one manual install, nobody installs wai by hand again.

## Goals

1. Every push to `main` that contains a releasable change publishes a GitHub release on
   `Guilbritto/wai`. The release carries the macOS and Windows downloads, `SHA256SUMS.txt`, notes
   generated from the commits, and install instructions.
2. The version is computed from conventional commits, not typed by anyone.
3. wai checks for updates on every launch by default, downloads the new build in the background,
   verifies it, and offers **Restart now**.
4. On macOS, a wai installed in `/Applications` moves itself to `~/Applications` once, as DevHub
   does. Every later update installs without an administrator.

## Non-goals

- Code signing and notarization. There are no Apple or Windows certificates; the existing optional
  signing path in `release.yml` stays as it is.
- Update channels (beta/nightly), delta updates, Linux, Intel Macs (the macOS runner builds aarch64
  only, as today).
- Rolling back on Windows.
- Restarting without the user's click.

## Anti-goals (a failure even if it technically works)

- An update that leaves wai unable to start, with no way back on macOS.
- wai restarting by itself and killing an agent mid-turn, or dropping unsaved edits.
- A project's `.wai/settings.toml` (any cloned repository) able to turn updates on or off, or to
  point them at another repository. Today only user settings can touch `updates`; that rule stays.
- A release whose binaries report a different version than their tag.
- A manual step creeping back into the release (hand-edited version, draft to publish, tag by
  hand).

## Current state (facts the plan builds on)

- Branch flow: features merge into `development`; `main` is fast-forwarded from `development`. Both
  are at `a818cc8` (`chore: release 0.6.2`), which is also the remote tag `v0.6.2`.
- Commit subjects already follow conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `test:`).
- Version: `[workspace.package] version = "0.6.2"` in the root `Cargo.toml`; the app reads it as
  `env!("CARGO_PKG_VERSION")`.
- Packaging: `cargo packager --release` (cargo-packager 0.11.8, config in `crates/wai/Cargo.toml`)
  writes `wai.app` + `wai_<v>_aarch64.dmg` on macOS and `wai_<v>_x64-setup.exe` (NSIS) on Windows.
  NSIS install mode is cargo-packager's default `CurrentUser`, so installing needs no administrator.
- `release.yml` triggers on tag `v*`, checks the tag equals `Cargo.toml`, builds both platforms,
  writes `SHA256SUMS.txt`, and runs `gh release create --draft --generate-notes`.
  `docs/building.md` ("Release signing secrets") documents it.
- `update.rs`: one GET of `https://api.github.com/repos/<repo>/releases/latest` per launch over
  hyper (no redirect following), semver compare, a notification with **Open release page**. Unit
  tests inject the fetch.
- `wai-settings` `UpdateSettings { check: bool, repo: Option<String> }`, both defaulting to
  off/unset; project-level `updates` is ignored with a notice. Settings → **Updates** section
  (`crates/wai/src/settings_view.rs`) has the switch and a Repository field. Its blurb says "once a
  day", but the code checks once per launch.
- Quit path: `workspace.rs` (R28/R29, around line 1935) already asks before quitting when a tab has
  unsaved edits or a session is working (`quit_asks_while_a_session_works`).
- The owner's machine: `/Applications/wai.app` is a local build, owned by the user, but
  `/Applications` itself is not writable without the Jamf admin elevation, the same situation as
  DevHub. `~/.config/wai/settings.toml` has `[updates] check = true` and no `repo`.
- **GitHub Actions has never run on `Guilbritto/wai`.** The repository is public, Actions is
  enabled, and both workflows show as `active`, but the run count is 0 and there are no releases.
  Pushes of `main`/`development` should have run `ci.yml` and did not.

## Decisions

### Release pipeline (`.github/workflows/release.yml`, rewritten)

- **Trigger:** `push` to `main`, plus `workflow_dispatch`. Concurrency is one group with no
  cancelling, as DevHub has, so a cancelled run can never leave a tag without assets. The tag
  trigger goes away: the pipeline creates the tags now, and a tag pushed with `GITHUB_TOKEN` does
  not start workflows anyway.
- **Version:** semantic-release with the `conventionalcommits` preset, as DevHub does: a `version`
  job runs it with `--dry-run` to compute the next version, the build jobs use it, and the `release`
  job runs it for real. `fix:` bumps patch, `feat:` bumps minor, and a breaking change bumps major.
  "No relevant changes" skips the build and release jobs without failing. Any other failure to
  compute a version fails the run, so a build is never mislabelled. Branch `main` only, tag format
  `v${version}`, starting from `v0.6.2`. Config lives in `.releaserc.json` at the repo root. Read
  DevHub's `version` job for the pitfalls already paid for: check out the branch tip rather than the
  event SHA, use `fetch-depth: 0`, and tell "nothing to release" apart from "semantic-release
  broke".
- **The version is stamped, not committed** *(my call, the owner approved)*. Each build job writes
  the computed version into the workspace `Cargo.toml` before building, and nothing is committed
  back. A bot commit on `main` would leave `development` behind and break the fast-forward
  promotion. Consequence: the repo's `Cargo.toml` stops being the released version. Leave it as is;
  local builds report it.
- **Assets** (published, never a draft):
  - macOS: the DMG, as today, for manual download, plus a zip of `wai.app` made with
    `ditto -c -k --sequesterRsrc --keepParent` for the updater (zip alone breaks bundle symlinks).
    The name must include the architecture.
  - Windows: the NSIS `-setup.exe`, as today. The updater uses it directly.
  - `SHA256SUMS.txt` covering every asset.
- **Notes:** semantic-release's generated notes, then a new `.github/release-notes.md` appended,
  as DevHub does: unsigned binaries; on macOS install to `~/Applications` and open the first time
  with right-click → Open; on Windows run the installer and pass SmartScreen with More info → Run
  anyway; updates are automatic after that.
- **Tests gate the release** *(my call)*: the release builds only after `ci.yml` passes on the same
  commit, on both systems. Make `ci.yml` callable (`workflow_call`) and have the release workflow
  `needs` it. With updates on by default, a build that fails its tests would otherwise reach every
  install at the next launch. `ci.yml` has never run, so the first gated run is also the first time
  the suite runs on a Windows runner. Fix what it finds before `v0.7.0`; do not drop the gate to
  ship.
- The existing optional signing steps stay as they are.
- Rewrite the release section of `docs/building.md` to describe the new flow.
- **Hard-coded versions in docs** *(my call)*: the manual bump also edited `README.md`
  (`**Status:** X`), `docs/extensions.md` ("implemented by wai X", the `"wai_version": "X"` example)
  and `docs/user-guide.md` ("describes wai X", the top-bar example). Since nothing bumps them now,
  make them version-free: point at the latest release, and use an obviously illustrative version in
  the examples. The real `wai_version` sent to extensions already comes from `CARGO_PKG_VERSION`,
  so it gets the stamped version.
- **`CLAUDE.md`'s "Release and install on this Mac"** describes the manual bump and says there is
  no remote. Replace it: releasing is promoting `development` to `main` and pushing, and nothing is
  bumped by hand. Installing on this Mac is the release's self-update, or a manual install of a
  local build into `~/Applications/wai.app` with the same rsync rules for testing unreleased work.
  Keep the rules that still hold (never kill the running wai, never `rm -rf` the bundle,
  `cargo clean` afterwards).

### Self-update in wai

- **Defaults:** `UpdateSettings` defaults to `check = true`, `repo = Some("Guilbritto/wai")`.
  Settings → Updates keeps the switch so anyone can turn updates off. The user-settings-only rule
  stays.
- **Only the built-in repository installs** *(my call)*: wai downloads and installs only from
  `Guilbritto/wai`, a constant in the code. If `updates.repo` names any other repository, wai keeps
  today's behaviour for it: a notification with **Open release page**, nothing downloaded. Without
  this, anything that can write `~/.config/wai/settings.toml` (an agent with file access included)
  would choose the code wai runs at the next **Restart now**, and the checksum would not help,
  because `SHA256SUMS.txt` comes from the same repository. This is the same reason wai asks for
  approval before it runs an agent command (`crates/wai-settings/src/approve.rs`).
- **Flow, once per launch, off the main thread:**
  1. GET latest release (as today) and also read its `assets`.
  2. If it is newer *and* this build can update itself: download the platform asset and
     `SHA256SUMS.txt`, verify the SHA-256, and stage the file in the user cache dir. Hash the
     staged file again right before applying it *(my call)*, because it sits on disk until the
     click. Downloads are
     HTTPS only, with a size cap, and redirects to GitHub's asset hosts are followed. hyper does not
     follow redirects today.
  3. Show a notification "wai X is ready" with **Restart now**.
  4. **Restart now** runs the normal quit path first, so it asks about unsaved edits and working
     sessions. Then it applies the update and relaunches.
- **When this build cannot update itself** (it is not an installed bundle/installer copy, or its
  folder is not writable), behave as today: notify with **Open release page**, and have Settings →
  Updates say why in one sentence that names the fix. This is DevHub's `Blocker()` pattern.
- **Integrity** *(my call)*: verifying against `SHA256SUMS.txt` catches corruption and truncation.
  It does not protect against a compromised repository; the trust is the same as a manual download,
  which is the same position DevHub takes (it leaves the checksum as a TODO; wai does it).
- **macOS apply:** port DevHub's `apply_darwin.go`. Extract with `ditto -x -k` into a staging dir
  next to the target. Rename `wai.app` → `wai.app.old`, move the new bundle into place, and restore
  the old one if that move fails. Relaunch with `open -n` once this process has exited (so the new
  one never reads state the old one is still saving). `wai.app.old` stays until the next update
  replaces it, as in DevHub; until then it is the rollback. Port `relocate.go` too: an installed copy in
  `/Applications` moves itself to `~/Applications` once, then tells the user it moved (or why it
  could not). That one move needs the Jamf admin active, as DevHub's notes explain. A file wai
  downloads itself carries no quarantine mark, so Gatekeeper does not prompt again.
- **Windows apply:** run the downloaded `-setup.exe` in silent mode (`/S`), detached, then exit so
  `wai.exe` is not locked, and relaunch wai once the installer finishes. An exe that is not in the
  installer's install location cannot update itself. Windows ships with the same self-update as
  macOS (the owner's decision). The owner tests it on a real Windows machine through an agent running
  there, following `docs/testing/windows-self-update.md`. Keep that checklist in step with the
  implementation: fix the *(confirm)* paths, and add any behaviour the implementation adds.
- **Local builds** (`cargo run`, anything that is not an installed bundle/installer copy) never
  update themselves. A locally packaged `wai.app` reports the repo's `Cargo.toml` version, so it
  will be *offered* the release, but nothing is applied without the click *(my call)*.
- **Settings → Updates:** shows the running version, the switch, the existing Repository field, a
  **Check for updates** button, and the state: up to date / downloading / ready (**Restart now**) /
  the error / the blocker sentence. Fix the "once a day" blurb.

### Known costs to state in the notes

- The binaries are not signed with a Developer ID, so every update is a new app to macOS. macOS may
  ask again for folder access (Documents, Desktop) after each update. DevHub has the same cost; a
  Developer ID removes it.

## Rollout

1. **First, find out why Actions never ran on `Guilbritto/wai` and fix it.** Nothing above works
   until a push to `main` runs a workflow. Lead: the repo was created and pushed 18 s later in one
   push of every branch and tag, and GitHub creates no push events for large batches of tags; push
   something trivial and watch for the run. Otherwise check the account's Actions/billing state. The
   `Guilbritto` token has the `workflow` scope; use it per command (`GH_TOKEN=$(gh auth token --user
   Guilbritto)`), never by switching the active `gh` account. Report what it was.
2. Land this work on `development`, then promote to `main`. Its commits are `feat:`, so the first
   automatic release is `v0.7.0`.
3. Current installs (0.6.2) do not have the new updater. *(My call: DevHub's route, so the move out
   of `/Applications` runs on a real machine.)* The owner installs `v0.7.0` by hand once, over the
   existing `/Applications/wai.app`, with the rsync rules in `CLAUDE.md`. The bundle is owned by the
   user, so this needs no admin. Installing into `~/Applications` instead would leave a second wai
   with the same bundle id that Spotlight or the Dock may keep opening.
4. On Windows, the owner's agent runs `docs/testing/windows-self-update.md` once `v0.7.0` and
   `v0.7.1` are published, and runs its section 3 after the next release. Fix any FAIL before
   relying on Windows updates.
5. Check the cycle end to end with a real second release: a small `fix:` reaching `main`. With the
   Jamf admin on, **Restart now** on the installed 0.7.0 should download, swap, move wai to
   `~/Applications`, relaunch as 0.7.1 and say it moved. A third release then has to install with
   no admin at all and leave `wai.app.old` behind.

## Constraints

- The repo is pushed as `Guilbritto` through the repo-local credential helper. The active `gh`
  account is `guilherme-brito-way`; do not switch it. The workflows use `GITHUB_TOKEN` only.
- The Mac's disk is nearly full: run `cargo clean` after heavy builds. Never `cargo run` with a fake
  `HOME`.
- Do not `rm -rf /Applications/wai.app` while testing; use `ditto` over it (see the gotcha in the
  project notes). Never kill the running wai: the session doing the work may be inside it.
- Follow the repo's comment style (comments explain why, and name rulings when they implement
  one).

## Left to the builder

- semantic-release config details, and how the version is written into `Cargo.toml`/`Cargo.lock` in
  CI.
- Exact asset names (keep the architecture in the macOS zip name), the cache dir layout, and the
  size cap.
- How `update.rs` is split (for example per-platform apply modules) and how its tests are laid out.
  Tests follow the existing pattern of injecting the network and using temp dirs for the swap.
- The Windows relaunch mechanism after the silent install (check what cargo-packager's NSIS
  template supports before writing a helper).
- UI copy and layout inside the existing Updates section and notification components.
