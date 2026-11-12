# wai release pipeline and self-update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A push to `main` publishes a downloadable GitHub release with a computed version, and an installed wai downloads, verifies and installs it after a **Restart now** click.

**Architecture:** `release.yml` runs `ci.yml` as a gate, then semantic-release computes the version, both platforms build with it stamped into `Cargo.toml`, and semantic-release publishes the release with the assets. In the app, `update.rs` grows from "check and notify" into check → download → verify → stage. `update/macos.rs` and `update/windows.rs` hold the per-platform install. The workspace owns the update state, shows it in Settings → Updates, and runs the install through its existing quit path.

**Tech Stack:** Rust 1.99.1 (GPUI via `gpui-kit` 0.9.0, hyper 1 + hyper-rustls, `sha2` 0.10.9 already in `Cargo.lock`), GitHub Actions, semantic-release 24 with `conventional-changelog-conventionalcommits` 8, cargo-packager 0.11.8 (NSIS supports `/S` and `/R`).

**Spec:** `docs/superpowers/specs/2026-11-12-wai-release-and-auto-update-design.md`. Reference implementation: DevHub in `/Users/guilherme.brito/Project/waycarbon-ecosystem-local` (`.github/workflows/release-devhub.yml`, `devhub/internal/service/selfupdate/apply_darwin.go`, `devhub/relocate.go`).

## Global Constraints

- Work in the worktree `.claude/worktrees/release-auto-update`, on branch `feat/release-auto-update` from `development`. Other sessions commit to `development` in parallel; never commit their untracked plan files.
- Only `Guilbritto/wai` installs: `pub const DEFAULT_REPO: &str = "Guilbritto/wai";` in `wai-settings`, compared case-insensitively. Any other `updates.repo` only notifies (**Open release page**).
- `UpdateSettings::default()` is `check = true`, `repo = Some(DEFAULT_REPO)`. Project `.wai/settings.toml` still cannot set `updates`.
- Nothing restarts without the user's click. **Restart now** goes through `Workspace::leave` (unsaved edits dialog, then the busy-sessions confirm).
- Downloads: HTTPS only. Redirects are followed only to `github.com`, `objects.githubusercontent.com` and `release-assets.githubusercontent.com`, at most 5 hops. Cap: 512 MB. The SHA-256 must match `SHA256SUMS.txt` at download time and again right before installing.
- Asset names: `wai_<v>_aarch64.dmg` (manual), `wai_<v>_aarch64.app.zip` (macOS updater), `wai_<v>_x64-setup.exe` (Windows), `SHA256SUMS.txt`.
- Staging dir: `dirs::cache_dir()/wai-updates/` (macOS `~/Library/Caches/wai-updates`, Windows `%LOCALAPPDATA%\wai-updates`). It is emptied before each download.
- macOS: the swap keeps `wai.app.old` until the next update replaces it. Relaunch happens only after this process has exited.
- Windows: `wai_<v>_x64-setup.exe /S /R`, started only after this process has exited. An exe with no `uninstall.exe` beside it cannot update itself.
- Launch-time failures are only logged (`eprintln!("wai: …")`). Settings → Updates shows them when the user asked via **Check now**.
- Commits: `feat:`/`fix:`/`docs:`/`ci:`/`test:`, no scope, no `Co-Authored-By`, no Jira refs (personal project).
- Never `cargo run` with a fake `HOME`. Never kill the running wai. `cargo clean` after heavy builds; the disk is nearly full.
- Push and anything that publishes a release need the owner's go-ahead at that step (Tasks 1 and 8).

## Review Focus

1. **A session working when Restart now is clicked:** expect the "still working" confirm with **Restart anyway**, and nothing installed on Cancel. Test in Task 6: `restart_asks_while_a_session_works`.
2. **The staged file changes between download and click** (truncated download, disk trouble, tampering): expect the install to be refused with the reason shown, wai to stay open, and the status to become `Failed`. Test in Task 3: `staged_file_is_hashed_again`.
3. **A release without this platform's asset or without `SHA256SUMS.txt`:** expect no crash, no download, and today's notice with **Open release page**. Test in Task 3: `release_without_asset_only_notifies`.
4. **`updates.repo` set to a case variant of the default (`guilbritto/WAI`), or cleared:** a case variant still installs. Cleared falls back to the default. Test in Task 2: `default_repo_survives_clearing`; test in Task 3: `only_the_default_repo_installs`.
5. **Relocation target already holds something that is not wai** (`~/Applications/wai.app` with another bundle id): expect wai to stay where it is, with a notice saying why. Test in Task 5: `relocate_refuses_a_foreign_bundle`.

---

### Task 1: Worktree, docs commit, and find out why Actions never ran

**Files:**
- Move: `docs/superpowers/specs/2026-11-12-wai-release-and-auto-update-design.md`, `docs/testing/windows-self-update.md` and this plan, untracked in the main tree, into the worktree. Use `mv`, so later merges do not trip on untracked copies.

- [ ] **Step 1:** `git -C /Users/guilherme.brito/Project/Guilherme/wai worktree add .claude/worktrees/release-auto-update -b feat/release-auto-update development`. Then `mv` the three files to the same paths inside the worktree.
- [ ] **Step 2:** Commit them: `docs: design and plan for automatic releases and self-update`.
- [ ] **Step 3: Probe Actions (owner go-ahead first).** On `development`, merge this branch (docs only), then `git push origin development`. This also publishes whatever other sessions already merged into `development`. Watch with `GH_TOKEN=$(gh auth token --user Guilbritto) gh run list -R Guilbritto/wai -L 5`; expect a `ci` run within 2 minutes. If none appears, check `gh api repos/Guilbritto/wai/actions/permissions` and the account's Actions/billing state with the same token. Report the cause to the owner. Never switch the active `gh` account.
- [ ] **Step 4:** If `ci` runs and fails, record which jobs fail and on which OS. Task 8 fixes them before the first release.

### Task 2: Settings defaults and the repository rule

**Files:**
- Modify: `crates/wai-settings/src/lib.rs:87-98` (`UpdateSettings`), the tests around `:632-665`
- Modify: `crates/wai/src/settings_view.rs:95` (Updates blurb)

**Interfaces:**
- Produces: `pub const DEFAULT_REPO: &str = "Guilbritto/wai";` and `impl Default for UpdateSettings` (`check: true, repo: Some(DEFAULT_REPO.into())`). Remove `Default` from the derive.

- [ ] **Step 1: Failing tests.** In `project_cannot_set_updates`, replace the default assertions with `assert!(UpdateSettings::default().check && UpdateSettings::default().repo.as_deref() == Some(DEFAULT_REPO));`. Keep the rest: the project's `evil/wai` is still ignored and the notice still says so. Add `default_repo_survives_clearing`: user file `[updates]\ncheck = true\n` (no repo) → `s.updates.repo.as_deref() == Some("Guilbritto/wai")`. Applying `Change::UpdatesRepo(None)` to a file with `repo = "x/y"`, then reloading, also gives `Some("Guilbritto/wai")`. A file with `check = false` gives `!s.updates.check`.
- [ ] **Step 2:** `cargo test -p wai-settings updates` → FAIL.
- [ ] **Step 3:** Implement. Notice text: "Ignored updates settings from {}: only user settings can change updates." Update its test.
- [ ] **Step 4:** `cargo test -p wai-settings` → PASS.
- [ ] **Step 5:** Blurb: "wai checks for a new release each time it opens, downloads it in the background and installs it when you click Restart now." In `crates/wai/src/update.rs` tests, `disabled_means_no_request` uses `UpdateSettings::default()` as "off". Make that an explicit `check: false`. `cargo build -p wai`.
- [ ] **Step 6:** Commit `feat: turn update checks on by default for Guilbritto/wai`.

### Task 3: Check, download, verify, stage (`update.rs` + `update/download.rs`)

**Files:**
- Modify: `crates/wai/src/update.rs` (declare `mod download; #[cfg(target_os = "macos")] pub mod macos; #[cfg(windows)] pub mod windows;`)
- Create: `crates/wai/src/update/download.rs`
- Modify: `crates/wai/Cargo.toml`: `sha2 = "0.10.9"` (comment: already in `Cargo.lock`, `cargo tree -i sha2@0.10.9` names who brings it); add tokio feature `fs` and `io-util` only if the streaming write needs them.

**Interfaces:**
- Consumes: `wai_settings::{UpdateSettings, DEFAULT_REPO}`.
- Produces:
  - `pub struct Asset { pub name: String, pub url: String, pub size: u64 }` (from `assets[].name`, `.browser_download_url`, `.size`), and `Release { tag, page, assets: Vec<Asset> }`. `text()` is unchanged.
  - `pub struct Staged { pub tag: String, pub file: PathBuf, pub sha256: String }`, with `Staged::text()` returning `"wai <version> is ready"`.
  - `pub enum Outcome { Off, UpToDate, Available { release: Release, why: Option<String> }, Ready(Staged) }`. `Available` means newer but this copy will not install it; `why` is the sentence Settings shows (the blocker, the other repository, or the missing asset).
  - `pub fn run(settings: &UpdateSettings, current: &str, blocker: Option<String>, cache: &Path, fetch: impl FnOnce(&str) -> Fetched, download: impl Fn(&str, &Path) -> anyhow::Result<()>) -> anyhow::Result<Outcome>`. `check` stays as the first step inside it.
  - `pub fn verify(staged: &Staged) -> anyhow::Result<()>`: re-hashes `staged.file`. Err text: "the downloaded update changed on disk; check for updates again".
  - `pub fn blocker() -> Option<String>`: dispatches to `macos::blocker`/`windows::blocker`; on other OSes "wai updates itself only on macOS and Windows."
  - `pub fn cache_dir() -> PathBuf` (`dirs::cache_dir()/wai-updates`).
  - In `download.rs`: `pub(super) fn download(url: &str, dest: &Path) -> anyhow::Result<()>` (real network, 512 MB cap, redirects); `pub(super) fn allowed(url: &str) -> bool`; `pub(super) fn sha256_file(path: &Path) -> anyhow::Result<String>` (lowercase hex); `pub(super) fn expected_sum(sums: &str, name: &str) -> Option<String>` (accepts `hash  name` and `hash *name`, lowercases).
  - `const ASSET_SUFFIX: &str` = `"_aarch64.app.zip"` on macOS, `"_x64-setup.exe"` on Windows, `""` elsewhere.

- [ ] **Step 1: Failing tests** (inject `fetch`/`download`; files in `tempfile` dirs):
  - `parses_assets`: a body with two assets yields both, with name/url/size. A body with no `assets` key parses with an empty list.
  - `only_the_default_repo_installs`: repo `"other/wai"` with a newer release returns `Available { why: Some(w), .. }`, where `w` contains "other/wai" and `download` is never called. Repo `"guilbritto/WAI"` reaches `download`.
  - `release_without_asset_only_notifies`: with no `ASSET_SUFFIX` asset, or no `SHA256SUMS.txt`, the result is `Available { why: Some(_) }` and no download (guard the test with `#[cfg(any(target_os = "macos", windows))]`).
  - `blocked_copy_only_notifies`: `blocker = Some("x".into())` gives `Available { why: Some("x") }` and no download.
  - `stages_a_verified_asset`: `download` writes known bytes to the asset path and a sums file with the right hash. The result is `Ready(Staged)` with `file` inside `cache` and `sha256` equal to the known hex. Files left in `cache` beforehand are gone.
  - `rejects_a_wrong_checksum`: the sums line has a different hash → `Err`, whose `{:#}` contains "checksum", and the asset file is removed.
  - `staged_file_is_hashed_again`: `verify` passes on the staged file, then fails after one byte changes, and fails after truncation.
  - `expected_sum_formats`: `"ABC  wai_1_x64-setup.exe\n"` gives `Some("abc")`; `"abc *wai_1_x64-setup.exe"` gives `Some("abc")`; a different name gives `None`; `"abc  wai_1_x64-setup.exe.sig"` for `wai_1_x64-setup.exe` gives `None`.
  - `allowed_hosts`: `https://github.com/Guilbritto/wai/releases/download/v1/x`, `https://objects.githubusercontent.com/…` and `https://release-assets.githubusercontent.com/…` → true. `http://github.com/…`, `https://github.com.evil.example/…`, `https://evil.example/?github.com` and `https://user@github.com/…` → false.
- [ ] **Step 2:** `cargo test -p wai update::` → FAIL.
- [ ] **Step 3:** Implement. `download` reuses `get`'s connector setup. Follow a 301/302/303/307/308 `Location` only when `allowed`. Stream to `dest` with a running byte count, failing past 512 MB. Total timeout 10 min; the 5 s `TIMEOUT` stays for the API call. On any error, remove the partial file. Update the module doc comment: wai now downloads, and why only `DEFAULT_REPO` installs.
- [ ] **Step 4:** `cargo test -p wai update::` → PASS; `cargo clippy -p wai --all-targets -- -D warnings`.
- [ ] **Step 5:** Commit `feat: download and verify wai updates`.

### Task 4: Windows install (`update/windows.rs`)

**Files:**
- Create: `crates/wai/src/update/windows.rs` (`#[cfg(windows)]`). The pure string builder lives in `update.rs` so it is tested on every OS.

**Interfaces:**
- Produces:
  - `pub fn blocker(exe: &Path) -> Option<String>`: `None` when `exe.parent()/uninstall.exe` exists. Otherwise: "This copy of wai was not installed by its installer, so it can't update itself. Install wai from the release page and updates will install on their own."
  - `pub fn install_after_exit(setup: &Path, pid: u32) -> std::io::Result<()>`: spawns `powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command <installer_script(setup, pid)>` with `CREATE_NO_WINDOW | DETACHED_PROCESS` (`std::os::windows::process::CommandExt::creation_flags`).
  - In `update.rs`: `pub(crate) fn installer_script(setup: &Path, pid: u32) -> String` →
    `Wait-Process -Id <pid> -ErrorAction SilentlyContinue; Start-Process -FilePath '<setup>' -ArgumentList '/S','/R'`. Any `'` in the path is doubled.

- [ ] **Step 1: Failing tests:** `installer_script_quotes_the_path` (on all OSes): `C:\Users\o'neil\AppData\Local\wai-updates\wai_1_x64-setup.exe`, pid 42, gives the exact string above with `o''neil`. `windows_blocker` (`#[cfg(windows)]`): a temp dir with `wai.exe` only gives `Some(_)`; adding `uninstall.exe` gives `None`.
- [ ] **Step 2:** `cargo test -p wai installer_script` → FAIL. **Step 3:** Implement. **Step 4:** PASS. The `#[cfg(windows)]` code is compiled and tested only by the Windows CI in Task 8: cross-checking GPUI from the Mac needs the MSVC toolchain.
- [ ] **Step 5:** Commit `feat: install wai updates on Windows through the silent installer`.

### Task 5: macOS install and relocation (`update/macos.rs`)

**Files:**
- Create: `crates/wai/src/update/macos.rs` (`#[cfg(target_os = "macos")]`), porting DevHub's `apply_darwin.go` and `relocate.go`. Read those first; their comments record bugs already paid for.

**Interfaces:**
- Produces:
  - `pub fn bundle_root(exe: &Path) -> Result<PathBuf, String>`: `…/X.app/Contents/MacOS/<bin>` gives `X.app`. A path containing `/AppTranslocation/` gives Err "this copy is running from a temporary location; move wai to ~/Applications first". Anything else gives Err "this build is not running from an installed wai.app".
  - `pub fn blocker() -> Option<String>`: resolves `std::env::current_exe()` (canonicalized), then `bundle_root`, then checks the parent is writable (create and remove `.wai-write-check-<pid>` in it). It returns DevHub's `notWritable` sentences with "DevHub" replaced by "wai" (the `/Applications` one names Jamf Connect).
  - `pub fn install(zip: &Path, bundle: &Path) -> anyhow::Result<()>`: `ditto -x -k <zip> <parent>/.wai-update-<pid>`, expecting exactly one `.app`. Remove `<bundle>.old`, rename `bundle` to `bundle.old`, rename the new bundle to `bundle`; if that fails, rename `.old` back. Always remove the staging dir.
  - `pub fn relaunch_after_exit(bundle: &Path, pid: u32) -> std::io::Result<()>`: spawns `/bin/sh -c 'while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec open -n "$2"' sh <pid> <bundle>` with `process_group(0)`.
  - `pub fn relocate(bundle: &Path, system_apps: &Path, home_apps: &Path) -> anyhow::Result<Option<PathBuf>>`: DevHub's `relocate()`. It acts only when `bundle.parent() == system_apps`, `<bundle>.old` exists, and the parent is writable. The same-app check reads `CFBundleIdentifier` with `plutil -extract CFBundleIdentifier raw`. It returns the new path.
  - `pub enum Start { Continue(Option<String>), Exit }` and `pub fn on_start() -> Start`: called first in `main` (after `shell_path::import`). If `WAI_RELOCATED` is set, it removes the variable and returns `Continue(Some("wai moved to ~/Applications. It was in <from>, where updates needed an administrator; from now on they install on their own."))`. Otherwise it runs `relocate(current bundle, "/Applications", ~/Applications)`. On a move it runs `open -n --env WAI_RELOCATED=/Applications <dest>` (bounded at 30 s) and returns `Exit`. On Err it returns `Continue(Some("wai could not move to ~/Applications: <e>. Updates here still need an administrator."))`. If the bundle's version is not semver it does nothing (same rule as DevHub).

- [ ] **Step 1: Failing tests** (temp dirs; build fake bundles as `X.app/Contents/{MacOS/wai, Info.plist}` with an XML plist holding `CFBundleIdentifier`; make zips with `ditto -c -k --keepParent`):
  - `bundle_root_shapes`: `/Applications/wai.app/Contents/MacOS/wai` gives Ok; `/x/target/release/wai` gives Err; `/private/var/folders/x/AppTranslocation/y/wai.app/Contents/MacOS/wai` gives Err containing "temporary".
  - `install_swaps_and_keeps_old`: after `install`, `wai.app` holds the zip's marker file, `wai.app.old` holds the previous marker, and no `.wai-update-*` dir remains. A second `install` replaces `.old` with the first new build.
  - `install_rejects_a_zip_without_one_app`: a zip with two `.app` or none gives Err, and `wai.app` is unchanged.
  - `relocate_moves_after_an_update`: `sys/wai.app` and `sys/wai.app.old` with `home` empty → `Some(home/wai.app)`; `home/wai.app.old` exists; `sys` is empty.
  - `relocate_waits_for_an_update`: without `.old`, the result is `None` and nothing moves.
  - `relocate_refuses_a_foreign_bundle`: `home/wai.app` with `CFBundleIdentifier` `com.other` → Err containing "is not wai", and both bundles stay where they were.
- [ ] **Step 2:** `cargo test -p wai update::macos` → FAIL. **Step 3:** Implement. **Step 4:** PASS + clippy.
- [ ] **Step 5:** Commit `feat: install wai updates on macOS and move it out of /Applications`.

### Task 6: Wire it into the app (workspace, Settings, launch)

**Files:**
- Modify: `crates/wai/src/workspace.rs` (`Leave` at `:148`, `proceed` at `:1999`, new fields/methods), `crates/wai/src/app.rs:207` (replace `check_on_launch`), `crates/wai/src/main.rs` (call `on_start`), `crates/wai/src/settings_view.rs` (Updates rows, `SettingsData`, `SettingsEvent`)
- Modify: `crates/wai/src/update.rs`: remove `check_on_launch`.

**Interfaces:**
- Consumes: `update::{run, verify, blocker, cache_dir, Outcome, Staged, Release}`, `macos::{install, relaunch_after_exit, on_start, Start}`, `windows::install_after_exit`.
- Produces:
  - `pub enum UpdateStatus { Idle, Checking, UpToDate, Downloading, Ready(Staged), Available { release: Release, why: Option<String> }, Failed(String) }` in `update.rs` (`Clone`). `Workspace` gets the field `update: UpdateStatus` (starts `Idle`), and `SettingsData` gets `update: UpdateStatus`.
  - `Workspace::check_updates(&mut self, from_launch: bool, window: &mut Window, cx: &mut Context<Self>)`: ignored while `Checking`. It sets `Checking`, runs `update::run` on `background_spawn` inside `catch_unwind` (as `check_on_launch` did), stores the result and refreshes an open Settings. On launch it pushes a notification for `Ready` (**Restart now**, `autohide(false)`) or `Available` (**Open release page**) and only logs `Failed`. From Settings it shows everything in the status row and pushes nothing.
  - `Leave::Update` → `proceed`: with no busy session it calls `install_update`. Otherwise `confirm(still_working(busy), "Restarting ends their turns. End them anyway?", "Restart anyway", …)`.
  - `Workspace::install_update(&mut self, window, cx)`: requires `Ready(s)`, then runs `verify(&s)`. On macOS: `install(&s.file, &bundle)` then `relaunch_after_exit(&bundle, std::process::id())`. On Windows: `install_after_exit(&s.file, id())`. Then `cx.quit()`. Any Err sets `Failed(e)` and pushes `Notification::error`; wai stays open.
  - `SettingsEvent::CheckUpdates`, `SettingsEvent::RestartToUpdate` (the workspace maps the second to `leave(Leave::Update, …)`).
  - Settings → Updates rows (keep them in `ROWS` so search finds them): "Automatic updates" (the existing switch, renamed from "Check for updates"), "Version" (`env!("CARGO_PKG_VERSION")` · `WAI_BUILD`, plus the status line and one button: **Check now** (disabled while `Checking`/`Downloading`), **Restart now** when `Ready`, **Open release page** when `Available`), and "Repository" (unchanged). Status copy: "Up to date", "Checking…", "Downloading wai X…", "wai X is ready", "wai X is available" + `why`, or the error.
  - `app::run` gets `update_notice: Option<String>` and pushes it as `Notification::success` once the window opens. `main` returns early on `Start::Exit`.

- [ ] **Step 1: Failing tests** (`TestAppContext`, following `quit_asks_while_a_session_works` at `workspace.rs:5055`):
  - `restart_asks_while_a_session_works`: busy session + `Ready` status, `leave(Leave::Update)` opens the dialog with title `still_working(1)`; Cancel leaves the status `Ready` and the app running.
  - `restart_asks_about_unsaved_edits`: a dirty tab + `leave(Leave::Update)` opens "Save changes to …?".
  - `failed_install_keeps_wai_open`: `Ready(Staged)` pointing at a file whose hash does not match → `install_update` sets `Failed(msg)`, with `msg` containing "changed on disk", and does not quit.
  - `settings_shows_update_status`: `SettingsData.update = Available { why: Some("x") }` renders the "Version" row containing "x" and an **Open release page** button (find it by id `settings-update-action`).
- [ ] **Step 2:** `cargo test -p wai -- restart_ failed_install settings_shows_update` → FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5:** `cargo build -p wai && cargo clippy -p wai --all-targets -- -D warnings`. Commit `feat: offer Restart now for downloaded wai updates`.

### Task 7: Release pipeline and docs

**Files:**
- Create: `.releaserc.json`, `.github/release-notes.md`
- Modify: `.github/workflows/release.yml` (rewrite), `.github/workflows/ci.yml` (`on`)
- Modify: `docs/building.md` (release section), `CLAUDE.md` (release section), `README.md:17`, `docs/extensions.md:5,135`, `docs/user-guide.md:3,7` (and an "Updates" paragraph), `docs/testing/windows-self-update.md` (*(confirm)* paths)

**Interfaces:**
- `.releaserc.json`: `branches: ["main"]`, `tagFormat: "v${version}"`. Plugins: commit-analyzer and release-notes-generator with `preset: "conventionalcommits"`, and `@semantic-release/github` with `assets: [{"path": "dist/*"}]`, `successComment: false`, `failComment: false`, `releasedLabels: false`. No npm plugin.
- `ci.yml`: `on: push: branches: [development]`, `pull_request`, `workflow_call`. Drop `main`, because the release runs it there.
- `release.yml` jobs:
  - `ci` (`uses: ./.github/workflows/ci.yml`).
  - `version`, which needs `ci`: DevHub's `compute` script verbatim (semantic-release@24 `--dry-run`, "no relevant changes" → empty and skip, anything else → fail). It also outputs `sha=$(git rev-parse HEAD)`.
  - `macos` and `windows`, which need `version` and run only `if version != ''`. They check out `needs.version.outputs.sha`. A "Stamp version" step (bash on both) runs `perl -0pi -e 's/^version = "[^"]*"/version = "'"$V"'"/m' Cargo.toml && grep -qxF "version = \"$V\"" Cargo.toml`, replacing the tag check. The macOS job adds `ditto -c -k --sequesterRsrc --keepParent target/release/wai.app "target/release/wai_${V}_aarch64.app.zip"` and uploads `*.dmg` and `*.app.zip`. The signing steps are unchanged.
  - `release` (`contents: write`): check out the same sha with `fetch-depth: 0`, download the artifacts to `dist/`, write `SHA256SUMS.txt` inside `dist`, run `npx semantic-release`, then append `.github/release-notes.md` to the notes (DevHub's last step). If semantic-release refuses a detached HEAD, check out `main` and fail when `git rev-parse HEAD` differs from the version job's sha.
  - Concurrency `release` with no cancel; triggers `push: branches: [main]` and `workflow_dispatch`. No tag trigger.
- `.github/release-notes.md` (English): unsigned binaries. macOS: put `wai.app` in `~/Applications` (create it), not `/Applications`, and the first time use right-click → Open. Windows: run the setup; SmartScreen → More info → Run anyway; no admin. Updates install themselves after **Restart now**; on macOS the previous version stays as `wai.app.old`. Known cost: without a Developer ID, macOS may ask again for folder access (Documents, Desktop) after each update.
- Docs: replace hard-coded versions with "the latest release" and use `1.2.3` in the protocol and top-bar examples. `CLAUDE.md`: releasing is merging into `development`, fast-forwarding `main` and pushing both; the version and tag come from CI. Installing a local build for testing goes into `~/Applications/wai.app` with the existing rsync rules (never `rm -rf`, never kill the running wai, `cargo clean`). Remove "there is no remote".

- [ ] **Step 1:** Write the files. Check the YAML: `python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/*.yml`, then `actionlint` if it is installed (`brew list actionlint`).
- [ ] **Step 2:** Check the stamp on a copy: `cp Cargo.toml /tmp/C.toml && V=9.8.7 perl -0pi -e 's/^version = "[^"]*"/version = "'"9.8.7"'"/m' /tmp/C.toml && grep -c '9.8.7' /tmp/C.toml` → `1`, and `diff Cargo.toml /tmp/C.toml` shows only the version line.
- [ ] **Step 3:** `grep -rn "0\.6\.2" README.md docs/extensions.md docs/user-guide.md CLAUDE.md` prints nothing.
- [ ] **Step 4:** Commit `ci: publish a release on every push to main` and `docs: describe automatic releases and updates` (two commits).

### Task 8: Whole-branch check, merge, first releases (owner-gated)

- [ ] **Step 1:** In the worktree (background shell, `export PATH="$HOME/.cargo/bin:$PATH"`): `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace`. All pass. `new_session_appears_in_open_list` is known to be flaky under load; rerun it alone before calling it a failure.
- [ ] **Step 2:** Final review of the whole branch (fresh reviewer on the most capable model) against the spec and the Review Focus. Fix what it confirms.
- [ ] **Step 3:** Merge into `development` (`git merge --no-ff feat/release-auto-update`) after `git log --oneline -5 development` shows what other sessions landed. Then rerun `cargo test --workspace`.
- [ ] **Step 4 (owner go-ahead):** Push `development` and confirm `ci` passes on macOS and Windows. Fix any failure on a branch, merged the same way. Keep the gate even if fixing takes time.
- [ ] **Step 5 (owner go-ahead):** Fast-forward `main` (`git branch -f main development` only when `main` is an ancestor) and push it. Expect the `release` run to publish `v0.7.0` with the four assets. Download the DMG and check `CFBundleShortVersionString` is `0.7.0`.
- [ ] **Step 6:** The owner installs 0.7.0 over `/Applications/wai.app` with the `CLAUDE.md` rsync rules. Then a small `fix:` reaches `main` → `v0.7.1`. With the Jamf admin on, **Restart now** should swap, relocate to `~/Applications` and show the moved notice. The owner's Windows agent then runs `docs/testing/windows-self-update.md`.
- [ ] **Step 7:** `cargo clean` in the worktree; remove the worktree once merged.
