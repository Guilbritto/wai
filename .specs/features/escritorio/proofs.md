# escritorio proofs

**HEAD**: a49eb9e
**Tree**: clean
**Shell**: /bin/sh
**Result**: 48 pass, 0 fail

| Check | Proof | Result |
| --- | --- | --- |
| C1 | `run go / --- PASS: TestNoSetupNoDial` | pass |
| C2 | `run go / --- PASS: TestSetupRejectsBadName` | pass |
| C3 | `run go / --- PASS: TestTokenRefusedShowsForm` | pass |
| C4 | `run go / --- PASS: TestUnreachableRetries` | pass |
| C5 | `run go / --- PASS: TestOfficeTreeShowsEveryone` | pass |
| C6 | `run go / --- PASS: TestKeyMovesOneTile` | pass |
| C7 | `run go / --- PASS: TestBlockedTileStays` | pass |
| C8 | `run go / --- PASS: TestMoveReachesOtherWithin200ms` | pass |
| C9 | `run go / --- PASS: TestRejoinAtLastPosition` | pass |
| C10 | `run go / --- PASS: TestSilentPeerLeaves` | pass |
| C11 | `run wai / test ext_scene::tests::viewport_keeps_margin ... ok` | pass |
| C12 | `run go / --- PASS: TestSecondLoginReplacesFirst` | pass |
| C13 | `run go / --- PASS: TestSetupSavedPrivate` | pass |
| C14 | `run go / --- PASS: TestChangeSetupDisconnectsAndPrefills` | pass |
| C15 | `run go / --- PASS: TestServerAcceptsOnConfiguredAddress` | pass |
| C16 | `run go / --- PASS: TestConfigRejectsShortToken` | pass |
| C17 | `run go / --- PASS: TestConfigRequiresTLSOffLoopback` | pass |
| C18 | `run go / --- PASS: TestWrongTokenRefusedAndLoggedWithoutToken` | pass |
| C19 | `run go / --- PASS: TestRateLimitDropsPast30` | pass |
| C20 | `run go / --- PASS: TestOversizeMessageCloses` | pass |
| C21 | `run go / --- PASS: TestFiftyFirstRefused` | pass |
| C22 | `run go / --- PASS: TestAuthenticate` | pass |
| C23 | `run go / --- PASS: TestIllegalMoveSnapsBack` | pass |
| C24 | `run go / --- PASS: TestLogsConnectDisconnectRefusal` | pass |
| C25 | `run wai / test ext_panel::tests::scene_draws_tiles_and_sprites ... ok` | pass |
| C26 | `run wai / test ext_scene::tests::tween_takes_150ms ... ok` | pass |
| C27 | `run wai / test ext_panel::tests::scene_keys_reach_extension ... ok` | pass |
| C28 | `run wai / test ext_panel::tests::scene_click_takes_focus ... ok` | pass |
| C29 | `run wai / test ext_scene::tests::viewport_centres_first_time ... ok` | pass |
| C30 | `run ext / test panel::tests::scene_rules_unsupported ... ok` | pass |
| C31 | `grep -q '/ \scene\' docs/extensions.md && grep -q '\panel/key\' docs/extensions.md && grep -q '"1.14"' docs/ex` | pass |
| C32 | `run go / --- PASS: TestPresenceListsNearby` | pass |
| C33 | `run go / --- PASS: TestEnterOfficeOpensPane` | pass |
| C34 | `run go / --- PASS: TestPresenceShowsSetupForm` | pass |
| C35 | `run go / --- PASS: TestPresenceOfflineReasons` | pass |
| C36 | `run go / --- PASS: TestPresenceAlone` | pass |
| C37 | `run go / --- PASS: TestDeriveFocused` | pass |
| C38 | `run go / --- PASS: TestDeriveNeedsYouWins` | pass |
| C39 | `run go / --- PASS: TestDeriveNone` | pass |
| C40 | `run go / --- PASS: TestStatusCarriesNoSessionData` | pass |
| C41 | `run go / --- PASS: TestStatusReachesOthersWithin1s` | pass |
| C42 | `run go / --- PASS: TestReconnectAfterServerRestart` | pass |
| C43 | `run go / --- PASS: TestConfigMapWithoutSpawnRefused` | pass |
| C44 | `run wai / test ext_panel::tests::scene_shared_tile_draws_both ... ok` | pass |
| C45 | `run go / --- PASS: TestNameRejectsControlChars` | pass |
| Build | `cargo fmt --all --check` | pass |
| Build | `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| Build | `test ! -d extensions/wai-escritorio // (cd extensions/wai-escritorio && test -z "$(gofmt -l .)" && go vet ./..` | pass |

## Output

`Run go` - exit 0

```
2026/11/26 23:08:07 connect addr=127.0.0.1:58069 name="Bob"
2026/11/26 23:08:07 disconnect addr=127.0.0.1:58068 name="Ghost"
2026/11/26 23:08:08 connect addr=127.0.0.1:58072 name="Cy"
2026/11/26 23:08:08 disconnect addr=127.0.0.1:58072 name="Cy"
2026/11/26 23:08:08 disconnect addr=127.0.0.1:58069 name="Bob"
--- PASS: TestSilentPeerLeaves (0.80s)
=== RUN   TestLogsConnectDisconnectRefusal
--- PASS: TestLogsConnectDisconnectRefusal (0.11s)
=== RUN   TestMoveFanOutWithin200ms
2026/11/26 23:08:08 connect addr=127.0.0.1:58077 name="Ana"
2026/11/26 23:08:08 connect addr=127.0.0.1:58078 name="Bob"
--- PASS: TestMoveFanOutWithin200ms (0.00s)
2026/11/26 23:08:08 disconnect addr=127.0.0.1:58077 name="Ana"
PASS
ok  	wai-escritorio/server	2.270s
```

`Run wai` - exit 0

```
test ext_panel::tests::scene_shared_tile_draws_both ... ok
test ext_panel::tests::scene_draws_tiles_and_sprites ... ok
test ext_panel::tests::scene_layer_cached_across_renders ... ok
test ext_panel::tests::scene_click_takes_focus ... ok
test ext_panel::tests::scene_cmd_keys_not_sent ... ok
test ext_panel::tests::scene_keys_reach_extension ... ok
test ext_panel::tests::scene_key_reaches_extension_as_panel_key ... ok
test ext_panel::tests::scene_keys_reach_extension_in_workspace ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 890 filtered out; finished in 0.96s

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s
   Compiling wai v0.6.2 (/Users/guilherme.brito/Project/Guilherme/wai/.claude/worktrees/escritorio/crates/wai)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.46s
     Running unittests src/main.rs (target/debug/deps/wai-49bb2ae4973e8235)
```

`Run ext` - exit 0

```


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Compiling wai-ext v0.6.2 (/Users/guilherme.brito/Project/Guilherme/wai/.claude/worktrees/escritorio/crates/wai-ext)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.84s
     Running unittests src/lib.rs (target/debug/deps/wai_ext-edd963afabbf0bae)
     Running unittests src/bin/wai-fake-ext.rs (target/debug/deps/wai_fake_ext-56abe35a95218c55)
     Running unittests src/bin/wai-index-check.rs (target/debug/deps/wai_index_check-daa834179a0efd07)
     Running tests/host.rs (target/debug/deps/host-f08c4d334f349485)
     Running tests/install.rs (target/debug/deps/install-5f47a4bc1f2bc4ae)
     Running tests/manifest.rs (target/debug/deps/manifest-95bd0a91818ac09e)
   Doc-tests wai_ext
```

`grep -q '/ \scene\' docs/extensions.md && grep -q '\panel/key\' docs/extensions.md && grep -q '"1.14"' docs/ex` - exit 0

```
(no output)
```
