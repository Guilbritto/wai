# Remote access Tasks

## Execution Protocol (MANDATORY -- do not skip)

Implement these tasks with the `tlc-spec-driven` skill: **activate it by name and follow its Execute flow and Critical Rules.** Do not search for skill files by filesystem path. The skill is the source of truth for the full flow (per-task cycle, sub-agent delegation, the walk, check_proofs, the Verifier and the screens sub-agent).

**If the skill cannot be activated, STOP and tell the user - do not proceed without it.**

---

**Design**: `.specs/features/remote-access/design.md`
**Status**: Approved
**Approved**: 2026-11-20 - user: "PODE SEGUIR"
**Build**: `cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings`
**Run ext**: `cargo test -p wai-ext --test host --test manifest`
**Run wai**: `cargo test -p wai --bin wai -- ext_sessions remote_ consent_`
**Run remoto**: `cd extensions/wai-remoto && go test -v ./...`
**Run mobile**: `cd ../wai-mobile && flutter test --reporter github`

### Prerequisites (before T1, not tasks)

- Go 1.22+ and Flutter stable installed on this machine and on PATH (`go version`, `flutter --version`). Neither is installed today.
- `C:\Users\guilh\Project\wai-mobile` is created by T24 (`flutter create`, `git init`); its commits live in that repository. For T24-T33 the `tasks.md` mark is committed in wai as `chore: mark T<n> done`.
- The iOS build and the walk on an iPhone need the Mac; Android runs from this machine with an emulator or a phone.

---

## Test Coverage Matrix

> Generated from codebase, project guidelines, and spec - confirm before Execute. Guidelines found: `.github/workflows/ci.yml` (fmt, clippy `-D warnings`, `cargo test --workspace`, `go vet` + `go test` per extension), `.github/windows-known-failures.txt`; no coverage thresholds - strong defaults applied.

| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| ---------- | ------------------ | -------------------- | ---------------- | ----------- |
| wai-ext manifest and permissions | unit | Every new permission parsed, displayed and refused when invalid | `crates/wai-ext/tests/manifest.rs` | `cargo test -p wai-ext --test manifest` |
| wai-ext host protocol (methods, events) | integration (fake extension process) | Every new method and event: granted, denied, forwarded | `crates/wai-ext/tests/host.rs`, `crates/wai-ext/src/bin/wai-fake-ext.rs` | `cargo test -p wai-ext --test host` |
| wai pure logic (`ext_sessions.rs`) | unit | All branches; 1:1 to HOST-01/02, QST-05 | `#[cfg(test)] mod tests` in the file | `cargo test -p wai --bin wai -- ext_sessions` |
| wai UI wiring (ChatSession, Workspace) | integration (GPUI `TestAppContext` + `wai-fake-agent`) | Each HOST/QST criterion: happy + error path | `mod tests` in `workspace.rs`, `chat/view.rs`, `extensions_view.rs` | `cargo test -p wai --bin wai -- remote_ consent_` |
| wai-remoto (Go) | unit + HTTP (`httptest`) | Every route: happy + every listed edge + error; panel trees | `extensions/wai-remoto/*_test.go` | `go test -v ./...` |
| wai-mobile stores, client, models (Dart) | unit | All branches; 1:1 to REMO/QST/DEV ACs | `test/state/`, `test/api/` | `flutter test` |
| wai-mobile screens | widget | Every control's gesture and every state the spec names | `test/screens/` | `flutter test` |
| Docs, manifests, CI config | none | build gate only | - | build gate only |

## Gate Check Commands

> Generated from codebase - confirm before Execute.

| Gate Level | When to Use | Command |
| ---------- | ----------- | ------- |
| Quick | Rust unit, Go or Dart tasks | the task's own run line: `cargo test -p wai --bin wai -- ext_sessions` / `cd extensions/wai-remoto && go test ./...` / `cd ../wai-mobile && flutter test` |
| Full | Host protocol and UI wiring tasks | `cargo test -p wai-ext --test host --test manifest && cargo test -p wai --bin wai -- ext_sessions remote_ consent_` |
| Build | End of each phase, docs and config tasks | `**Build**` line above, plus the quick gate of every layer the phase touched |

---

## Execution Plan

### Phase 1: Extension API 1.10 in wai-ext

```
T1 → T2 → T3 → T4
```

### Phase 2: Session content and answers in wai

```
T5 → T6 → T7 → T8 → T9 → T10 → T11 → T12
```

### Phase 3: wai-remoto core

```
T13 → T14 → T15 → T16 → T17
```

### Phase 4: wai-remoto API and panel

```
T18 → T19 → T20 → T21 → T22 → T23
```

### Phase 5: wai-mobile core

```
T24 → T25 → T26 → T27 → T28
```

### Phase 6: wai-mobile screens

```
T29 → T30 → T31 → T32 → T33
```

---

## Task Breakdown

### T1: Parse `listen` and `sessions:content`, API 1.10

**What**: Add `Permission::Listen` and `Permission::SessionsContent` (parse, `Display`), and set `API` to `"1.10"`.
**Where**: `crates/wai-ext/src/manifest.rs`
**Depends on**: None
**Reuses**: `parse_permission` at `manifest.rs:338`, `Display` at `:238`
**Requirement**: PAIR-01, HOST-11

**Done when**:

- [x] `tests/manifest.rs`: `listen_and_sessions_content_parse` and `api_is_1_10` pass; existing manifest tests still pass
- [x] Gate check passes: `cargo test -p wai-ext --test manifest`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: add the listen and sessions:content extension permissions`

---

### T2: Consent text for the new permissions

**What**: Show "Listen for connections on a network port" and "Read your sessions' conversations" in the install consent.
**Where**: `crates/wai/src/extensions_view.rs`
**Depends on**: T1
**Reuses**: the permission → text match at `extensions_view.rs:40`
**Requirement**: PAIR-01, HOST-11

**Done when**:

- [x] `extensions_view::tests::consent_lists_listen` and `consent_lists_sessions_content` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- consent_`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: explain listen and sessions:content in the install consent`

---

### T3: Content events need `sessions:content`

**What**: Add `HostEvent::SessionUpdate`, `PermissionRequested`, `PermissionResolved`, `QuestionRequested`, `QuestionResolved`, each sent only to extensions granted `sessions:content`; the fake extension records them.
**Where**: `crates/wai-ext/src/host.rs`
**Depends on**: T2
**Reuses**: `HostEvent::message` at `host.rs:128`; `wai-fake-ext` event logging
**Requirement**: HOST-02, HOST-04, HOST-06, HOST-10, QST-01, QST-03

**Done when**:

- [x] `tests/host.rs`: `content_events_reach_content_extension` and `content_events_skip_read_only_extension` pass
- [x] Gate check passes: `cargo test -p wai-ext --test host`

**Tests**: integration
**Gate**: full
**Commit**: `feat: send session content events to extensions granted sessions:content`

---

### T4: The four new ext→host methods

**What**: `sessions/read` (`sessions:content`), `sessions/cancel`, `permission/answer`, `question/answer` (`sessions:control`) in `call`, as new `ExtEvent` variants carrying a `Reply`.
**Where**: `crates/wai-ext/src/host.rs`
**Depends on**: T3
**Reuses**: `call` at `host.rs:519`, `params_of`, `Reply`
**Requirement**: HOST-09

**Done when**:

- [x] `tests/host.rs`: `content_and_control_methods_need_permissions` and `content_and_control_methods_reach_wai` pass
- [x] Gate check passes: `cargo test -p wai-ext --test host`

**Tests**: integration
**Gate**: full
**Commit**: `feat: let extensions read sessions, cancel turns and answer cards`

---

### T5: Transcript JSON

**What**: `item_json(&ChatItem)` and `page(items, from, limit)` (default the last 200, `limit` ≤ 500) in a new module.
**Where**: `crates/wai/src/ext_sessions.rs`
**Depends on**: None - starts once Phase 1 is done
**Reuses**: `ChatItem` (`chat/model.rs:41`), `QuestionRequest` (`wai-agent/src/types.rs:294`)
**Requirement**: HOST-01

**Done when**:

- [x] `ext_sessions::tests::item_json_covers_every_kind`, `tool_item_has_no_output`, `page_defaults_to_last_200` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- ext_sessions`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: serialize a session's transcript for extensions`

---

### T6: Question answer validation

**What**: `check_answer(&QuestionRequest, &Map)`: required fields present, values fit kind and choices; the error names the field.
**Where**: `crates/wai/src/ext_sessions.rs`
**Depends on**: T5
**Reuses**: `FieldKind`, `Choice`
**Requirement**: QST-05

**Done when**:

- [x] `ext_sessions::tests::check_answer_needs_required_fields` and `check_answer_rejects_values_outside_kind` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- ext_sessions`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: validate an extension's answer to an agent question`

---

### T7: Update coalescing

**What**: `Feed`: per-session dirty item indexes, flushed at most once every 100 ms, with time injected.
**Where**: `crates/wai/src/ext_sessions.rs`
**Depends on**: T6
**Reuses**: the injected-`Instant` pattern of `ext_host.rs` `allow_burst` tests
**Requirement**: HOST-02

**Done when**:

- [x] `ext_sessions::tests::feed_flushes_at_most_every_100ms` and `feed_merges_repeated_indexes` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- ext_sessions`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: coalesce session updates for extensions`

---

### T8: ChatSession reports item and card changes

**What**: Emit `ChatEvent::ItemsChanged`, `PermissionAsked`, `PermissionAnswered {id, option}`, `QuestionAsked`, `QuestionAnswered {id, answer}`; `option: None` when the agent resolves it.
**Where**: `crates/wai/src/chat/view.rs`
**Depends on**: T7
**Reuses**: `handle_event`, `handle_permission`, `answer` (`view.rs:657`, `:713`, `:1019`)
**Requirement**: HOST-04, HOST-06, QST-01, QST-03

**Done when**:

- [x] `chat::view::tests::remote_card_events_follow_clicks` and `remote_agent_resolution_has_no_option` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- remote_`

**Tests**: integration
**Gate**: full
**Commit**: `feat: report transcript and card changes from a session`

---

### T9: Workspace broadcasts the content events

**What**: Route the T8 events into `HostEvent`s, session updates through `Feed`.
**Where**: `crates/wai/src/workspace.rs`
**Depends on**: T8
**Reuses**: the `ChatEvent::StateChanged` broadcast at `workspace.rs:1327`
**Requirement**: HOST-02, HOST-04, HOST-06, QST-01, QST-03

**Done when**:

- [x] `workspace::tests::remote_turn_streams_session_update`, `remote_permission_requested_event`, `remote_permission_resolved_event`, `remote_question_requested_event`, `remote_question_resolved_event` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- remote_`

**Tests**: integration
**Gate**: full
**Commit**: `feat: broadcast session content to extensions`

---

### T10: `sessions/read` and `sessions/cancel` in wai

**What**: Handle `ExtEvent::ReadSession` (page or "No open session <agent> <session>") and `ExtEvent::Cancel` (`ChatSession::stop`).
**Where**: `crates/wai/src/workspace.rs`
**Depends on**: T9
**Reuses**: `session_call` at `workspace.rs:1914`, `ChatSession::stop`
**Requirement**: HOST-01, HOST-03, HOST-08

**Done when**:

- [x] `workspace::tests::remote_read_returns_transcript`, `remote_read_unknown_session_errors`, `remote_cancel_ends_turn_cancelled` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- remote_`

**Tests**: integration
**Gate**: full
**Commit**: `feat: serve sessions/read and sessions/cancel`

---

### T11: `permission/answer` and `question/answer` in wai

**What**: Answer a pending card through `ChatSession::answer` / `answer_question`; error when not pending; `check_answer` before accept.
**Where**: `crates/wai/src/workspace.rs`
**Depends on**: T10
**Reuses**: `ChatSession::answer`, `answer_question`, `ext_sessions::check_answer`
**Requirement**: HOST-05, HOST-07, QST-02, QST-04, QST-05

**Done when**:

- [x] `workspace::tests::remote_permission_answer_reaches_agent`, `remote_permission_answer_twice_errors`, `remote_question_answer_reaches_agent`, `remote_question_answer_twice_errors`, `remote_question_answer_invalid_stays_pending` pass
- [x] Gate check passes: `cargo test -p wai --bin wai -- remote_`

**Tests**: integration
**Gate**: full
**Commit**: `feat: let extensions answer permission and question cards`

---

### T12: Document extension API 1.10

**What**: Permissions table, methods, events and the `api` field line for 1.10.
**Where**: `docs/extensions.md`
**Depends on**: T11
**Reuses**: existing table formats
**Requirement**: HOST-01, HOST-09, HOST-11

**Done when**:

- [x] Every method, event and permission from T1-T4 is documented
- [x] Gate check passes: Build line

**Tests**: none
**Gate**: build
**Commit**: `docs: document extension API 1.10`

---

### T13: wai-remoto process skeleton

**What**: Manifest (`api = "1.10"`, permissions, sidebar panel `remoto`), `go.mod` with `rsc.io/qr v0.2.0`, and the stdio JSON-RPC loop with a test host over pipes.
**Where**: `extensions/wai-remoto/main.go`
**Depends on**: None - starts once Phase 2 is done
**Reuses**: `extensions/wai-rotinas/main.go` loop
**Requirement**: PAIR-01

**Done when**:

- [x] `TestInitializeAndRender` passes; `crates/wai-ext/tests/manifest.rs` asserts the new wai-remoto manifest
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: start wai-remoto's process`

---

### T14: Device store

**What**: `state.json` (`0600`) with port, tunnel URL and devices; token lookup by SHA-256; 30-day idle expiry; renew on use; revoke one or all.
**Where**: `extensions/wai-remoto/store.go`
**Depends on**: T13
**Reuses**: `WAI_EXT_DATA`
**Requirement**: DEV-02, DEV-03, DEV-04, DEV-05

**Done when**:

- [x] `TestStoreKeepsOnlyTokenHash`, `TestTokenExpiresAfter30DaysIdle`, `TestTokenUseRenewsExpiry`, `TestRevokeOneAndAll` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: store wai-remoto devices by token hash`

---

### T15: Pairing codes

**What**: One code at a time: 16 random bytes, 5 minutes, single use; redeeming it with a device name creates a device and a token.
**Where**: `extensions/wai-remoto/pair.go`
**Depends on**: T14
**Reuses**: T14 store
**Requirement**: PAIR-09, PAIR-10

**Done when**:

- [x] `TestPairIssuesTokenAndUsesCode`, `TestPairRejectsUnknownExpiredUsedCode` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: issue one-time pairing codes in wai-remoto`

---

### T16: QR rows

**What**: `qrRows(payload)` → `pixels` rows (`#` dark, level M, 4-module quiet zone), ≤ 64 rows for the longest expected payload.
**Where**: `extensions/wai-remoto/qr.go`
**Depends on**: T15
**Reuses**: `rsc.io/qr` `Encode`, `Code.Black`
**Requirement**: PAIR-08

**Done when**:

- [x] `TestQRRowsFitPixelsNode` and `TestQRRowsHaveFinderPatterns` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: draw wai-remoto's pairing QR code`

---

### T17: Request guard

**What**: Middleware: `Origin` → 403, `Host` → 421, body > 64 KB → 413, Bearer token → 401 (counted; 20 in an hour → one `notify`), `Cache-Control: no-store`.
**Where**: `extensions/wai-remoto/auth.go`
**Depends on**: T16
**Reuses**: T14 store
**Requirement**: EDGE-01, EDGE-02, EDGE-03, EDGE-04, EDGE-06, EDGE-09

**Done when**:

- [x] `TestOriginHeaderIs403`, `TestUnknownHostIs421`, `TestMissingOrUnknownTokenIs401`, `TestBodyOver64KBIs413`, `TestTwentyFailuresNotifyOnce`, `TestResponsesAreNoStore` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: guard wai-remoto's requests`

---

### T18: Session routes

**What**: `GET /v1/sessions`, `GET /v1/sessions/{a}/{s}`, `POST .../prompt` (409 unless idle, 30/min/device → 429), `POST .../cancel`, `POST /v1/pair`, `POST /v1/unpair`.
**Where**: `extensions/wai-remoto/routes.go`
**Depends on**: None - starts once Phase 3 is done
**Reuses**: host calls from T13
**Requirement**: REMO-01, REMO-08, REMO-10, REMO-14, EDGE-08, PAIR-09, DEV-06

**Done when**:

- [x] `TestSessionsListForwardsHost`, `TestReadForwardsHost`, `TestPromptForwardsToIdleSession`, `TestPromptToBusySessionIs409`, `TestPromptRateLimit429`, `TestCancelForwards`, `TestPairRoute`, `TestUnpairRevokesOwnToken` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: serve wai-remoto's session routes`

---

### T19: Answer routes

**What**: `POST .../permissions/{r}` and `POST .../questions/{r}`: host error "already answered" → 409, invalid field → 422 `{field}`.
**Where**: `extensions/wai-remoto/answers.go`
**Depends on**: T18
**Reuses**: T18 helpers
**Requirement**: REMO-12, QST-02, QST-05, QST-08

**Done when**:

- [x] `TestPermissionAnswerForwards`, `TestAnsweredPermissionIs409`, `TestQuestionAnswerForwards`, `TestInvalidQuestionAnswerIs422` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: answer cards through wai-remoto`

---

### T20: Event stream

**What**: `GET /v1/events` SSE hub: forwards the host events, pings every 15 s, at most 4 streams per device, closes a device's streams on revoke.
**Where**: `extensions/wai-remoto/events.go`
**Depends on**: T19
**Reuses**: T14 revoke hooks
**Requirement**: REMO-02, REMO-04, EDGE-05, DEV-02, DEV-03

**Done when**:

- [x] `TestEventsForwardHostEvents`, `TestFifthStreamIs429`, `TestRevokeClosesStreams` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: stream session events from wai-remoto`

---

### T21: Listener lifecycle

**What**: Start (validate port 1024-65535 and `https://` URL, bind `127.0.0.1` only, port-in-use message) and Stop (close listener and streams within 1 s).
**Where**: `extensions/wai-remoto/server.go`
**Depends on**: T20
**Reuses**: `net.Listen`, `http.Server.Shutdown`
**Requirement**: PAIR-03, PAIR-04, PAIR-05, PAIR-06, PAIR-07, PAIR-12

**Done when**:

- [x] `TestStartRejectsPortOutOfRange`, `TestStartRejectsNonHTTPSTunnel`, `TestStartListensOnLoopbackOnly`, `TestStartReportsPortInUse`, `TestNothingListensBeforeStart`, `TestStopClosesListenerAndStreams` pass
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: start and stop wai-remoto's listener`

---

### T22: Panel

**What**: The `remoto` panel tree: port and URL fields, Start/Stop, status, `tailscale serve` command, "Pair a device" with QR and time left, devices with Revoke, "Sign out all devices", connected count; its `panel/action` handling.
**Where**: `extensions/wai-remoto/panel.go`
**Depends on**: T21
**Reuses**: panel nodes from `docs/extensions.md`; T16 rows
**Requirement**: PAIR-02, PAIR-08, PAIR-11, DEV-01, DEV-02, DEV-03

**Done when**:

- [x] `TestPanelShowsSettingsAndServeCommand`, `TestPairShowsQRWithURLAndCode`, `TestPanelListsPairedDevice`, `TestPanelCountsConnectedDevices`, `TestPanelRevokeAndSignOutAll` pass
- [x] The `tailscale serve` command is checked against Tailscale's docs and recorded in the README
- [x] Gate check passes: `cd extensions/wai-remoto && go test ./...`

**Tests**: unit
**Gate**: quick
**Commit**: `chore: build wai-remoto's panel`

---

### T23: wai-remoto docs and CI

**What**: README (setup with Tailscale, pairing), DESIGN.md moved to the token model, `go vet` + `go test` for wai-remoto in `ci.yml`.
**Where**: `extensions/wai-remoto/README.md`
**Depends on**: T22
**Reuses**: wai-rotinas README shape; `ci.yml` steps
**Requirement**: PAIR-02

**Done when**:

- [x] README, DESIGN.md and `ci.yml` updated
- [x] Gate check passes: Build line

**Tests**: none
**Gate**: build
**Commit**: `chore: document wai-remoto and test it in CI`

---

### T24: wai-mobile project and models

**What**: `flutter create` (iOS + Android), `git init`, packages `http`, `mobile_scanner`, `flutter_secure_storage`; `models.dart` parsing every transcript item kind and event.
**Where**: `../wai-mobile/lib/api/models.dart`
**Depends on**: None - starts once Phase 4 is done
**Reuses**: JSON shapes from design.md Data Models
**Requirement**: REMO-03

**Done when**:

- [x] `test/api/models_test.dart`: "models parse every item kind" and "models parse every event" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: start wai-mobile with the wai-remoto models`

---

### T25: API client

**What**: HTTP calls with Bearer token and status mapping (401, 409, 422, 429, unreachable) and an SSE parser with 2 s reconnect.
**Where**: `../wai-mobile/lib/api/client.dart`
**Depends on**: T24
**Reuses**: `package:http/testing.dart` `MockClient`
**Requirement**: REMO-06, REMO-07, REMO-14, EDGE-08

**Done when**:

- [x] "client maps statuses to errors" and "client parses an SSE stream" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: talk to wai-remoto over HTTP and SSE`

---

### T26: Sessions store

**What**: List state: sessions with label, agent and state; applies `session_state`; empty, unreachable and unpaired states.
**Where**: `../wai-mobile/lib/state/sessions_store.dart`
**Depends on**: T25
**Reuses**: T25 client
**Requirement**: REMO-01, REMO-02, REMO-05, REMO-06, REMO-07

**Done when**:

- [x] "SessionsStore labels title agent and state", "SessionsStore applies session_state", "SessionsStore is empty with no sessions", "SessionsStore goes offline when unreachable", "SessionsStore unpairs on 401" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: keep wai's session list on the phone`

---

### T27: Transcript store

**What**: One session's items: snapshot, `session_update`, card requested/resolved ("Answered in wai: <option>", "Answered in wai"), send enabled only when idle, busy and rate-limit messages.
**Where**: `../wai-mobile/lib/state/transcript_store.dart`
**Depends on**: T26
**Reuses**: T25 client
**Requirement**: REMO-04, REMO-09, REMO-13, REMO-14, QST-10, EDGE-08

**Done when**:

- [x] "TranscriptStore applies session_update", "TranscriptStore allows sending only when idle", "TranscriptStore marks a permission answered in wai", "TranscriptStore marks a question answered in wai", "TranscriptStore reports busy and rate limit" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: follow a session's transcript live`

---

### T28: Question form state

**What**: Values per field kind, Submit enabled only when every required field has a value, answer payloads for accept, decline and cancel, field error from 422.
**Where**: `../wai-mobile/lib/state/question_state.dart`
**Depends on**: T27
**Reuses**: T24 models
**Requirement**: QST-07, QST-08, QST-09, QST-05

**Done when**:

- [x] "QuestionState disables Submit while a required field is empty", "QuestionState builds accept values", "QuestionState builds decline and cancel", "QuestionState marks the field from 422" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: unit
**Gate**: quick
**Commit**: `feat: hold an agent question's answers`

---

### T29: Pairing screen

**What**: First-launch screen with "Scan QR code"; scans `wai-remoto:pair?...`, pairs, stores URL and token in secure storage, opens the list; "Pairing failed: scan a new code" on failure.
**Where**: `../wai-mobile/lib/screens/pair_screen.dart`
**Depends on**: None - starts once Phase 5 is done
**Reuses**: `mobile_scanner`, `flutter_secure_storage` behind an interface faked in tests
**Requirement**: PAIR-13, PAIR-14, PAIR-15

**Done when**:

- [x] "PairScreen shows Scan QR code on first launch", "PairScreen stores url and token and opens sessions", "PairScreen shows Pairing failed on 401" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: widget
**Gate**: quick
**Commit**: `feat: pair the phone by QR code`

---

### T30: Sessions screen

**What**: Renders the sessions store: rows, "No open sessions in wai", the unreachable message with Retry; tapping a row opens the session.
**Where**: `../wai-mobile/lib/screens/sessions_screen.dart`
**Depends on**: T29
**Reuses**: T26 store
**Requirement**: REMO-01, REMO-05, REMO-06

**Done when**:

- [x] "SessionsScreen lists sessions", "SessionsScreen shows No open sessions in wai", "SessionsScreen shows can't reach wai with Retry" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: widget
**Gate**: quick
**Commit**: `feat: list wai's sessions on the phone`

---

### T31: Session screen

**What**: Transcript as plain text, composer, Cancel while working, permission card with one button per option, busy and rate-limit messages.
**Where**: `../wai-mobile/lib/screens/session_screen.dart`
**Depends on**: T30
**Reuses**: T27 store
**Requirement**: REMO-03, REMO-08, REMO-09, REMO-10, REMO-11, REMO-12, REMO-13, REMO-14, EDGE-08

**Done when**:

- [x] "SessionScreen shows agent output as text", "SessionScreen sends a prompt", "SessionScreen disables send and shows Cancel while working", "SessionScreen Cancel calls cancel", "SessionScreen shows one button per permission option", "SessionScreen sends the tapped option" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: widget
**Gate**: quick
**Commit**: `feat: drive a session from the phone`

---

### T32: Question form

**What**: One control per field kind (single list, multi list, text, switch, number) with Submit, Decline and Cancel.
**Where**: `../wai-mobile/lib/screens/question_form.dart`
**Depends on**: T31
**Reuses**: T28 state
**Requirement**: QST-06, QST-08, QST-09

**Done when**:

- [x] "QuestionForm shows one control per field kind", "QuestionForm Submit sends values", "QuestionForm Decline and Cancel send decline and cancel" pass
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: widget
**Gate**: quick
**Commit**: `feat: answer an agent's question from the phone`

---

### T33: Unpair

**What**: "Unpair" in the sessions screen's menu: revoke on wai-remoto, delete the stored token, show the pairing screen.
**Where**: `../wai-mobile/lib/screens/unpair.dart`
**Depends on**: T32
**Reuses**: T25 client, T29 storage interface
**Requirement**: DEV-06

**Done when**:

- [x] "Unpair revokes and returns to pairing" passes
- [x] Gate check passes: `cd ../wai-mobile && flutter test`

**Tests**: widget
**Gate**: quick
**Commit**: `feat: unpair the phone`

---

## Checks

### P1: Pair a phone

**C1** - The install consent lists `listen` as "Listen for connections on a network port" (PAIR-01, AC 1)
Proof: run wai | extensions_view::tests::consent_lists_listen ... ok

**C2** - The panel shows the port field, the tunnel URL field, Start and `tailscale serve --bg <port>` (PAIR-02, AC 2)
Proof: run remoto | --- PASS: TestPanelShowsSettingsAndServeCommand

**C3** - Start with a port outside 1024-65535 shows "Port must be between 1024 and 65535" and does not listen (PAIR-03, AC 3)
Proof: run remoto | --- PASS: TestStartRejectsPortOutOfRange

**C4** - Start with a non-`https://` tunnel URL shows "The tunnel URL must start with https://" and does not listen (PAIR-04, AC 4)
Proof: run remoto | --- PASS: TestStartRejectsNonHTTPSTunnel

**C5** - Start listens on `127.0.0.1:<port>` only and shows "Listening on 127.0.0.1:<port>" and Stop (PAIR-05, AC 5)
Proof: run remoto | --- PASS: TestStartListensOnLoopbackOnly

**C6** - A port in use shows "Port <port> is in use" and keeps Start (PAIR-06, AC 6)
Proof: run remoto | --- PASS: TestStartReportsPortInUse

**C7** - Before Start nothing listens (PAIR-07, AC 7)
Proof: run remoto | --- PASS: TestNothingListensBeforeStart

**C8** - "Pair a device" shows a QR of the tunnel URL and a new code, and the time left (PAIR-08, AC 8)
Proof: run remoto | --- PASS: TestPairShowsQRWithURLAndCode

**C9** - The QR rows fit the `pixels` node (≤ 64 rows) and carry the three finder patterns (PAIR-08, AC 8)
Proof: run remoto | --- PASS: TestQRRowsHaveFinderPatterns

**C10** - A valid code and device name return a token and mark the code used (PAIR-09, AC 9)
Proof: run remoto | --- PASS: TestPairIssuesTokenAndUsesCode

**C11** - An unknown, expired or used code answers 401 with no token (PAIR-10, AC 10)
Proof: run remoto | --- PASS: TestPairRoute

**C12** - A paired device appears in the panel with its name and pairing date (PAIR-11, AC 11)
Proof: run remoto | --- PASS: TestPanelListsPairedDevice

**C13** - Stop closes the listener and every stream within 1 s (PAIR-12, AC 12)
Proof: run remoto | --- PASS: TestStopClosesListenerAndStreams

**C14** - First launch shows the pairing screen with "Scan QR code" (PAIR-13, AC 13)
Proof: run mobile | ✅ PairScreen shows Scan QR code on first launch

**C15** - A successful scan stores URL and token in secure storage and opens the session list (PAIR-14, AC 14)
Proof: run mobile | ✅ PairScreen stores url and token and opens sessions

**C16** - A failed pairing shows "Pairing failed: scan a new code" and stays on the pairing screen (PAIR-15, AC 15)
Proof: run mobile | ✅ PairScreen shows Pairing failed on 401

### P1: Follow sessions

**C17** - `sessions/read` returns user, agent and tool items (title and status, no output) in order (HOST-01, AC 1)
Proof: run wai | workspace::tests::remote_read_returns_transcript ... ok

**C18** - Every `ChatItem` kind serializes, and a tool item carries no output (HOST-01, AC 1)
Proof: run wai | ext_sessions::tests::tool_item_has_no_output ... ok

**C19** - A running turn sends `event/session_update` to a `sessions:content` extension (HOST-02, AC 2)
Proof: run wai | workspace::tests::remote_turn_streams_session_update ... ok

**C20** - Session updates are flushed at most once every 100 ms per session (HOST-02, AC 2)
Proof: run wai | ext_sessions::tests::feed_flushes_at_most_every_100ms ... ok

**C21** - `sessions/read` for a session that is not open answers an error naming it (HOST-03, AC 3)
Proof: run wai | workspace::tests::remote_read_unknown_session_errors ... ok

**C22** - The app lists each session with its title (or agent name), agent and state (REMO-01, AC 4)
Proof: run mobile | ✅ SessionsStore labels title agent and state

**C23** - `GET /v1/sessions` returns the host's `sessions/list` (REMO-01, AC 4)
Proof: run remoto | --- PASS: TestSessionsListForwardsHost

**C24** - A `session_state` event changes the state shown in the list (REMO-02, AC 5)
Proof: run mobile | ✅ SessionsStore applies session_state

**C25** - wai-remoto forwards every host event on `/v1/events` (REMO-02, AC 5)
Proof: run remoto | --- PASS: TestEventsForwardHostEvents

**C26** - Agent output is rendered as plain text, never HTML (REMO-03, AC 6)
Proof: run mobile | ✅ SessionScreen shows agent output as text

**C27** - A `session_update` appends and replaces items in the open session (REMO-04, AC 7)
Proof: run mobile | ✅ TranscriptStore applies session_update

**C28** - With no sessions the app shows "No open sessions in wai" (REMO-05, AC 8)
Proof: run mobile | ✅ SessionsScreen shows No open sessions in wai

**C29** - Unreachable wai-remoto shows the "Can't reach wai…" message with Retry (REMO-06, AC 9)
Proof: run mobile | ✅ SessionsScreen shows can't reach wai with Retry

**C30** - A 401 deletes the stored token and shows the pairing screen (REMO-07, AC 10)
Proof: run mobile | ✅ SessionsStore unpairs on 401

### P1: Drive a session

**C31** - A permission request sends `event/permission_requested` with session, id, title and options (HOST-04, AC 1)
Proof: run wai | workspace::tests::remote_permission_requested_event ... ok

**C32** - `permission/answer` answers the agent with the option and closes wai's card as answered (HOST-05, AC 2)
Proof: run wai | workspace::tests::remote_permission_answer_reaches_agent ... ok

**C33** - An answer in wai or by an extension sends `event/permission_resolved` with the option (HOST-06, AC 3)
Proof: run wai | workspace::tests::remote_permission_resolved_event ... ok

**C34** - `permission/answer` for an answered or unknown request errors and leaves the agent's answer (HOST-07, AC 4)
Proof: run wai | workspace::tests::remote_permission_answer_twice_errors ... ok

**C35** - `sessions/cancel` ends a running turn with reason `cancelled` (HOST-08, AC 5)
Proof: run wai | workspace::tests::remote_cancel_ends_turn_cancelled ... ok

**C36** - Each new method without its permission answers `-32001` `Permission denied: <permission>` (HOST-09, AC 6)
Proof: run ext | test content_and_control_methods_need_permissions ... ok

**C37** - An extension with `sessions:read` but not `sessions:content` receives none of the five content events (HOST-10, AC 7)
Proof: run ext | test content_events_skip_read_only_extension ... ok

**C38** - The install consent lists `sessions:content` as "Read your sessions' conversations" (HOST-11, AC 8)
Proof: run wai | extensions_view::tests::consent_lists_sessions_content ... ok

**C39** - A prompt to an idle session is forwarded as `sessions/prompt` (REMO-08, AC 7)
Proof: run remoto | --- PASS: TestPromptForwardsToIdleSession

**C40** - The app sends the typed prompt to the open session (REMO-08, AC 7)
Proof: run mobile | ✅ SessionScreen sends a prompt

**C41** - While working, send is disabled and Cancel shows (REMO-09, AC 8)
Proof: run mobile | ✅ SessionScreen disables send and shows Cancel while working

**C42** - Tapping Cancel calls cancel, which wai-remoto forwards as `sessions/cancel` (REMO-10, AC 9)
Proof: run remoto | --- PASS: TestCancelForwards

**C43** - The Cancel button calls the cancel route (REMO-10, AC 9)
Proof: run mobile | ✅ SessionScreen Cancel calls cancel

**C44** - A pending permission shows its title and one button per option (REMO-11, AC 10)
Proof: run mobile | ✅ SessionScreen shows one button per permission option

**C45** - Tapping an option sends it, and wai-remoto forwards it as `permission/answer` (REMO-12, AC 11)
Proof: run remoto | --- PASS: TestPermissionAnswerForwards

**C46** - The app sends the tapped option to the permission route (REMO-12, AC 11)
Proof: run mobile | ✅ SessionScreen sends the tapped option

**C47** - A permission answered in wai shows "Answered in wai: <option>" in the app (REMO-13, AC 12)
Proof: run mobile | ✅ TranscriptStore marks a permission answered in wai

**C48** - A 31st prompt in a minute from one device answers 429 (REMO-14, AC 13)
Proof: run remoto | --- PASS: TestPromptRateLimit429

**C49** - A 429 shows "Too many prompts, wait a minute" and a 409 shows "The session is busy" (REMO-14, AC 13)
Proof: run mobile | ✅ TranscriptStore reports busy and rate limit

### P2: Answer an agent's question

**C50** - A question sends `event/question_requested` with message and every field's key, title, description, required, kind and choices (QST-01, AC 1)
Proof: run wai | workspace::tests::remote_question_requested_event ... ok

**C51** - `question/answer` accept, decline or cancel reaches the agent and closes wai's card (QST-02, AC 2)
Proof: run wai | workspace::tests::remote_question_answer_reaches_agent ... ok

**C52** - An answered question sends `event/question_resolved` with the answer kind (QST-03, AC 3)
Proof: run wai | workspace::tests::remote_question_resolved_event ... ok

**C53** - `question/answer` for an answered or unknown request errors and changes nothing (QST-04, AC 4)
Proof: run wai | workspace::tests::remote_question_answer_twice_errors ... ok

**C54** - An accept missing a required field, or with a value outside its kind or choices, errors naming the field and keeps the question pending (QST-05, AC 5)
Proof: run wai | workspace::tests::remote_question_answer_invalid_stays_pending ... ok

**C55** - `check_answer` rejects a value outside the field's kind or choices (QST-05, AC 5)
Proof: run wai | ext_sessions::tests::check_answer_rejects_values_outside_kind ... ok

**C56** - The form shows one control per field kind with Submit, Decline and Cancel (QST-06, AC 6)
Proof: run mobile | ✅ QuestionForm shows one control per field kind

**C57** - Submit is disabled while a required field has no value (QST-07, AC 7)
Proof: run mobile | ✅ QuestionState disables Submit while a required field is empty

**C58** - Submit sends the entered values, which wai-remoto forwards as `question/answer` accept (QST-08, AC 8)
Proof: run remoto | --- PASS: TestQuestionAnswerForwards

**C59** - The form's Submit sends the entered values (QST-08, AC 8)
Proof: run mobile | ✅ QuestionForm Submit sends values

**C60** - Decline and Cancel send `decline` and `cancel` (QST-09, AC 9)
Proof: run mobile | ✅ QuestionForm Decline and Cancel send decline and cancel

**C61** - A question answered in wai shows "Answered in wai" in the app (QST-10, AC 10)
Proof: run mobile | ✅ TranscriptStore marks a question answered in wai

### P2: Manage devices

**C62** - The panel shows "<n> devices connected" counting devices with a live stream (DEV-01, AC 1)
Proof: run remoto | --- PASS: TestPanelCountsConnectedDevices

**C63** - Revoke deletes the token and closes the device's streams within 1 s (DEV-02, AC 2)
Proof: run remoto | --- PASS: TestRevokeClosesStreams

**C64** - "Sign out all devices" deletes every token and closes every stream within 1 s (DEV-03, AC 3)
Proof: run remoto | --- PASS: TestPanelRevokeAndSignOutAll

**C65** - A token unused for 30 days answers 401 and its device leaves the list (DEV-04, AC 4)
Proof: run remoto | --- PASS: TestTokenExpiresAfter30DaysIdle

**C66** - An authenticated request restarts the token's 30-day expiry (DEV-05, AC 5)
Proof: run remoto | --- PASS: TestTokenUseRenewsExpiry

**C67** - "Unpair" revokes the token, deletes it locally and shows the pairing screen (DEV-06, AC 6)
Proof: run mobile | ✅ Unpair revokes and returns to pairing

**C68** - `POST /v1/unpair` revokes the calling device's token only (DEV-06, AC 6)
Proof: run remoto | --- PASS: TestUnpairRevokesOwnToken

### Edge cases

**C69** - A request with an `Origin` header answers 403 (EDGE-01)
Proof: run remoto | --- PASS: TestOriginHeaderIs403

**C70** - A `Host` that is neither the tunnel host nor `127.0.0.1:<port>` answers 421 (EDGE-02)
Proof: run remoto | --- PASS: TestUnknownHostIs421

**C71** - A missing or unknown token answers 401 (EDGE-03)
Proof: run remoto | --- PASS: TestMissingOrUnknownTokenIs401

**C72** - A body over 64 KB answers 413 (EDGE-04)
Proof: run remoto | --- PASS: TestBodyOver64KBIs413

**C73** - A fifth event stream from one device answers 429 (EDGE-05)
Proof: run remoto | --- PASS: TestFifthStreamIs429

**C74** - 20 failed authentications in an hour send one `notify` (EDGE-06)
Proof: run remoto | --- PASS: TestTwentyFailuresNotifyOnce

**C75** - Disabling wai-remoto in wai stops its process and frees the port (EDGE-07)
Proof: walk - Extensions → wai-remoto → Disable while started; `Test-NetConnection 127.0.0.1 -Port 7717` fails afterwards

**C76** - A prompt to a session that is not idle answers 409 (EDGE-08)
Proof: run remoto | --- PASS: TestPromptToBusySessionIs409

**C77** - Every API response carries `Cache-Control: no-store` (EDGE-09)
Proof: run remoto | --- PASS: TestResponsesAreNoStore

---

## Handoff

The feature's live state, written as it happens - a compaction gives no warning.

**Settled mid-build:** The `**Build**` line keeps only the Rust build: wai-remoto and wai-mobile do not exist before T13 and T24, so the `--execute` run of the Build line would be red before any change. `go vet` and `flutter analyze` run in those phases' build gates. Execution: one sub-agent per phase, in order (user: "Subagente por lote"). Branch `feat/remote-access` from `origin/development`; the wai-github WIP is in `stash@{0}` "wip wai-github (antes do remote-access)" (user: "Nova branch, WIP em stash"). Phase 1: `wai` is a bin-only crate, so every `cargo test -p wai --lib` became `cargo test -p wai --bin wai`; the API bump changed two existing tests' expected version (`api_version_is_checked_first` now tests 1.11 as newer, `initialize_roundtrip` expects 1.10), as the 1.9 bump did; T1 alone does not build `wai` (T2 completes it). Phase 2: the transcript is `items` followed by pending cards (`model.waiting`); host error strings for wai-remoto: `Request <id> is already answered or unknown`, `Field <key>: <why>`, `No option <option> in request <id>`, `Unknown answer <x>: use accept, decline or cancel`, `No open session <agent> <session>`; an agent-resolved question sends `answer: "cancel"`. Phase 3: C11's proof moved to `TestPairRoute` (T18), which asserts the 401 for unknown, expired and used codes; `TestPairRejectsUnknownExpiredUsedCode` covers the redeem logic. On Windows `state.json`'s mode is not enforceable (Go reports 0666); the file lives in the user's `%APPDATA%`. Phase 4: `tailscale serve --bg <port>` confirmed in Tailscale's docs; whether it keeps the original `Host` is still unknown (both hosts accepted, check on the walk). wai-remoto statuses beyond the spec: other host errors 400/502, no host answer in 10 s 504, prompt to an unknown session 404 `No open session <agent> <session>`. Phase 5: `flutter test --reporter github` prints `✅ <file>: <name>`, which the `run mobile` proofs still match; a 502/503/504 without wai-remoto's JSON body is `Unreachable` (what `tailscale serve` answers when wai-remoto is stopped); an agent-withdrawn permission shows `Cancelled`. Go 1.27.4 from winget (`C:\Program Files\Go\bin`); Flutter stable cloned to `C:\Users\guilh\dev\flutter` (not in winget).
**Dispatched:** batch worker Phase 1 (T1-T4) at `7588aa9` - returned: T1 77e821b, T2 3d51db8, T3 d33c39b, T4 ac91b14; batch worker Phase 2 (T5-T12) at `1c228fd` - returned: T5 9d2dc14, T6 bb70454, T7 2841534, T8 0feb7d7, T9 d418238, T10 22cb1f4, T11 0930772, T12 889012b; batch worker Phase 3 (T13-T17) at `889012b` - returned: T13 86e34c9, T14 4289f4c, T15 56e9a15, T16 7a975df, T17 5917331; batch worker Phase 4 (T18-T23) at `5917331` - returned: T18 bc8d07d, T19 6e30170, T20 9e6272b, T21 31c0cbb, T22 8c75308, T23 c5a2b88; batch worker Phase 5 (T24-T28) at `c5a2b88` - returned: T24 befc372/5ae5c0e, T25 4f2e882/9984a09, T26 f3b93d4/6b17eab, T27 ccea359/c288446, T28 5a24088/ca45f39 (wai-mobile/wai); batch worker Phase 6 (T29-T33) at wai `ca45f39`, wai-mobile `5a24088` - returned: T29 ee76c96/c0d1e46, T30 33a50f1/76e0fc9, T31 79ba860/88022af, T32 4310534/1130f0d, T33 28703eb/3728f22
**Walked:** none

---

## Phase Execution Map

```
Phase 1 → Phase 2 → Phase 3 → Phase 4 → Phase 5 → Phase 6

Phase 1:  T1 → T2 → T3 → T4
Phase 2:  T5 → T6 → T7 → T8 → T9 → T10 → T11 → T12
Phase 3:  T13 → T14 → T15 → T16 → T17
Phase 4:  T18 → T19 → T20 → T21 → T22 → T23
Phase 5:  T24 → T25 → T26 → T27 → T28
Phase 6:  T29 → T30 → T31 → T32 → T33
```

Batches for sub-agents (~7 tasks, whole phases): Phase 1 (4) · Phase 2 (8) · Phases 3 (5) · Phase 4 (6) · Phases 5-6 split as Phase 5 (5) and Phase 6 (5).
