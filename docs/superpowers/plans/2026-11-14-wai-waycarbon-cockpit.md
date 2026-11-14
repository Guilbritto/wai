# wai-waycarbon cockpit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extension API 1.5 (required Claude plugins, SVG panel icons, a `tabs` node), the help-requests relay, and wai-waycarbon 0.2.0 (Work + Help tabs, bottom-bar counter).

**Architecture:** General additions to `wai-ext` (manifest, panel tree) and `wai` (consent/install, rail, panel rendering) first; then a stdlib-only Go relay (`extensions/wai-waycarbon/relay`, SSE + POST, JSON-file store) and a shared `helpapi` package; then the extension rewritten around one panel with two tabs, driving the user's `gh`, `jira` and `claude` CLIs with argv only.

**Tech Stack:** Rust 2024, GPUI (`gpui-kit` 0.9, gpui-fast `svg().external_path`), serde/toml; Go 1.22 stdlib only (`net/http`, `net/http/httptest`, `crypto/rsa`, `os/exec`).

**Spec:** `docs/superpowers/specs/2026-11-14-wai-waycarbon-extension-design.md` (and `2026-11-14-help-requests-design.md` for everything about help requests the spec does not change). Read both before any task.

## Global Constraints

- Work only in the worktree `/Users/guilherme.brito/Project/Guilherme/wai/.claude/worktrees/waycarbon`, branch `feat/waycarbon-cockpit`. Never push, never release, never touch `main`/`development`.
- Cargo: `export PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_DIR=/Users/guilherme.brito/Project/Guilherme/wai/target` (shared target: the disk is nearly full). Never run `cargo clean`, `cargo packager`, or install anything into `~/Applications`. Never kill a running `wai`.
- Rust gate per task: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p <crates touched>`.
- Go gate per task (in `extensions/wai-waycarbon`): `gofmt -l . ` prints nothing, `go vet ./... && go test ./...`. Go 1.22 language level, **no third-party modules** (go.mod stays without `require`).
- Every program run with argv only (`exec.Command`/`std::process::Command`), never a shell. stdin null for CLIs that could prompt.
- Extension API string becomes `"1.5"`.
- Commits: `feat:` / `fix:` / `refactor:` / `docs:` / `test:`, no scope, no `Co-Authored-By`, imperative, first line < 72 chars. `git add` explicit paths only (never `.playwright-mcp/`).
- Copy fixed by the spec, verbatim:
  - consent line `Install Claude Code plugin <name> from <marketplace>`
  - `Installed Claude Code plugin <name>` / `Could not install Claude Code plugin <name>: <reason>`
  - `The WayCarbon Claude plugin is not installed, so the /waycarbon commands will not work.`
  - `Help requests need the WayCarbon relay, which is not set up yet.`
  - `Request from @<login> via wai. A colleague wrote this; check it before running anything.`
  - `Do not paste secrets.`
  - `@<login> replied to your request: <text>`
  - draft suggestion: `Write a request for a colleague who has the rights I lack: what to run, where (repo, branch, environment), why, and what to send back. Plain text.`
- Text from another person (request, reply, decline reason, PR titles, card summaries) is rendered only in `text`, `card` `title`/`subtitle` or list fields — never `markdown`.
- Keep doc style: `docs/extensions.md` is plain, terse, tables for fields.

## Review Focus

1. A required plugin whose install hangs (private repo, no credentials): `GIT_TERMINAL_PROMPT=0` and the 5-minute kill must end it, and the extension must still be installed and enabled — pinned in Task 4.
2. The Work tab when `gh` returns partial data (a PR with no commits, `statusCheckRollup: null`, `mergeable: UNKNOWN`, a deleted repository): no panic, the PR lands in "Waiting on others" — pinned in Task 9.
3. A relay request from someone who left the org, or acting on a request that is not theirs: `403`, nothing changes — pinned in Task 7.
4. The relay restarting while requests wait (deploy): waiting requests survive and are delivered on the next stream — pinned in Task 5 (reload) and Task 7 (sync on connect).
5. Switching tabs while typing a help request: the typed text stays — pinned in Task 2.

---

### Task 1: Manifest — `requires.claude_plugins`, SVG icon paths, API 1.5

**Files:**
- Modify: `crates/wai-ext/src/manifest.rs`
- Modify: `crates/wai-ext/tests/manifest.rs`, `crates/wai-ext/tests/fixtures/full.toml`

**Interfaces:**
- Produces:
  - `pub const API: &str = "1.5";`
  - `pub struct Manifest { …, pub requires: Requires }`
  - `#[derive(Debug, Clone, Default, PartialEq)] pub struct Requires { pub claude_plugins: Vec<RequiredPlugin> }`
  - `#[derive(Debug, Clone, PartialEq, Deserialize)] #[serde(deny_unknown_fields)] pub struct RequiredPlugin { pub name: String, pub marketplace: String }`
  - `pub const MAX_REQUIRED_PLUGINS: usize = 5;`
  - `impl PanelSpec { pub fn svg_icon(&self) -> Option<&Path> }` — `Some` when `icon` ends in `.svg` and passes `relative_inside`.
  - `pub fn plugin_name_ok(name: &str) -> bool` — `[A-Za-z0-9._-]`, 1–214 chars, no leading `-` (no `@`, no `/`: a bare name).
  - `pub fn marketplace_ok(source: &str) -> bool` — `owner/repo` (each side `[A-Za-z0-9._-]+`, neither starting with `-`), or `https://` + ≤ 2,048 chars with no whitespace or control characters.

- [ ] **Step 1: Write failing tests** in `crates/wai-ext/tests/manifest.rs`:

```rust
#[test]
fn required_plugins_parse() {
    let m = parse(&format!("{}\n[[requires.claude_plugins]]\nname = \"waycarbon\"\nmarketplace = \"waycarbon/claude-code\"\n", with("x", "bin/x", "\"process\""))).unwrap();
    assert_eq!(m.requires.claude_plugins, [RequiredPlugin { name: "waycarbon".into(), marketplace: "waycarbon/claude-code".into() }]);
}

#[test]
fn required_plugins_are_checked() {
    for (name, market) in [("-x", "a/b"), ("a b", "a/b"), ("a@b", "a/b"), ("ok", "-a/b"), ("ok", "a"), ("ok", "http://x/y"), ("ok", "https://x/ y")] {
        let toml = format!("id = \"x\"\nname = \"X\"\nversion = \"1\"\n[[requires.claude_plugins]]\nname = {name:?}\nmarketplace = {market:?}\n");
        assert!(parse(&toml).is_err(), "{name} {market}");
    }
    let six = "[[requires.claude_plugins]]\nname = \"p\"\nmarketplace = \"a/b\"\n".repeat(6);
    assert!(parse(&format!("id = \"x\"\nname = \"X\"\nversion = \"1\"\n{six}")).is_err());
    assert!(parse("id = \"x\"\nname = \"X\"\nversion = \"1\"\n[requires]\nother = 1\n").is_err());
}

#[test]
fn svg_icon_paths() {
    let spec = |icon: &str| PanelSpec { id: "p".into(), title: "P".into(), icon: Some(icon.into()), location: Default::default() };
    assert_eq!(spec("icon.svg").svg_icon(), Some(Path::new("icon.svg")));
    assert_eq!(spec("assets/w.svg").svg_icon(), Some(Path::new("assets/w.svg")));
    assert_eq!(spec("../w.svg").svg_icon(), None);
    assert_eq!(spec("/tmp/w.svg").svg_icon(), None);
    assert_eq!(spec("database").svg_icon(), None);
}
```

Also extend `api_version_is_checked_first` so `api = "1.5"` is accepted and `"1.6"` refused with `TooOld`, and add a `[[requires.claude_plugins]]` entry to `fixtures/full.toml` asserted in `parses_full_manifest`.

- [ ] **Step 2: Run** `cargo test -p wai-ext --test manifest` — Expected: compile errors / FAIL.
- [ ] **Step 3: Implement** in `manifest.rs`: `RawManifest.requires: RawRequires` (`#[serde(default)]`, `deny_unknown_fields`, `claude_plugins: Vec<RequiredPlugin>`), validate each with `plugin_name_ok`/`marketplace_ok` (error `ManifestError::Unknown("invalid required Claude plugin <name> from <marketplace>")`) and the cap; bump `API`; add `svg_icon`.
- [ ] **Step 4: Run** `cargo test -p wai-ext` — Expected: PASS (including `first_party_manifests_validate`).
- [ ] **Step 5: Commit** `feat: let a manifest require Claude plugins and name an SVG icon`

---

### Task 2: The `tabs` panel node

**Files:**
- Modify: `crates/wai-ext/src/panel.rs` (enum, parser, `walk`, tests)
- Modify: `crates/wai/src/ext_panel.rs` (state, `form`, render, size, tests)
- Modify: `docs/extensions.md` (Layout table row + rules, API 1.5 note)

**Interfaces:**
- Produces: `Node::Tabs { id: String, value: Option<String>, tabs: Vec<Tab> }`, `pub struct Tab { pub id: String, pub title: Arc<str>, pub badge: Option<Arc<str>>, pub children: Vec<Node> }`, `pub const MAX_TABS: usize = 10;`

- [ ] **Step 1: Write failing parser tests** in `panel.rs` tests:
  - `tabs_parse`: `{"type":"tabs","id":"t","value":"help","tabs":[{"id":"work","title":"Work","badge":"3","children":[{"type":"text","text":"a"}]},{"id":"help","title":"Help","children":[]}]}` → `Node::Tabs` with two tabs, `value == Some("help")`, first badge `"3"`.
  - `broken_tabs_are_unsupported`: missing `id`; `tabs` not an array; empty `tabs`; 11 tabs; a tab without `title`; a tab without `children`; two tabs with the same id; tabs `id` equal to a button's id elsewhere in the tree → each `Node::Unsupported` (the duplicate-id case: the second taker).
  - `tabs_children_count_toward_limits`: a tabs node whose hidden tab holds `MAX_NODES` texts → `Err(TooLarge)`.
  - `walk` visits every tab's children (assert a text in the second tab is visited).
- [ ] **Step 2: Run** `cargo test -p wai-ext panel` — Expected: FAIL.
- [ ] **Step 3: Implement** the `"tabs"` arm in `Parser::kind` (the node id joins `self.ids` only when the node is otherwise whole, as `section` does; tab ids unique within the node; titles/badges via `line`), and `walk`.
- [ ] **Step 4: Write failing view tests** in `ext_panel.rs` tests (follow `fill_and_placeholder_follow_the_tree` for setup):
  - `tabs_switch_locally`: render tabs Work/Help; only Work's text is on screen (`debug_bounds`); click the tab element `panel-tab:t:help` → Help's text shows, Work's does not; no `PanelAction` emitted.
  - `tab_value_reaches_the_form`: after switching to Help, a button click's `values["t"] == "help"`.
  - `typed_text_survives_a_tab_switch` (Review Focus 5): type into an input in Help, switch to Work and back, re-render the same tree → the input still holds the text and `values` carries it.
  - `tabs_value_is_an_initial_value`: rendering with `value:"help"` selects Help; the user switches to Work; re-rendering with the same `value:"help"` keeps Work; rendering with `value:"work"` then `"help"` selects Help.
  - `fill_through_tabs`: a `fill` list inside the selected tab of a root-level tabs fills the panel (as the existing fill test asserts).
- [ ] **Step 5: Implement** in `ext_panel.rs`: `set_tree`'s walk treats `Node::Tabs { id, value, .. }` like a section (`initial = value.map(Value::from)`); `form` inserts the selected tab id; a `selected(id, tabs) -> &Tab` helper (stored value if it names a tab, else the first); render a tab bar (gpui_kit's `TabBar`/`Tab` from gpui-component if it fits the sidebar; else a row of pills styled like the rail items) with element ids `panel-tab:<tabs id>:<tab id>`, `Role::Tab`, `aria_selected`, badge as `Tag::secondary().small()`; then only the selected tab's children as a column, passing `At.column` through so `fill` works; `size` sums all tabs.
- [ ] **Step 6: Run** `cargo test -p wai ext_panel` and `cargo test -p wai-ext` — Expected: PASS.
- [ ] **Step 7: Docs**: add the `tabs` row to the Layout table and the bullets from the spec §1.3 to `docs/extensions.md`; say API 1.1–1.5 only add fields and methods; note `api = "1.5"` for `tabs`.
- [ ] **Step 8: Commit** `feat: add a tabs node to extension panels`

---

### Task 3: SVG panel icons in the rail and the pane header

**Files:**
- Modify: `crates/wai/src/ext_host.rs` (`Panel.icon`, resolution near line 364)
- Modify: `crates/wai/src/sidebar.rs` (`render_rail`, the `mark` match ~line 395)
- Modify: `crates/wai/src/workspace.rs` (pane header ~line 3176)
- Modify: `.github/workflows/extension-release.yml` (pack icon files)
- Modify: `docs/extensions.md` (`icon` paragraph under Panels)

**Interfaces:**
- Consumes: `PanelSpec::svg_icon` (Task 1).
- Produces: `pub enum PanelIcon { Lucide(Lucide), Svg(SharedString /* absolute path */) }`; `Panel.icon: Option<PanelIcon>`; `pub(crate) fn panel_icon(spec: &PanelSpec, ext_dir: &Path) -> Option<PanelIcon>`; `pub(crate) const MAX_SVG_ICON: u64 = 32 * 1024;`

- [ ] **Step 1: Write failing tests** (unit tests in `ext_host.rs`):
  - `panel_icon_resolves`: a temp ext dir with `icon.svg` (a 24×24 path) → `Some(PanelIcon::Svg(abs))`; `"database"` → `Some(PanelIcon::Lucide(_))`; `"nope"` → `None`; `missing.svg` → `None`; a 33 KB `big.svg` → `None`; `../x.svg` → `None`; `icon.svg` that is a symlink → `None` (use `symlink_metadata`).
  - A rail test (beside the existing sidebar/rail tests): an installed sample extension whose panel has `icon = "icon.svg"` shows its rail button (`tab-panel` 0) with no initials text (assert the initials string is not on screen).
- [ ] **Step 2: Run** `cargo test -p wai panel_icon` — Expected: FAIL.
- [ ] **Step 3: Implement** `panel_icon` (svg: `symlink_metadata` is a regular file ≤ `MAX_SVG_ICON`, else `None`; else `lucide(name)`), use it where `Panel` is built; in the rail and the pane header draw `PanelIcon::Svg(path)` as `svg().external_path(path).size(px(17.))` (15 in the header) with `.text_color(if on { selected } else { idle })` — a mask in the rail's colours, like Lucide icons.
- [ ] **Step 4: Run** `cargo test -p wai` (sidebar, ext_host, workspace tests) — Expected: PASS.
- [ ] **Step 5: Workflow**: in the manifest step's Python also print `icons=` the space-separated `panels[].icon` values that end in `.svg` and contain no `..` and do not start with `/`; in `pack()` copy each with `mkdir -p "$stage/$(dirname "$f")" && cp "$f" "$stage/$f"`. Docs: the `icon` paragraph gains the `.svg` path rule (≤ 32 KB, drawn as a mask in the activity bar's colours, initials when missing; packed by the shared workflow).
- [ ] **Step 6: Commit** `feat: draw an extension's own SVG icon in the activity bar`

---

### Task 4: Install required Claude plugins on consent

**Files:**
- Create: `crates/wai/src/claude_plugins.rs` (+ `mod claude_plugins;` in `main.rs`)
- Modify: `crates/wai/src/extensions_view.rs` (consent lines, `install`, plugin rows)
- Modify: `docs/extensions.md` (manifest table row, Installing and consent, Contributions, Security model)

**Interfaces:**
- Consumes: `Manifest.requires.claude_plugins: Vec<RequiredPlugin>` (Task 1); `wai_settings::find_program`.
- Produces:
  - `pub fn install_args(p: &RequiredPlugin) -> Vec<String>` → `["plugin","install",<name>,"--marketplace",<marketplace>,"--scope","user","--json"]` (never `-y`).
  - `pub fn terminal_line(p: &RequiredPlugin) -> String` → `claude plugin install <name> --marketplace <marketplace>`.
  - `pub fn installed(claude: &Path, cwd: &Path) -> Vec<String>` — ids from `claude plugin list --json` (empty on any failure).
  - `pub fn install(claude: &Path, p: &RequiredPlugin, cwd: &Path, timeout: Duration) -> Result<(), String>` — skips when `installed` has an id starting with `<name>@`; runs with stdin null, env `GIT_TERMINAL_PROMPT=0`, `current_dir(cwd)`; polls `try_wait` every 50 ms and `kill()`s at `timeout` (`Err("no answer in 5 minutes")`); non-zero exit → `Err(<last non-empty stderr line, else stdout's, clamped to 300 chars>)`.
  - `pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);`

- [ ] **Step 1: Write failing tests** in `claude_plugins.rs` (unix-only `#[cfg(unix)]` for the fake script; write executable scripts into a tempdir as `waycarbon.rs` does):
  - `install_runs_claude_with_argv_only`: fake `claude` appends `"$@"` and `$GIT_TERMINAL_PROMPT` to a log and prints `[]` for `plugin list --json`; `install` → `Ok`; log has `plugin install waycarbon --marketplace waycarbon/claude-code --scope user --json` and `0`; no `-y` anywhere.
  - `already_installed_is_skipped`: `plugin list --json` prints `[{"id":"waycarbon@waycarbon"}]` → `Ok`, the log has no `plugin install`.
  - `failure_reports_the_last_stderr_line`: install exits 1 with stderr `a\nPermission denied (publickey)\n` → `Err("Permission denied (publickey)")`.
  - `hung_install_is_killed` (Review Focus 1): fake install `sleep 30`; `install(.., Duration::from_millis(300))` returns `Err` within 5 s.
  - In `extensions_view.rs` tests: `consent_lists_required_plugins` — a sample manifest with `api = "1.5"` and one `[[requires.claude_plugins]]` → `consent: Install Claude Code plugin waycarbon from waycarbon/claude-code` is on screen; and the installed row shows `Install in terminal` for it, whose `InstallPlugin` event carries `claude plugin install waycarbon --marketplace waycarbon/claude-code`.
- [ ] **Step 2: Run** `cargo test -p wai claude_plugins consent_lists_required_plugins` — Expected: FAIL.
- [ ] **Step 3: Implement** `claude_plugins.rs`; in `consent()` push one line per required plugin (clamped with `clamp(.., NAME_CHARS)`); in `install()` on `Ok`, take the manifest's required plugins and, if any, `cx.spawn_in(window, …)` a background task: resolve `claude` with `find_program(Path::new("claude"))` (none → `Err("claude is not on PATH")`), run `install` for each with the config folder as cwd, then push `Notification::success("Installed Claude Code plugin <name>")` or `Notification::error("Could not install Claude Code plugin <name>: <reason>").autohide(false)` per plugin. The extension's own install result and enabled state never depend on it. In `row()`, list required plugins before suggested ones with the same `Install in terminal` button (emitting `InstallPlugin(terminal_line(p))`).
- [ ] **Step 4: Run** `cargo test -p wai` (extensions_view, claude_plugins) — Expected: PASS.
- [ ] **Step 5: Docs** (`docs/extensions.md`): manifest table row `requires.claude_plugins` (array of tables: `name`, `marketplace`; at most 5; API 1.5); "Installing and consent" gains the consent line and what happens after Install (command, `GIT_TERMINAL_PROMPT=0`, 5-minute limit, notification, never `-y`, the extension stays installed); Contributions "Claude plugins" bullet covers required ones' **Install in terminal**; Security model line becomes the spec's §1.1 wording.
- [ ] **Step 6: Commit** `feat: install an extension's required Claude plugins after consent`

---

### Task 5: Help API types and the relay store

**Files:**
- Create: `extensions/wai-waycarbon/helpapi/helpapi.go`
- Create: `extensions/wai-waycarbon/relay/store.go`, `extensions/wai-waycarbon/relay/store_test.go`

**Interfaces:**
- Produces (`package helpapi`, import `wai-waycarbon/helpapi`):
  - `type State string` with `Waiting, Accepted, Replied, Declined, Cancelled, Expired State = "waiting", "accepted", "replied", "declined", "cancelled", "expired"`
  - `type Project struct { Repo string `json:"repo,omitempty"`; Branch string `json:"branch,omitempty"` }`
  - `type Request struct { ID string; From, To string /* logins */; FromID, ToID int64; Project Project; Text string; State State; Reply, Reason string; Created, Updated time.Time }` with JSON tags `id, from, to, from_id, to_id, project, text, state, reply, reason, created, updated` (`reply`,`reason` omitempty)
  - `type Person struct { Login string `json:"login"`; Online bool `json:"online"` }`
  - `type Update struct { ID string; State State; Reply, Reason string }` (json `id,state,reply,reason`)
  - consts `MaxText = 8000`, `MaxBody = 64 << 10`, `PerHour = 20`, `WaitingPerRecipient = 10`, `StreamsPerUser = 4`, `AcceptWithin = 24 * time.Hour`, `Retention = 7 * 24 * time.Hour`, `TokenLife = 30 * 24 * time.Hour`
  - event names `EvSync = "sync"`, `EvNew = "request.new"`, `EvUpdated = "request.updated"`, `EvPeople = "people"`
- Produces (`relay`, package main):
  - `type User struct { ID int64; Login string; LastSeen time.Time }`
  - `type Store struct` with `func OpenStore(path string, now func() time.Time) (*Store, error)`, and methods:
    - `PutUser(id int64, login string)`; `UserByLogin(login string) (User, bool)` (case-insensitive); `Users() []User`
    - `AddToken(hash [32]byte, userID int64)` (expiry `now+TokenLife`); `TokenUser(hash [32]byte) (User, bool)` (false when expired); `DropToken(hash [32]byte)`
    - `Create(from User, to User, p helpapi.Project, text string) (helpapi.Request, error)` — errors `ErrTooLong`, `ErrRateLimited` (≥ 20 created by `from` in the last hour), `ErrRecipientFull` (≥ 10 waiting for `to`), `ErrSelf`
    - `Act(userID int64, id, action, text string) (helpapi.Request, error)` — `action` ∈ `cancel` (sender; waiting/accepted), `accept`, `decline` (recipient; waiting), `reply` (recipient; accepted; text ≤ MaxText). Errors `ErrNotFound` (also for a request the user is not part of), `ErrForbidden` (wrong side), `ErrBadState`, `ErrTooLong`
    - `Open(userID int64) []helpapi.Request` — requests of this user not past retention, any state (the client shows ended ones until retention)
    - `Sweep() (expired []helpapi.Request)` — waiting older than `AcceptWithin` → expired; ended (`replied|declined|cancelled|expired`) older than `Retention` since `Updated` → deleted
  - Persistence: the whole store as one JSON file, written to `<path>.tmp` (mode 0600) then renamed, on every change, under one mutex; `// ponytail: one JSON file rewritten per change; past a few thousand open requests, move to SQLite.`
  - IDs: 16 random bytes, hex.

- [ ] **Step 1: Write failing tests** in `store_test.go` with a fake clock:
  - `TestRequestLifecycle`: create → accept → reply; states and `Updated` move; reply text stored.
  - `TestOnlyTheRightSideActs`: recipient cannot cancel (`ErrForbidden`), sender cannot accept/reply (`ErrForbidden`), a third user gets `ErrNotFound`; replying before accept → `ErrBadState`.
  - `TestLimits`: 8,001-char text → `ErrTooLong`; 21st request in an hour → `ErrRateLimited` (and allowed again after the clock moves 61 min); 11th waiting for one recipient → `ErrRecipientFull`; to self → `ErrSelf`.
  - `TestExpiryAndRetention`: waiting at +24h01m → `Sweep` returns it expired; at +7d+24h01m it is gone from `Open`.
  - `TestTokens`: a token works, stops after 30 days, `DropToken` revokes.
  - `TestReloadKeepsWaitingRequests` (Review Focus 4): create two, reopen `OpenStore` on the same path → both present with their states; the file mode is 0600.
- [ ] **Step 2: Run** `go test ./relay/` — Expected: FAIL (no code).
- [ ] **Step 3: Implement** `helpapi.go` and `store.go`.
- [ ] **Step 4: Run** `go vet ./... && go test ./...` in `extensions/wai-waycarbon` — Expected: PASS.
- [ ] **Step 5: Commit** `feat: add the help requests relay store`

---

### Task 6: Relay sign-in through a GitHub App

**Files:**
- Create: `extensions/wai-waycarbon/relay/github.go`, `extensions/wai-waycarbon/relay/auth.go`, `extensions/wai-waycarbon/relay/auth_test.go`, `extensions/wai-waycarbon/relay/fakegithub_test.go`

**Interfaces:**
- Consumes: `Store` (Task 5).
- Produces:
  - `type Config struct { Addr, Data, Org, ClientID, AppID, InstallationID string; AppKey *rsa.PrivateKey; API, Web string }` and `func ConfigFromEnv() (Config, error)` reading `RELAY_ADDR` (default `:8080`), `RELAY_DATA`, `RELAY_ORG`, `RELAY_GITHUB_CLIENT_ID`, `RELAY_GITHUB_APP_ID`, `RELAY_GITHUB_APP_KEY` (PEM file path, PKCS#1 or PKCS#8), `RELAY_GITHUB_INSTALLATION_ID`, `RELAY_GITHUB_API` (default `https://api.github.com`), `RELAY_GITHUB_WEB` (default `https://github.com`); missing required → error naming the variable.
  - `type GitHub struct` with `NewGitHub(c Config, client *http.Client) *GitHub` and:
    - `DeviceCode(ctx) (DeviceStart, error)`; `type DeviceStart struct { DeviceCode, UserCode, VerificationURI string; Interval int }` (json `device_code,user_code,verification_uri,interval`)
    - `PollToken(ctx, deviceCode string) (token string, err error)` — `ErrPending` for `authorization_pending`/`slow_down`, other errors as is
    - `User(ctx, token string) (id int64, login string, err error)`
    - `IsMember(ctx, login string) (bool, error)` — installation token: RS256 JWT (`iat` now−60s, `exp` now+9m, `iss` AppID) → `POST {API}/app/installations/{id}/access_tokens`, cached until 1 minute before its `expires_at`; `GET {API}/orgs/{org}/members/{login}` → 204 true, 404 false.
  - `type Server struct { store *Store; gh *GitHub; cfg Config; now func() time.Time; hub *Hub /* Task 7 */ }`; handlers registered by `func (s *Server) Routes() http.Handler` (Task 7 adds the rest):
    - `POST /auth/device` → 200 `DeviceStart` (device_code included; it is the client's to poll with)
    - `POST /auth/device/poll` body `{"device_code"}` → `428 {"error":"pending"}`; `403 {"error":"Only members of <org> can use help requests"}`; `200 {"token","login"}` — on success `PutUser`, a fresh 32-byte token (base64url, no padding), `AddToken(sha256(token))`. The GitHub token is never stored or logged.
    - `POST /auth/signout` (Bearer) → `DropToken`, 204.
    - `func (s *Server) authed(r *http.Request) (User, bool)` — `Authorization: Bearer <token>` → `TokenUser(sha256)`.
  - Logs: `log.Printf` with GitHub ids/logins only.
- [ ] **Step 1: Write the fake GitHub** (`fakegithub_test.go`): an `httptest.Server` serving `/login/device/code`, `/login/oauth/access_token` (first poll pending, then a token), `/user` (`{"id":7,"login":"ana"}`), `/app/installations/42/access_tokens` (checks a parseable RS256 JWT with the test key's public part), `/orgs/waycarbon/members/{login}` (204 for `ana`, 404 else); configurable members.
- [ ] **Step 2: Write failing tests** (`auth_test.go`): `TestDeviceSignIn` (device → poll 428 → poll 200 with token; `/auth/signout` then makes it 401 on a Bearer-only route); `TestNonMemberIsRefused` (`403` with the exact message, no token, no user stored); `TestInstallationTokenIsCached` (two `IsMember` calls → one token exchange); `TestTokenNotLogged` (capture `log` output during sign-in; neither the relay token nor the GitHub token appears).
- [ ] **Step 3: Run** `go test ./relay/` — Expected: FAIL.
- [ ] **Step 4: Implement** `github.go` (JWT with `crypto/rsa.SignPKCS1v15` + `crypto/sha256`, base64url JSON header/claims) and `auth.go`.
- [ ] **Step 5: Run** `go vet ./... && go test ./...` — Expected: PASS.
- [ ] **Step 6: Commit** `feat: sign in to the relay with a GitHub App device flow`

---

### Task 7: Relay requests API, event stream, and the binary

**Files:**
- Create: `extensions/wai-waycarbon/relay/hub.go`, `extensions/wai-waycarbon/relay/api.go`, `extensions/wai-waycarbon/relay/main.go`, `extensions/wai-waycarbon/relay/api_test.go`, `extensions/wai-waycarbon/relay/README.md`

**Interfaces:**
- Consumes: `Store`, `Server`, `authed`, `GitHub.IsMember` (Tasks 5–6); `helpapi` events.
- Produces:
  - `type Hub struct` — per user id, up to `StreamsPerUser` channels; `Send(userID int64, event string, data any)`; `Online(userID int64) bool`; `Join(userID int64) (ch <-chan []byte, leave func(), ok bool)`.
  - `GET /events` (Bearer): `text/event-stream`; on connect a membership check (`IsMember`; false → 403), then `event: sync` (`[]Request` from `Open`), `event: people` (`[]Person`: every known user but self, with `Online`); `: ping\n\n` every 30 s; a 5th stream → 429. Online changes broadcast `people` to every connected user.
  - `POST /requests` `{"to","project","text"}` → 201 `Request`; recipient must be a known (signed-in) user, else 404 `{"error":"@<to> has not signed in to help requests"}`; store errors → 400/429 with a message; sends `request.new` (the full `Request`) to both sides, since both list it.
  - `POST /requests/{id}/{cancel|accept|decline|reply}` body `{"text"?}` (reason for decline, reply for reply) → 200 `Request`; errors map `ErrNotFound`→404, `ErrForbidden`→403, `ErrBadState`→409, `ErrTooLong`→400; on success `request.updated` (`Update`) to both sides.
  - Bodies over `MaxBody` → 413 (`http.MaxBytesReader`).
  - A loop: every minute `Sweep()` (send `request.updated` for each expired), every hour `IsMember` for each connected user (false → close their streams, drop their tokens).
  - `main.go`: `ConfigFromEnv`, `OpenStore(cfg.Data)`, `http.Server{Addr, ReadHeaderTimeout: 10s}`, graceful shutdown on SIGTERM.
- [ ] **Step 1: Write failing tests** (`api_test.go`, an `httptest.Server` over `Routes()` with the fake GitHub, two signed-in users `ana` (7) and `bob` (8)):
  - `TestRequestFlowOverStreams`: bob opens `/events` (reads `sync` `[]` and `people`); ana opens hers → bob gets a `people` event with ana `online:true`; ana POSTs a request to bob → bob's stream gets `request.new` with ana's text; bob accepts → ana's stream gets `request.updated` `accepted`; bob replies → `replied` with the reply.
  - `TestStrangersAndLeaversAreRefused` (Review Focus 3): a third user `eve` gets 404 acting on ana's request; after the fake GitHub drops `bob` from the org, bob's `/events` gets 403 and his POSTs to accept get 403; nothing changes in the store.
  - `TestOfflineRecipientGetsSyncOnConnect` (Review Focus 4): ana sends while bob has no stream; bob connects → the request is in `sync`.
  - `TestLimitsOverHTTP`: body over 64 KB → 413; 5th stream → 429; unknown recipient → 404 with the message.
- [ ] **Step 2: Run** `go test ./relay/` — Expected: FAIL.
- [ ] **Step 3: Implement** `hub.go`, `api.go`, `main.go`.
- [ ] **Step 4: Run** `go vet ./... && go test ./...`, and `go build -o /tmp/wai-relay ./relay && rm /tmp/wai-relay` — Expected: PASS, builds.
- [ ] **Step 5: README** (`relay/README.md`): what it is, how to build/run (`go build ./relay`), every env var, what the GitHub App needs (device flow enabled; org permission *Members: read*; installed on the org; no repository permissions), TLS ended by the load balancer, logs carry no text or tokens, and the owner-only items from the spec.
- [ ] **Step 6: Commit** `feat: serve help requests over an event stream`

---

### Task 8: Extension skeleton — manifest 0.2.0, icon, tabs root, bar item, plugin upkeep

**Files:**
- Modify: `extensions/wai-waycarbon/wai-extension.toml`, `extensions/wai-waycarbon/main.go`, `extensions/wai-waycarbon/jira.go` (move the generic runner out)
- Create: `extensions/wai-waycarbon/icon.svg`, `extensions/wai-waycarbon/run.go`, `extensions/wai-waycarbon/plugin.go`, `extensions/wai-waycarbon/view.go`, `extensions/wai-waycarbon/plugin_test.go`
- Modify: `extensions/wai-waycarbon/main_test.go` (panel id `waycarbon`, tree under the Work tab)
- Modify: `crates/wai/src/waycarbon.rs` (copy every non-test `*.go`, `helpapi/`, and `icon.svg`; fake `gh` and `claude` beside fake `jira`; assert through the tabs node)

**Interfaces:**
- Consumes: wai API 1.5 (`tabs`, `requires`, svg icon).
- Produces:
  - `run.go`: `type Problem string` (moved), `func runCLI(ctx context.Context, name string, args ...string) (stdout string, err error)` — `exec.LookPath`, missing → `ErrNotFound{name}`; 30 s timeout default when ctx has none; stdin null; `cmd.WaitDelay = 2s`; env adds `GIT_TERMINAL_PROMPT=0`; failure → error with stderr's last non-empty line. `jira()` becomes `runCLI(ctx, "jira", …)` with its existing messages.
  - `plugin.go`: `const pluginName = "waycarbon"; const pluginMarketplace = "waycarbon/claude-code"`; `func pluginInstalled(ctx) (id string, ok bool, err error)` (parses `claude plugin list --json`, first `id` with prefix `waycarbon@`); `func installPlugin(ctx) error` (`claude plugin install waycarbon --marketplace waycarbon/claude-code --scope user`, 5-minute ctx, never `-y`); `func updatePluginDaily(ctx, dataDir string, now time.Time) error` (file `<dataDir>/plugin-updated` holds RFC 3339; skip if < 24 h; else `claude plugin marketplace update <market>` then `claude plugin update waycarbon@<market>`, where `<market>` is the part after `@` of the installed id; write the time even on failure so a broken network does not retry every start).
  - `view.go`: `func rootTree(work, help node, workBadge, helpBadge int) node` → `{"type":"column","children":[{"type":"tabs","id":"tab","tabs":[{"id":"work","title":"Work","badge"?…,"children":[work]},{"id":"help",…}]}]}` (badge omitted at 0); `func (s *server) paintAll()` renders the root from the current Work/Help state and sends `item/update` for `attention` (hidden: `{"item":"attention"}` with no text/icon at 0; else `text` = count, `icon` `bell`, `tone` `warning`, `card` = `kv` of non-zero groups `Needs you`, `Review requests`, `Cards to update`, `Help requests`, `Replies to send`); `item/action` → `panel/open` `waycarbon`.
  - Panel id constant `panelID = "waycarbon"`.
- Manifest exactly as the spec's "Manifest" block, keeping the three templates; `icon.svg` exactly the spec's four paths in `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor">`.

- [ ] **Step 1: Write failing tests**:
  - `plugin_test.go` with fake `claude` on PATH (the `fakeJira` pattern, generalized to `fakeCLI(t, name, script)`): `TestPluginInstalledParsesList` (`[{"id":"waycarbon@waycarbon"}]` → ok, id); `TestInstallPluginArgv` (log shows `plugin install waycarbon --marketplace waycarbon/claude-code --scope user`, no `-y`, `GIT_TERMINAL_PROMPT=0`); `TestUpdateOncePerDay` (first call runs `plugin marketplace update waycarbon` and `plugin update waycarbon@waycarbon`; a second call 1 h later runs nothing; 25 h later runs again).
  - `main_test.go`: `TestInitializeRendersTabs` — after `initialize`, a `panel/render` for panel `waycarbon` whose root column's first child is `tabs` with ids `work`,`help`; `TestBarItemOpensPanel` — `item/action` `attention` → a `panel/open` `{"panel":"waycarbon"}` call.
- [ ] **Step 2: Run** `go test ./...` — Expected: FAIL.
- [ ] **Step 3: Implement** as in Interfaces; at `initialize` read `data_dir` and `project` from params, reply `{}`, paint a Loading root, then in goroutines: plugin check (missing → Work shows the warning + **Install plugin** button id `plugin:install`; click runs `installPlugin` with a spinner, then re-checks), `updatePluginDaily`, and the existing jira cards (until Task 9 replaces the Work body). Keep the `gen`/mutex staleness pattern.
- [ ] **Step 4: Update** `crates/wai/src/waycarbon.rs` so its build copies the module's files (every `*.go` not ending in `_test.go`, `helpapi/`, `icon.svg`, `go.mod`) and its assertions find the cards inside the Work tab.
- [ ] **Step 5: Run** the Go gate and `cargo test -p wai waycarbon` — Expected: PASS.
- [ ] **Step 6: Commit** `feat: give wai-waycarbon one tabbed panel, its icon and a bar counter`

---

### Task 9: Work tab

**Files:**
- Create: `extensions/wai-waycarbon/github.go`, `extensions/wai-waycarbon/work.go`, `extensions/wai-waycarbon/work_test.go`, `extensions/wai-waycarbon/testdata/graphql.json`
- Modify: `extensions/wai-waycarbon/jira.go` (`moveCard`), `extensions/wai-waycarbon/main.go`, `extensions/wai-waycarbon/view.go`, `extensions/wai-waycarbon/main_test.go`

**Interfaces:**
- Consumes: `runCLI`, `myCards`, `paintAll`, `rootTree` (Task 8).
- Produces:
  - `type PR struct { Number int; Title, URL, Repo, Branch string; Draft bool; Review string /* reviewDecision */; Mergeable string; CI string /* statusCheckRollup.state or "" */; Labels []string; Merged bool }`
  - `type Snapshot struct { Login string; Mine, Requested, Queue, Merged []PR }`
  - `func fetchGitHub(ctx, org string, since time.Time) (Snapshot, error)` — one `gh api graphql -f query=<Q>` (the spec's four searches, `first: 50` each, `since` formatted `2006-01-02`), parse JSON; `gh` missing → `ErrNotFound`; signed out detected by `gh auth status` failing (`errSignedOut`).
  - `type Reason int` with `Conflict, CIFailed, ChangesRequested, ReadyToMerge, Waiting` in that order; `func reasonOf(p PR) Reason` (conflict: `Mergeable=="CONFLICTING"`; CI failed: `CI` is `FAILURE` or `ERROR`; changes: `Review=="CHANGES_REQUESTED"`; ready: `Review=="APPROVED" && CI=="SUCCESS" && !Draft`; else `Waiting`).
  - `func keysOf(p PR) []string` — Jira keys (`[A-Z][A-Z0-9]+-\d+`) in title and branch, deduplicated.
  - `type Work struct { NeedsYou []PR; Reviews []ReviewPR; Cards []CardRow; Waiting []PR }`, `type ReviewPR struct { PR; Requested bool }`, `type CardRow struct { Card; Open []PR; MergedNoOpen bool }`; `func buildWork(s Snapshot, cards []Card) Work` — reviews deduplicated by URL (`Requested` wins), sorted Requested first; a card is `MergedNoOpen` when it has ≥ 1 merged PR and no open one.
  - `func promptFor(action string, …) string` per the spec's table, exact strings: `Resolve the merge conflicts on <url>.`, `The CI fails on <url>. Find out why and fix it.`, `/waycarbon:fix-review <url>`, `Merge <url>, then move its Jira card.`, `/waycarbon:code-review <url>`, `/waycarbon:solve-card <KEY>`, `/waycarbon:deploy`, `/waycarbon:recheck-review <url>`.
  - `func workTree(w Work, st WorkState) node` — the spec's sections (`Needs you`, `Review requests`, `My cards`, `Waiting on others` with `open:false`), header row `@login · updated HH:MM` + `button` `work:refresh` icon `refresh-cw`; PR cards: `card` with `id` `pr:<url>`, `title` `#<n> <title>`, `subtitle` repo, `badge` reason text, child `link` `Open on GitHub`; review cards id `review:<url>`; card rows id `card:<KEY>`; **Move to Done** `button` id `done:<KEY>` with `confirm {"title":"Move <KEY> to Done?"}`.
  - `func moveCard(ctx, key string) error` → `jira issue move <KEY> <done>` (`WAI_WAYCARBON_DONE`, default `Done`).
  - `type WorkState struct { GhMissing, GhSignedOut, JiraMissing bool; JiraProblem string; SignIn *GhSignIn; Err string; Updated time.Time }`; `type GhSignIn struct { Code string; Err string }`; `func ghLogin(ctx) (*exec.Cmd, <-chan string /* code */, <-chan error /* exit */)` running `gh auth login --web --hostname github.com --git-protocol https --skip-ssh-key`, parsing `one-time code: (\S+)` from stderr.
  - Refresh: at start, on `work:refresh`, every 10 minutes (`time.Ticker`), after a successful `done:`; one at a time (a refresh while one runs is dropped).
- [ ] **Step 1: Write failing tests** (`work_test.go`, pure functions over `testdata/graphql.json` holding: a conflicting PR, a CI-failed one, a changes-requested one, an approved green one, a draft, a PR with `commits.nodes: []`, one with `statusCheckRollup: null` and `mergeable: "UNKNOWN"`, one whose `repository` is null; a review PR present in both requested and queue; merged PRs for `EC-7` and `EC-8`, with `EC-8` also open):
  - `TestReasons`: each PR maps to its reason; the three partial ones (Review Focus 2) → `Waiting`, no panic.
  - `TestBuildWork`: NeedsYou order conflict, CI, changes, ready; the duplicated review appears once with `Requested`; `EC-7` is `MergedNoOpen`, `EC-8` is not; a card with no PR has empty `Open`.
  - `TestPrompts`: each exact prompt string above.
  - `TestWorkTreeTextIsPlain`: a PR title `[x](javascript:alert(1))` appears only in a `card` `title`, and the tree has no `markdown` node.
- [ ] **Step 2: Write failing protocol tests** (`main_test.go`, fake `gh`, `jira`, `claude`): `TestPRClickOpensSession` (click `pr:<url>` of the changes-requested PR → `sessions/open` with `/waycarbon:fix-review <url>`, `send:false`); `TestMoveToDone` (click `done:EC-7` → fake jira log has `issue move EC-7 Done`, then a refresh); `TestGhMissing` (no `gh` on PATH → Work shows `GitHub CLI not found` and a `log` node with `brew install gh`, and the jira cards still render); `TestGhSignIn` (fake `gh auth status` exits 1; click `gh:signin` → fake `gh auth login` prints `! First copy your one-time code: AB12-CD34` to stderr then sleeps 1 s and exits 0 → the tree shows `AB12-CD34` and a `link` to `https://github.com/login/device`, then refreshes).
- [ ] **Step 3: Run** `go test ./...` — Expected: FAIL.
- [ ] **Step 4: Implement** `github.go`, `work.go`, the handlers in `main.go`, and wire the Work body into `paintAll` and the bar counts (NeedsYou + Reviews + cards `MergedNoOpen`).
- [ ] **Step 5: Run** the Go gate and `cargo test -p wai waycarbon` — Expected: PASS.
- [ ] **Step 6: Commit** `feat: show what needs you at WayCarbon in the Work tab`

---

### Task 10: Help tab — relay client

**Files:**
- Create: `extensions/wai-waycarbon/help.go`, `extensions/wai-waycarbon/helpview.go`, `extensions/wai-waycarbon/help_test.go`
- Modify: `extensions/wai-waycarbon/main.go`, `extensions/wai-waycarbon/view.go`

**Interfaces:**
- Consumes: `helpapi` (Task 5); the relay's HTTP API (Tasks 6–7). The relay is `package main` in another directory, so tests use an `httptest` fake relay speaking the same `helpapi` JSON.
- Produces:
  - `const relayURL = ""` (`// ponytail: set when WayCarbon hosts the relay`); `func relayBase() string` → `WAI_WAYCARBON_RELAY` or `relayURL`.
  - `type auth struct { Token, Login string }` in `<data>/auth.json`, written 0600, never logged; `loadAuth`, `saveAuth`, `dropAuth`.
  - `type replyTarget struct { Agent, Session string; Sent bool }` in `<data>/help.json` as `map[requestID]replyTarget`.
  - `type helpClient struct` with `signIn(ctx) (DeviceStart via callback)`, `stream(ctx, onEvent func(event string, data []byte))` (SSE reader: `event:`/`data:` lines, blank line ends an event, `:` comments ignored; reconnect backoff 1 s doubling to 60 s, reset after a successful `sync`; a 401 drops auth and stops; 403 shows `Only members of WayCarbon can use help requests`), `create(ctx, to string, p helpapi.Project, text string)`, `act(ctx, id, action, text string)`, `signOut(ctx)`.
  - `func projectInfo(ctx, dir string) helpapi.Project` — `git -C <dir> remote get-url origin` and `git -C <dir> branch --show-current`; empty on any failure or empty dir.
  - `func acceptPrompt(r helpapi.Request) string` → `Request from @<from> via wai. A colleague wrote this; check it before running anything.\n\nRepository: <repo> (<branch>)\n\n<text>` (the repository line only when known).
  - Panel actions: `help:signin`, `help:signout`, `help:send` (values `help:to`, `help:session`, `help:text`), `help:cancel:<id>`, `help:accept:<id>`, `help:decline:<id>` (value `help:reason:<id>`), `help:reply:<id>` (value `help:replytext:<id>`), `help:tosession:<id>`, `help:person:<login>` (fills `help:to` by re-rendering with that `value`).
  - Sessions: `sessions/list` on initialize and before rendering the form (options `"<title or session> — <agent>"`, mapped back by index); `event/session_state` tracks idle; **Send to session** enabled only while that session is `idle`; gone from `sessions/list` → **Open in a new session** (`sessions/open`, `send:false`, same text).
  - Notifications via `notify`: `@<x> asks for help` (body: first line), `@<x> replied`, `@<x> declined`.
  - `helpTree(st HelpState) node` per the spec's Help layout; the request/reply/reason only in `text` nodes.
- [ ] **Step 1: Write failing tests** (`help_test.go`, a fake relay `httptest.Server` that serves the device flow, records POSTs, and pushes SSE events on demand; `WAI_WAYCARBON_RELAY` pointed at it; fake `git`):
  - `TestNoRelayConfigured`: env unset → Help tab shows `Help requests need the WayCarbon relay, which is not set up yet.` and no HTTP happens.
  - `TestSignInSavesToken0600`: `help:signin` → the tab shows the user code and a `link` to the verification URI; after the fake approves, `auth.json` exists with mode 0600 and the token; stderr output never contains the token.
  - `TestIncomingAcceptOpensUnsentSession`: push `request.new` from `ana` with text `deploy\nplease` → a `notify` `@ana asks for help`; the card shows the text in a `text` node; `help:accept:<id>` → POST `/requests/<id>/accept`, then `sessions/open` with `acceptPrompt` and `send:false`.
  - `TestSendRequestCarriesProjectAndTarget`: values `help:to=bob`, session option 0, text → POST `/requests` with `project` from fake git and the text; `help.json` stores the chosen session.
  - `TestReplyGoesToIdleSessionOnly`: a `request.updated` `replied` with reply `done` → `notify` `@bob replied`; **Send to session** is `disabled` while the session is `working`, enabled after `event/session_state` `idle`; click → `sessions/prompt` with `@bob replied to your request: done` and `help.json` marks it sent (the bar counter drops by one).
  - `TestStreamReconnects`: the fake relay closes the stream → the client reconnects and receives `sync` again; a 401 → `auth.json` deleted and the tab shows **Sign in with GitHub**.
- [ ] **Step 2: Run** `go test ./...` — Expected: FAIL.
- [ ] **Step 3: Implement** `help.go`, `helpview.go`, wire into `main.go`/`paintAll` (Help badge and bar counts: incoming `waiting` + replies not sent).
- [ ] **Step 4: Run** the Go gate — Expected: PASS.
- [ ] **Step 5: Commit** `feat: ask a colleague for help from the Help tab`

---

### Task 11: Docs and the whole-branch check

**Files:**
- Modify: `extensions/wai-waycarbon/README.md` (what it does now: Work, Help, the counter, the required plugin, the env overrides `WAI_WAYCARBON_ORG`, `WAI_WAYCARBON_DONE`, `WAI_WAYCARBON_RELAY`, the CLIs it needs and how it never touches their tokens)
- Modify: `docs/superpowers/plans/2026-11-11-wai-master-plan.md` (wai-waycarbon line: 0.2 cockpit, help requests pending relay hosting)
- Modify: `docs/extensions.md` only if Tasks 2–4 left a gap (manifest table mentions `api` "1.5" for `tabs`/`requires`/svg icon)

- [ ] **Step 1:** Update the docs above.
- [ ] **Step 2: Run** the full gates: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` and, in `extensions/wai-waycarbon`, `gofmt -l .` (empty) `&& go vet ./... && go test ./...` — Expected: all PASS. Report any test that fails on `development` too (pre-existing) separately.
- [ ] **Step 3: Commit** `docs: describe the WayCarbon cockpit`
