# Extension Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extensions emit named events (`events/emit`) that wai forwards as `event/ext` to the extensions the user granted `events:<emitter id>`.

**Architecture:** The ext→host call is parsed and validated in `wai-ext`'s host (`crates/wai-ext/src/host.rs`), like `notify`, and reaches wai as `ExtEvent::Emit`. `ExtHost` (`crates/wai/src/ext_host.rs`) rate-limits it per emitter and forwards it as `HostEvent::Ext` to every other running extension; `ExtHandle::notify_event` already drops events the listener was not granted. The new permission `events:<id>` is parsed like `fs:<path>`.

**Tech Stack:** Rust, gpui (gpui-kit), serde_json, the `wai-fake-ext` test binary.

**Spec:** `docs/superpowers/specs/2026-11-17-extension-events-design.md`

## Global Constraints

- Work in the worktree `.claude/worktrees/ext-events`, branch `feat/ext-events`. Do not touch the main checkout or other worktrees. Never bare `git stash`.
- Commits: `feat:` / `test:` / `docs:`, no scope, no `Co-Authored-By` line. Do not push, merge or tag.
- Extension API becomes `1.7` (`API` in `crates/wai-ext/src/manifest.rs`).
- Event name: `[a-z0-9][a-z0-9._-]{0,63}` (1 to 64 bytes).
- Event `data`: any JSON, default `null`; at most 16 KiB (16,384 bytes) as serialized by `serde_json::to_string`.
- At most 20 events per second per emitter; the excess is dropped and logged to stderr.
- Emitting needs no permission. Listening needs `events:<extension id>`, one per source; the source need not be installed.
- An emitter never receives its own events, even when granted `events:<its own id>`.
- Delivered message: `{"jsonrpc":"2.0","method":"event/ext","params":{"source","name","data"}}`; `source` is set by wai.
- wai's existing `event/*` notifications do not change.
- Before each commit: `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` clean. wai's tests need the fake binaries: `cargo build --workspace --bins` first.
- The disk is nearly full: run `cargo clean` in this worktree when done.

## Review Focus

1. A notification-form `events/emit` (no id) with a bad name or oversized data: dropped, logged, and the extension keeps working. Pinned in Task 2 (`emit_notification_with_bad_name_is_dropped`).
2. Payload exactly at the limit (16,384 bytes serialized) passes; one byte over fails. Pinned in Task 2 (`emit_checks_name_and_size`).
3. Name boundaries: 64 bytes pass, 65 fail; uppercase, a space, or a leading `.`/`-` fail. Pinned in Task 2.
4. An extension granted `events:<its own id>` does not hear itself. Pinned in Task 3.
5. A burst over 20 in one second delivers exactly 20, and the next second delivers again. Pinned in Task 3 (`allow_burst` unit test plus the routing test).

---

### Task 1: `events:<id>` permission and API 1.7

**Files:**
- Modify: `crates/wai-ext/src/manifest.rs` (`API`, `Permission`, its `Display`, `parse_permission`)
- Modify: `crates/wai/src/extensions_view.rs` (`permission_text`)
- Modify: `docs/extensions.md` (permission table near line 84, `api` row at line 48, `initialize` row at line 157)
- Test: `crates/wai-ext/tests/manifest.rs`, `crates/wai-ext/tests/host.rs:127`

**Interfaces:**
- Produces: `Permission::Events(ExtId)`, shown and parsed as `events:<id>`; `API == "1.7"`.

- [ ] **Step 1: Write the failing test** in `crates/wai-ext/tests/manifest.rs`:

```rust
#[test]
fn events_permission_names_one_extension() {
    let manifest = |perm: &str| {
        format!("id = \"pet\"\nname = \"Pet\"\nversion = \"1.0.0\"\npermissions = [\"{perm}\"]\n")
    };
    let m = parse_manifest(&manifest("events:wai-waycarbon"), Path::new("/x")).unwrap();
    let want = Permission::Events(ExtId::new("wai-waycarbon").unwrap());
    assert_eq!(m.permissions, [want.clone()]);
    assert_eq!(want.to_string(), "events:wai-waycarbon");
    for bad in ["events:", "events:Bad_Id", "events:-x", "events:a/b"] {
        assert_eq!(
            parse_manifest(&manifest(bad), Path::new("/x")).unwrap_err().to_string(),
            format!("events permission {bad:?} must name an extension id")
        );
    }
}
```

Also in the existing `api` test (around line 285): accept `"1.7"`, and make the too-new case `"1.8"` with the message `"Go Tools needs a newer wai (extension API 1.8; this wai has 1.7)"`; the `"2.0"` message ends in `this wai has 1.7`. In `crates/wai-ext/tests/host.rs:127`, the `initialize` params now say `"api": "1.7"`.

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p wai-ext --test manifest`
Expected: FAIL to compile (`no variant Events`).

- [ ] **Step 3: Implement**
  - `Permission::Events(ExtId)`; `Display` writes `events:{id}`.
  - In `parse_permission`, an `events:` prefix maps through `ExtId::new`; its error becomes `ManifestError::Unknown(format!("events permission {s:?} must name an extension id"))`.
  - `pub const API: &str = "1.7";`
  - `permission_text`: `Permission::Events(id) => format!("Receive events from the {} extension", id.as_str())`.
  - Docs:
    - permission row: `` | `events:<id>` | Receive `event/ext` from extension `<id>` (see [Events between extensions](#events-between-extensions)). `<id>` need not be installed. API 1.7 | ``;
    - `api` row: "this wai has 1.7", plus "to `"1.7"` when you use `events/emit` or an `events:<id>` permission";
    - `initialize` row: `"api": "1.7"`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p wai-ext && cargo build --workspace --bins && cargo test -p wai extensions_view`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-ext crates/wai/src/extensions_view.rs docs/extensions.md
git commit -m "feat: let extensions ask for another extension's events"
```

### Task 2: `events/emit` in the extension host

**Files:**
- Modify: `crates/wai-ext/src/host.rs` (`ExtEvent`, `HostEvent` + `message`, `Ext::call`, the `Err` branch of the incoming loop near line 892)
- Modify: `crates/wai-ext/src/bin/wai-fake-ext.rs` (new flag `--notification <method>[ <params>]`)
- Test: `crates/wai-ext/tests/host.rs`

**Interfaces:**
- Consumes: `Permission::Events(ExtId)` (Task 1).
- Produces:
  - `ExtEvent::Emit { name: String, data: Value }`;
  - `HostEvent::Ext { source: ExtId, name: String, data: Value }`, whose `message()` is `("event/ext", json!({"source": source.as_str(), "name": name, "data": data}), Some(Permission::Events(source)))`;
  - `pub const MAX_EVENT_DATA: usize = 16 * 1024;` in `host.rs`.

- [ ] **Step 1: Add the fake's flag**

The flag is `--notification <method>[ <params>]` (repeatable). After `initialize`, it sends each one with no id, before the `--call`s, so a reply to a call proves the notifications before it were handled. Document it in the fake's header comment, next to `--call`.

- [ ] **Step 2: Write the failing tests** in `crates/wai-ext/tests/host.rs`. All three are granted `[Process, Notify]` and use the existing `installed`/`until`/`reported` helpers.

`emit_reaches_the_host`:
- calls are `events/emit {"name": "pr.review", "data": {"n": 1}}` and `events/emit {"name": "ping"}`;
- the first two `ExtEvent::Emit` are `("pr.review", json!({"n": 1}))` and `("ping", Value::Null)`;
- replies 1 and 2 are `result: null`.

`emit_checks_name_and_size`:
- one call per case, ids 1 to 8, with names:
  1. `"a".repeat(64)`: ok
  2. `"a".repeat(65)`: error
  3. `"PR"`: error
  4. `"pr review"`: error
  5. `".pr"`: error
  6. `"-pr"`: error
- 7: name `"big"`, data a JSON string of `"x".repeat(16_382)` (16,384 bytes serialized): ok.
- 8: name `"big"`, data `"x".repeat(16_383)`: error.
- Expected:
  - exactly the `Emit`s of 1 and 7 arrive;
  - each bad name gets `{"code": -32602, "message": "invalid event name \"<name>\""}`;
  - 8 gets `{"code": -32602, "message": "event data larger than 16 KiB"}`.

`emit_notification_with_bad_name_is_dropped`:
- args are `--notification 'events/emit {"name": "Bad"}'` and `--call 'events/emit {"name": "good"}'`;
- once reply 1 (`null`) arrives, the only `Emit` received is `good`.

`ext_event_needs_its_permission`:
- granted `[Process, Notify, Events(ExtId::new("a"))]`;
- `handle.notify_event(HostEvent::Ext { source: a, name: "x", data: json!(1) })`, then the same with source `b`, then `HostEvent::FileSaved { path: "/fence".into() }`;
- the reports up to `event/file_saved` contain exactly one `event/ext`, with body `{"source": "a", "name": "x", "data": 1}`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -p wai-ext --test host -- emit_ ext_event_`
Expected: FAIL to compile (`no variant Emit`).

- [ ] **Step 4: Implement**
  - In `Ext::call`, `"events/emit"` parses `EmitParams { name: String, #[serde(default)] data: Value }`.
  - It checks the name with `fn valid_event_name(name: &str) -> bool`, which implements the regex above by hand, without a regex crate.
  - It checks `serde_json::to_string(&data)?.len() <= MAX_EVENT_DATA`.
  - Valid: `Call::Done(ExtEvent::Emit { .. })`.
  - Invalid: `RpcError::new(rpc::INVALID_PARAMS, …)` with the exact messages above.
  - In the loop's `Err(error)` branch, when `id` is `None`, `eprintln!("wai: {} {method}: {}", ext.id.as_str(), error.message)` before `respond`. A notification's error has nowhere else to go.

- In `crates/wai/src/ext_host.rs` `apply`, add `ExtEvent::Emit { .. } => return,` so the workspace compiles; Task 3 replaces it.

- [ ] **Step 5: Run tests**

Run: `cargo test -p wai-ext && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/wai-ext crates/wai/src/ext_host.rs
git commit -m "feat: let extensions emit events"
```

### Task 3: Route events between extensions in `ExtHost`

**Files:**
- Modify: `crates/wai/src/ext_host.rs` (`Running`, `ExtHost::apply`, a new `allow_burst`, constants next to `CALL_GAP`)
- Modify: `docs/extensions.md` (method tables near lines 161 and 176, a short "Events between extensions" section)
- Test: `crates/wai/src/ext_host.rs` tests module

**Interfaces:**
- Consumes: `ExtEvent::Emit`, `HostEvent::Ext` (Task 2); `Permission::Events` (Task 1).
- Produces: `const EMIT_LIMIT: u32 = 20; const EMIT_WINDOW: Duration = Duration::from_secs(1);`, `fn allow_burst(window: &mut Option<(Instant, u32)>, now: Instant) -> bool`, and a `Running.emitted: Option<(Instant, u32)>` field.

- [ ] **Step 1: Write the failing tests**

`allow_burst_lets_twenty_through_per_second` is a unit test with synthetic `Instant`s:
- from `t0`, 20 calls pass and the 21st fails;
- at `t0 + 999 ms` it still fails;
- at `t0 + 1 s` it passes again and opens a new window.

`ext_events_reach_granted_listeners` is a `#[gpui_kit::test]` that follows `session_events_follow_permissions`:
- install, all with `api = "1.7"`:
  - `emitter` with `["process", "events:emitter"]`;
  - `listener` with `["process", "events:emitter"]`;
  - `deaf` with `["process"]`;
- wait for all three `initialize` reports;
- through `workspace.read(cx).ext_host.update(cx, |h, cx| …)`, call `h.apply(&emitter, ExtEvent::Emit { name: "pr.review".into(), data: json!({"n": i}) }, cx)` for `i in 0..25`, then `h.broadcast(HostEvent::FileSaved { path: "/fence".into() })`;
- after each log shows `event/file_saved`:
  - `listener` has exactly 20 `event/ext` bodies, `{"source": "emitter", "name": "pr.review", "data": {"n": i}}` for `i in 0..20`, in order;
  - `deaf` and `emitter` have none.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo build --workspace --bins && cargo test -p wai -- ext_host::tests::allow_burst ext_host::tests::ext_events`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

In `apply`, handle `ExtEvent::Emit { name, data }`:
- if `allow_burst` refuses it on the emitter's `Running.emitted`: `eprintln!("wai: {} emits too often; dropped {name:?}", id.as_str())` and return;
- otherwise, for every `(other, running)` in `self.running` with `other != id`: `running.handle.notify_event(HostEvent::Ext { source: id.clone(), name: name.clone(), data: data.clone() })`, then return.

`allow_burst` is a fixed window: a new window starts when there is none or `now >= start + EMIT_WINDOW`, and a call passes while the window's count is `<= EMIT_LIMIT`.

Docs:
- host→ext table: `` | `event/ext` | notification | `{"source", "name", "data"}` | An event another extension emitted. API 1.7. Needs `events:<source>` | ``;
- ext→host table: `` | `events/emit` | none | `{"name", "data"?}` | `null` | ``;
- the new "Events between extensions" section, five lines from the spec's Design:
  - name and data limits;
  - 20 per second;
  - not to the emitter;
  - no queue for extensions that are not running;
  - the emitter documents its events in its README.

- [ ] **Step 4: Run the full suite**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit, then clean**

```bash
git add crates/wai/src/ext_host.rs docs/extensions.md
git commit -m "feat: forward extension events to granted extensions"
cargo clean
```
