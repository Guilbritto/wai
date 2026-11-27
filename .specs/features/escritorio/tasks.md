# Escritório Tasks

## Execution Protocol (MANDATORY -- do not skip)

Implement these tasks with the `tlc-spec-driven` skill: **activate it by name and follow its Execute flow and Critical Rules.** Do not search for skill files by filesystem path. The skill is the source of truth for the full flow (per-task cycle, sub-agent delegation, the walk, check_proofs, the Verifier and the screens sub-agent).

**If the skill cannot be activated, STOP and tell the user - do not proceed without it.**

---

**Design**: `.specs/features/escritorio/design.md`
**Status**: Approved
**Approved**: 2026-11-26 - user: "aprovado"
**Build**: `cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings; test ! -d extensions/wai-escritorio || (cd extensions/wai-escritorio && test -z "$(gofmt -l .)" && go vet ./...)`
**Run ext**: `cargo test -p wai-ext`
**Run wai**: `cargo build -p wai-ext --bin wai-fake-ext && cargo test -p wai scene`
**Run go**: `cd extensions/wai-escritorio && go test ./... -v -count=1`

Worktree: `.claude/worktrees/escritorio`, branch `feat/escritorio`, its own `target/` (never a shared `CARGO_TARGET_DIR`). Commits are `feat:`/`fix:` for `crates/**` and `docs/**`, `chore:` for `extensions/**` (CLAUDE.md, Release), no scope, no Co-Authored-By.

---

## Test Coverage Matrix

> Generated from codebase, project guidelines, and spec - confirm before Execute. Guidelines found: `CLAUDE.md` (release and commit rules), `docs/building.md:39` (`cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace`), `.github/workflows/ci.yml:34-53` (fmt, clippy `-D warnings`, `go vet`, `go test` per Go extension). No coverage thresholds; strong defaults applied.

| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| ---------- | ------------------ | -------------------- | ---------------- | ----------- |
| Panel schema (`wai-ext/src/panel.rs`) | unit | Every scene rule: valid parse, each limit and each bad field → Unsupported; 1:1 to OFC-30, OFC-35 | `#[cfg(test)] mod tests` in `panel.rs` | `cargo test -p wai-ext` |
| Extension host / manifest (`wai-ext/src/host.rs`, `manifest.rs`) | integration | `panel/key` round trip through the fake extension; API 1.10 accepted, 1.11 refused | `crates/wai-ext/tests/host.rs`, `tests/manifest.rs` | `cargo test -p wai-ext` |
| Scene pure functions (`wai/src/ext_scene.rs`) | unit | All branches of `rasterise`, `viewport`, `tween`, label stacking; 1:1 to OFC-11, 31, 34, 62 | `#[cfg(test)] mod tests` in `ext_scene.rs` | `cargo test -p wai scene` |
| Scene element, focus, keys (`wai/src/ext_scene.rs`, `ext_panel.rs`, `ext_host.rs`) | integration (GPUI `TestAppContext`) | Draws, click focuses, keys emitted with panel/scene/key, ⌘ keys not sent, Unsupported on bad tree | `ext_panel.rs` tests (`panel_window`/`show` helpers) | `cargo test -p wai scene` |
| `office` package (Go) | unit | All branches; 1:1 to OFC-02, 06, 07, 28, 40, 50-52, 61, 63 | `extensions/wai-escritorio/office/*_test.go` | `go test ./... -v` |
| Server (Go) | integration (`httptest` + WebSocket clients) | Every refusal, limit and log line: happy + edge + error, OFC-08..10, 12, 20-29, 54, 60 | `extensions/wai-escritorio/server/*_test.go` | `go test ./... -v` |
| Extension (Go) | integration (JSON-RPC lines in, trees out, in-process server) | Every panel state and action: OFC-01..06, 12-14, 40-44, 53 | `extensions/wai-escritorio/*_test.go` | `go test ./... -v` |
| Wire types, manifest, art data, CI config, docs | none | build gate only (art: a validity test in T15) | - | Build |

## Gate Check Commands

> Generated from codebase - confirm before Execute.

| Gate Level | When to Use | Command |
| ---------- | ----------- | ------- |
| Quick | After a Go task | `cd extensions/wai-escritorio && go test ./... -count=1` |
| Full | After a Rust task | `cargo test -p wai-ext && cargo build -p wai-ext --bin wai-fake-ext && cargo test -p wai scene` |
| Build | After each phase and before each Rust commit | the `**Build**:` line above, then `cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace` once per phase that touches `crates/` |

---

## Execution Plan

Phases run in order; tasks within a phase run in order.

### Phase 1: Scene schema and protocol (`wai-ext`)

```
T1 → T2 → T3
```

### Phase 2: Scene view in wai

```
T4 → T5 → T6 → T7
```

### Phase 3: `office` package (Go)

```
T8 → T9 → T10
```

### Phase 4: Office server (Go)

```
T11 → T12 → T13
```

### Phase 5: The extension (Go)

```
T14 → T15 → T16 → T17 → T18
```

### Phase 6: End to end and CI

```
T19 → T20
```

---

## Task Breakdown

### Phase 1: Scene schema and protocol

### T1: Parse and validate the `scene` node

**What**: `Node::Scene(Scene)` in the panel schema with every rule of design §1, Unsupported on any break, never inside a hover card.
**Where**: `crates/wai-ext/src/panel.rs`
**Depends on**: None
**Reuses**: `pixels` parse (`panel.rs:880-920`), `tone()`, the id set (`:924-933`), `card()` (`:317`)
**Requirement**: OFC-30, OFC-35

**Done when**:

- [x] `scene_parse` and `scene_rules_unsupported` (one case per rule: missing id, cell 3/33, scale 5, bad hex, 65 palette keys, 33×33 pattern, tile pattern not cell×cell, 129 rows, 101 sprites, sprite outside map, unknown pattern, repeated sprite id, label 33 chars) pass
- [x] `scene_not_in_card` passes
- [x] Gate check passes: `cargo test -p wai-ext`

**Tests**: unit
**Gate**: full

---

### T2: Bump the extension API to 1.10

**What**: `API = "1.10"`; manifests asking 1.10 load, 1.11 is refused with "this wai has 1.10".
**Where**: `crates/wai-ext/src/manifest.rs`
**Depends on**: T1
**Reuses**: `check_api` (`manifest.rs:412-430`), tests `tests/manifest.rs:295-320`, `tests/host.rs:128`
**Requirement**: OFC-30

**Done when**:

- [x] `api_versions` test covers 1.10 ok and 1.11 too old
- [x] Gate check passes: `cargo test -p wai-ext`

**Tests**: integration
**Gate**: full

---

### T3: `panel/key` notification from host to extension

**What**: `ExtHandle::panel_key(panel, scene, key, repeat)` sends the `panel/key` notification, gated on `panels`.
**Where**: `crates/wai-ext/src/host.rs`
**Depends on**: T2
**Reuses**: `panel_action` (`host.rs:354-359`), `send()`, `wai-fake-ext` echo
**Requirement**: OFC-32

**Done when**:

- [x] `panel_key_notification` (host test via `wai-fake-ext`) sees `{"panel","scene","key","repeat"}` arrive with no `id`
- [x] `panel_key_needs_panels` sees nothing sent without the permission
- [x] Gate check passes: `cargo test -p wai-ext`

**Tests**: integration
**Gate**: full

---

### Phase 2: Scene view in wai

### T4: Scene pure functions

**What**: `rasterise`, `viewport`, `tween`, `label_rows` in a new module, all pure.
**Where**: `crates/wai/src/ext_scene.rs`
**Depends on**: T3
**Reuses**: `ext_bubble.rs` (pure timeline style), hex parser `appearance/overrides.rs:31-49`
**Requirement**: OFC-11, OFC-31, OFC-34, OFC-62

**Done when**:

- [x] `rasterise_paints_palette_and_transparency`, `viewport_keeps_margin`, `viewport_centres_first_time`, `tween_takes_150ms`, `tween_skips_jumps`, `shared_tile_labels_stack` pass
- [x] Gate check passes: `cargo test -p wai scene`

**Tests**: unit
**Gate**: full

---

### T5: Draw the scene in a panel

**What**: The `Scene` arm of `ExtPanel::node` draws the cached tile layer, sprites, labels, badges, the tween ticker and the "Click the map to walk" hint.
**Where**: `crates/wai/src/ext_scene.rs` (element) + one arm in `crates/wai/src/ext_panel.rs`
**Depends on**: T4
**Reuses**: `ExtPanel::pixels` (`ext_panel.rs:280-336`), `ensure_ticker` (`:237-276`), `tone_color()` (`:1776`), `RenderImage` + `img()`
**Requirement**: OFC-30, OFC-31, OFC-35

**Done when**:

- [x] `scene_draws_tiles_and_sprites` and `scene_layer_cached_across_renders` pass; `scene_bad_tree_unsupported` shows "Unsupported element"
- [x] A `WAI_SHOT_TREE` screenshot of a sample scene shows correct colours (red is red)
- [x] Gate check passes: Full

**Tests**: integration
**Gate**: full

---

### T6: Scene focus and keys

**What**: The scene takes the keyboard focus on click and turns non-⌘/ctrl keys into `PanelKey`, which `ExtHost` forwards with `panel_key`.
**Where**: `crates/wai/src/ext_scene.rs` (focus + key handler); wiring in `ext_panel.rs` (`FocusHandle`) and `ext_host.rs` (subscription beside `:437`)
**Depends on**: T5
**Reuses**: terminal key handler (`terminal_view.rs:166-181, 367-369`), `PanelAction` path (`ext_panel.rs:452-455`, `ext_host.rs:437-441`)
**Requirement**: OFC-32, OFC-33

**Done when**:

- [x] `scene_click_takes_focus`, `scene_keys_reach_extension` (arrows and WASD with panel, scene, key), `scene_cmd_keys_not_sent` pass
- [x] Gate check passes: Full, then the Build gate (Phase 2 touches `crates/`)

**Tests**: integration
**Gate**: build

---

### T7: Document scene, `panel/key` and API 1.10

**What**: `docs/extensions.md`: the scene row and its rules, the `panel/key` row, the `api` version list, the version mentions; `panel.rs:13` doc comment.
**Where**: `docs/extensions.md`
**Depends on**: T6
**Reuses**: the `pixels` and `bubble` rows as the model
**Requirement**: OFC-36

**Done when**:

- [x] The C31 grep proof passes
- [x] Gate check passes: Build

**Tests**: none
**Gate**: build

---

### Phase 3: `office` package

### T8: Go module and the office map

**What**: `go.mod` for `wai-escritorio`, `office.ParseMap`, `Map.Blocked`, the embedded `default.map` (original layout, about 40 × 30, legend of design §5).
**Where**: `extensions/wai-escritorio/office/map.go`
**Depends on**: T7
**Reuses**: none (bichinho's `go 1.22` module style)
**Requirement**: OFC-07, OFC-61

**Done when**:

- [x] `TestDefaultMapParses`, `TestBlocked` (wall, desk, plant, table, outside), `TestMapWithoutSpawnRefused`, `TestUnknownTileRefused` pass
- [x] Gate check passes: Quick

**Tests**: unit
**Gate**: quick

---

### T9: Office rules

**What**: `Step`, `Adjacent`, `Nearby`, `Derive`, `ValidName`.
**Where**: `extensions/wai-escritorio/office/rules.go`
**Depends on**: T8
**Reuses**: none
**Requirement**: OFC-02, OFC-06, OFC-28, OFC-40, OFC-50, OFC-51, OFC-52, OFC-63

**Done when**:

- [x] `TestStepKeys`, `TestAdjacent`, `TestNearbyChebyshev`, `TestDeriveFocused`, `TestDeriveNeedsYouWins`, `TestDeriveNone`, `TestValidName`, `TestNameRejectsControlChars` pass
- [x] Gate check passes: Quick

**Tests**: unit
**Gate**: quick

---

### T10: Wire types

**What**: The message structs of design "Data Models", with `t` constants.
**Where**: `extensions/wai-escritorio/office/wire.go`
**Depends on**: T9
**Reuses**: none
**Requirement**: OFC-27

**Done when**:

- [x] Compiles; `go vet` clean
- [x] Gate check passes: Quick

**Tests**: none
**Gate**: quick

---

### Phase 4: Office server

### T11: Server config

**What**: Load `office.json`; refuse short token, plain listen off loopback, missing or bad map, with the spec's messages.
**Where**: `extensions/wai-escritorio/server/config.go`
**Depends on**: T10
**Reuses**: `office.ParseMap`
**Requirement**: OFC-20, OFC-21, OFC-22, OFC-61

**Done when**:

- [x] `TestConfigRejectsShortToken`, `TestConfigRequiresTLSOffLoopback`, `TestConfigAllowsPlainLoopback`, `TestConfigBadMapRefused` pass
- [x] Gate check passes: Quick

**Tests**: unit
**Gate**: quick

---

### T12: Server hub

**What**: The goroutine owning people and last positions: `authenticate`, join (spawn or last position), move (adjacent and free, else `position`), status, leave, replaced, full.
**Where**: `extensions/wai-escritorio/server/hub.go`
**Depends on**: T11
**Reuses**: `office` rules and wire types
**Requirement**: OFC-09, OFC-12, OFC-26, OFC-27, OFC-28, OFC-54

**Done when**:

- [x] `TestAuthenticate`, `TestRejoinAtLastPosition`, `TestIllegalMoveSnapsBack`, `TestSameNameReplacesOlder`, `TestFiftyFirstRefused`, `TestStatusFansOut` pass
- [x] Gate check passes: Quick

**Tests**: integration
**Gate**: quick

---

### T13: Server connections

**What**: `main.go`: WebSocket accept, `hello` first, read limit 16 KiB, 30 msg/s window, ping 3 s / deadline 7 s, log lines, TLS when configured.
**Where**: `extensions/wai-escritorio/server/main.go`
**Depends on**: T12
**Reuses**: hub, config, `github.com/coder/websocket`
**Requirement**: OFC-08, OFC-10, OFC-20, OFC-23, OFC-24, OFC-25, OFC-29

**Done when**:

- [x] `TestServerAcceptsOnConfiguredAddress`, `TestWrongTokenRefusedAndLoggedWithoutToken`, `TestRateLimitDropsPast30`, `TestOversizeMessageCloses`, `TestSilentPeerLeaves`, `TestLogsConnectDisconnectRefusal`, `TestMoveFanOutWithin200ms` pass
- [x] `go.sum` committed
- [x] Gate check passes: Quick

**Tests**: integration
**Gate**: quick

---

### Phase 5: The extension

### T14: Extension setup

**What**: `Setup` load/save (`setup.json`, 0600) and `validate` (name, server URL, refuses `ws://` off loopback).
**Where**: `extensions/wai-escritorio/setup.go`
**Depends on**: T13
**Reuses**: `office.ValidName`
**Requirement**: OFC-02, OFC-13, OFC-63

**Done when**:

- [x] `TestSetupRejectsBadName`, `TestSetupSavedPrivate`, `TestSetupRefusesPlainRemote` pass
- [x] Gate check passes: Quick

**Tests**: unit
**Gate**: quick

---

### T15: Original pixel art

**What**: Palette and 16 × 16 patterns: floor, carpet, wall, desk, chair, plant, table, and 8 avatars.
**Where**: `extensions/wai-escritorio/art.go`
**Depends on**: T14
**Reuses**: bichinho's art-in-code style (`extensions/wai-bichinho/art.go`)
**Requirement**: OFC-30

**Done when**:

- [x] `TestArtValid` (every pattern 16 × 16, only palette keys or space, every map legend character has a pattern, 8 avatars) passes
- [x] A `WAI_SHOT_TREE` screenshot of the default map is shown to the user before the commit
- [x] Gate check passes: Quick

**Tests**: unit
**Gate**: quick

---

### T16: Panel trees

**What**: `setupTree`, `presenceTree`, `officeTree` as pure functions of the extension's state.
**Where**: `extensions/wai-escritorio/views.go`
**Depends on**: T15
**Reuses**: `office.Nearby`, art patterns
**Requirement**: OFC-01, OFC-05, OFC-40, OFC-42, OFC-43, OFC-44, OFC-50, OFC-51, OFC-52

**Done when**:

- [x] `TestNoSetupShowsForm`, `TestOfficeTreeShowsEveryone`, `TestPresenceListsNearby`, `TestPresenceShowsSetupForm`, `TestPresenceOfflineReasons`, `TestPresenceAlone`, `TestStatusBadges` pass
- [x] Gate check passes: Quick

**Tests**: unit
**Gate**: quick

---

### T17: Server client

**What**: `run`: dial, `hello`, deliver server messages to the loop, retry every 5 s, stop on `token` and `replaced`.
**Where**: `extensions/wai-escritorio/client.go`
**Depends on**: T16
**Reuses**: wire types, the hub in an `httptest` server for tests
**Requirement**: OFC-03, OFC-04, OFC-12, OFC-60

**Done when**:

- [x] `TestTokenRefusedStops`, `TestUnreachableRetries`, `TestReplacedStopsRetrying`, `TestReconnectAfterServerRestart` pass
- [x] Gate check passes: Quick

**Tests**: integration
**Gate**: quick

---

### T18: Extension main loop and manifest

**What**: `main.go` (bichinho's loop + server channel): `initialize`, `sessions/list`, `panel/action` (submit, change setup, enter office), `panel/key` (step, blocked, 120 ms pace), `event/session_state` → status on change; `wai-extension.toml`; `README.md`.
**Where**: `extensions/wai-escritorio/main.go`
**Depends on**: T17
**Reuses**: `extensions/wai-bichinho/main.go:30-92, 538-705`
**Requirement**: OFC-01, OFC-03, OFC-06, OFC-07, OFC-14, OFC-41, OFC-53

**Done when**:

- [ ] `TestNoSetupNoDial`, `TestTokenRefusedShowsForm`, `TestKeyMovesOneTile`, `TestBlockedTileStays`, `TestHeldKeyPace`, `TestChangeSetupDisconnectsAndPrefills`, `TestEnterOfficeOpensPane`, `TestStatusCarriesNoSessionData` pass
- [ ] Gate check passes: Quick

**Tests**: integration
**Gate**: quick

---

### Phase 6: End to end and CI

### T19: End-to-end tests

**What**: Two extension loops and a real server in one test process: moves, status and second login across clients.
**Where**: `extensions/wai-escritorio/e2e_test.go`
**Depends on**: T18
**Reuses**: `serve`, the server's handler via `httptest`
**Requirement**: OFC-08, OFC-12, OFC-54

**Done when**:

- [ ] `TestMoveReachesOtherWithin200ms`, `TestStatusReachesOthersWithin1s`, `TestSecondLoginReplacesFirst` pass
- [ ] Gate check passes: Quick

**Tests**: integration
**Gate**: quick

---

### T20: CI for the Go extension

**What**: `go vet ./... && go test ./...` for `extensions/wai-escritorio` in CI.
**Where**: `.github/workflows/ci.yml`
**Depends on**: T19
**Reuses**: the `wai-rotinas` steps (`ci.yml:46-51`)
**Requirement**: OFC-20

**Done when**:

- [ ] The step mirrors `wai-rotinas`'
- [ ] Gate check passes: Build

**Tests**: none
**Gate**: build

---

## Checks

**C1** - With no saved setup, opening the office shows the setup form (name, server, token, 8 avatars) and nothing dials (OFC-01, AC 1)
Proof: run go | --- PASS: TestNoSetupNoDial

**C2** - An empty name, a 33-character name, or one with a newline is refused with "Name must be 1 to 32 characters" (OFC-02, AC 2)
Proof: run go | --- PASS: TestSetupRejectsBadName

**C3** - A refused token keeps the form and shows "The server refused the team token" (OFC-03, AC 3)
Proof: run go | --- PASS: TestTokenRefusedShowsForm

**C4** - An unreachable server shows "Cannot reach <address>" and is dialled again every 5 s (OFC-04, AC 4)
Proof: run go | --- PASS: TestUnreachableRetries

**C5** - Once connected, the office tree has a sprite with the name label for every connected person at their position (OFC-05, AC 5)
Proof: run go | --- PASS: TestOfficeTreeShowsEveryone

**C6** - An arrow key or W/A/S/D moves the user's avatar exactly one tile that way (OFC-06, AC 6)
Proof: run go | --- PASS: TestKeyMovesOneTile

**C7** - A key towards a wall, furniture or the map edge leaves the avatar on its tile (OFC-07, AC 7)
Proof: run go | --- PASS: TestBlockedTileStays

**C8** - A move shows on another connected client within 200 ms over a local server (OFC-08, AC 8)
Proof: run go | --- PASS: TestMoveReachesOtherWithin200ms

**C9** - A person who reconnects appears at their last position on this server run, else at spawn (OFC-09, AC 9)
Proof: run go | --- PASS: TestRejoinAtLastPosition

**C10** - A peer that stops answering disappears for others within 10 s (OFC-10, AC 10)
Proof: run go | --- PASS: TestSilentPeerLeaves

**C11** - The view scrolls the least needed to keep the followed sprite 3 tiles from every edge (OFC-11, AC 11)
Proof: run wai | test ext_scene::tests::viewport_keeps_margin ... ok

**C12** - A second login with the same name keeps the new one; the older shows "Signed in elsewhere" and never redials (OFC-12, AC 12)
Proof: run go | --- PASS: TestSecondLoginReplacesFirst

**C13** - `setup.json` holds name, server, avatar and token and has mode 0600 (OFC-13, AC 13)
Proof: run go | --- PASS: TestSetupSavedPrivate

**C14** - "Change setup" disconnects and shows the form with the saved name, server and avatar and an empty token field; an empty token keeps the saved one (OFC-14, AC 14)
Proof: run go | --- PASS: TestChangeSetupDisconnectsAndPrefills

**C15** - A server started from a valid config accepts an office connection on its listen address (OFC-20, AC 1)
Proof: run go | --- PASS: TestServerAcceptsOnConfiguredAddress

**C16** - A token shorter than 32 characters stops the server with "token must be at least 32 characters" (OFC-21, AC 2)
Proof: run go | --- PASS: TestConfigRejectsShortToken

**C17** - A non-loopback listen address without cert and key stops the server with "TLS is required off loopback" (OFC-22, AC 3)
Proof: run go | --- PASS: TestConfigRequiresTLSOffLoopback

**C18** - A wrong token is refused with `token` and logged with the address and without the token (OFC-23, AC 4)
Proof: run go | --- PASS: TestWrongTokenRefusedAndLoggedWithoutToken

**C19** - Of 40 messages sent in one second, the server acts on 30 (OFC-24, AC 5)
Proof: run go | --- PASS: TestRateLimitDropsPast30

**C20** - A message over 16 KiB closes that connection (OFC-25, AC 6)
Proof: run go | --- PASS: TestOversizeMessageCloses

**C21** - The 51st person is refused with "The office is full" (OFC-26, AC 7)
Proof: run go | --- PASS: TestFiftyFirstRefused

**C22** - `authenticate` alone decides identity and avatar ids are server-assigned, never the token (OFC-27, AC 8)
Proof: run go | --- PASS: TestAuthenticate

**C23** - A move to a blocked tile or two tiles away is ignored and the sender gets its current position (OFC-28, AC 9)
Proof: run go | --- PASS: TestIllegalMoveSnapsBack

**C24** - Connect, disconnect and refusal each log one stderr line without the token (OFC-29, AC 10)
Proof: run go | --- PASS: TestLogsConnectDisconnectRefusal

**C25** - A panel with a scene draws its tiles and sprites with labels and badges (OFC-30, AC 1)
Proof: run wai | test ext_panel::tests::scene_draws_tiles_and_sprites ... ok

**C26** - A sprite whose tile changed reaches the new tile in 150 ms (OFC-31, AC 2)
Proof: run wai | test ext_scene::tests::tween_takes_150ms ... ok

**C27** - With the scene focused, arrows and WASD send the panel id, scene id and key name (OFC-32, AC 3)
Proof: run wai | test ext_panel::tests::scene_keys_reach_extension ... ok

**C28** - A click on a scene gives its panel the keyboard focus (OFC-33, AC 4)
Proof: run wai | test ext_panel::tests::scene_click_takes_focus ... ok

**C29** - A scene larger than its panel shows the part around the followed sprite's tile (OFC-34, AC 5)
Proof: run wai | test ext_scene::tests::viewport_centres_first_time ... ok

**C30** - A scene breaking a rule (missing field, too many tiles or sprites) is Unsupported (OFC-35, AC 6)
Proof: run ext | test panel::tests::scene_rules_unsupported ... ok

**C31** - `docs/extensions.md` documents the scene node, `panel/key` and API 1.10 (OFC-36, AC 7)
Proof: `grep -q '| \`scene\`' docs/extensions.md && grep -q '\`panel/key\`' docs/extensions.md && grep -q '"1.10"' docs/extensions.md`

**C32** - The presence panel lists every connected person by name and marks those within 3 tiles "Nearby" (OFC-40, AC 1)
Proof: run go | --- PASS: TestPresenceListsNearby

**C33** - "Enter office" in the presence panel opens the office pane (`panel/open` office) (OFC-41, AC 2)
Proof: run go | --- PASS: TestEnterOfficeOpensPane

**C34** - With no saved setup, the presence panel shows the setup form (OFC-42, AC 3)
Proof: run go | --- PASS: TestPresenceShowsSetupForm

**C35** - Not connected, the presence panel shows "Offline" and the reason (token, unreachable, elsewhere, full) (OFC-43, AC 4)
Proof: run go | --- PASS: TestPresenceOfflineReasons

**C36** - Alone, the presence panel shows "Nobody else is in the office" (OFC-44, AC 5)
Proof: run go | --- PASS: TestPresenceAlone

**C37** - A `working` session and no `needs_you` derive "Focused" (OFC-50, AC 1)
Proof: run go | --- PASS: TestDeriveFocused

**C38** - Any `needs_you` session derives "Needs you", even with others working (OFC-51, AC 2)
Proof: run go | --- PASS: TestDeriveNeedsYouWins

**C39** - No `working` or `needs_you` session derives no badge (OFC-52, AC 3)
Proof: run go | --- PASS: TestDeriveNone

**C40** - The status frame carries only `{"t":"status","status":…}`: no title, prompt, agent or session id (OFC-53, AC 4)
Proof: run go | --- PASS: TestStatusCarriesNoSessionData

**C41** - A status change shows on another client within 1 s (OFC-54, AC 5)
Proof: run go | --- PASS: TestStatusReachesOthersWithin1s

**C42** - After a server restart the extension reconnects within 5 s and the person starts at spawn (OFC-60)
Proof: run go | --- PASS: TestReconnectAfterServerRestart

**C43** - A map with no spawn point stops the server and names the problem (OFC-61)
Proof: run go | --- PASS: TestMapWithoutSpawnRefused

**C44** - Two sprites on one tile are both drawn and their labels stack apart (OFC-62)
Proof: run wai | test ext_scene::tests::shared_tile_labels_stack ... ok

**C45** - A name with a control character is refused with the OFC-02 message (OFC-63)
Proof: run go | --- PASS: TestNameRejectsControlChars

---

## Handoff

**Settled mid-build:** 2026-11-26 - OFC-14: token field stays empty on "Change setup", an empty submit keeps the saved token - user: "Token vazio, mantém o salvo"; T15 art: chairs and plants on carpet get carpet-backed variants - user: "Aprovo e corrige o fundo"
**Dispatched:** batch 1 (Phases 1-2, T1-T7) - sonnet worker - 2026-11-26 - returned (79e0c54..4cd1565); batch 2 (Phases 3-4, T8-T13) - sonnet worker - 2026-11-26 - returned (c534b49..5a6f088); batch 3 (Phases 5-6, T14-T20) - sonnet worker - 2026-11-26
**Walked:** none

---

## Phase Execution Map

```
Phase 1 → Phase 2 → Phase 3 → Phase 4 → Phase 5 → Phase 6

Phase 1:  T1 → T2 → T3
Phase 2:  T4 → T5 → T6 → T7
Phase 3:  T8 → T9 → T10
Phase 4:  T11 → T12 → T13
Phase 5:  T14 → T15 → T16 → T17 → T18
Phase 6:  T19 → T20
```

20 tasks → three batches of whole phases: Phases 1-2 (7, Rust), Phases 3-4 (6, Go office + server), Phases 5-6 (7, Go extension).
