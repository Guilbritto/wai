# Escritorio verification

**Verdict**: FAIL
**Proofs**: `proofs.md` at b3d7e4f
**Verifier**: independent sub-agent (author != verifier)

## Checks

| Check | Test | Holds |
| --- | --- | --- |
| C1 | `main_test.go` `TestNoSetupNoDial` | yes - form ids on both panels, 0 dials, hello after submit |
| C2 | `setup_test.go` `TestSetupRejectsBadName` | no - calls `validate` directly, skipping the form path where `TrimSpace` (`main.go:229`) lets "Ana\n" through; multi-byte 32/33 only in `office` |
| C3 | `main_test.go` `TestTokenRefusedShowsForm` | yes |
| C4 | `client_test.go` `TestUnreachableRetries` | no - asserts the 5 s retry only; "Cannot reach <address>" is never asserted through `onServer` (`tUnreachable` to `reasonUnreachable`) |
| C5 | `views_test.go` `TestOfficeTreeShowsEveryone` | yes |
| C6 | `main_test.go` `TestKeyMovesOneTile` | yes - all 8 keys, exact frame and sprite tile |
| C7 | `main_test.go` `TestBlockedTileStays` | yes - wall, desk, edge (`TestBlockedByMapEdge`) |
| C8 | `e2e_test.go` `TestMoveReachesOtherWithin200ms` | yes - real server, 200 ms window |
| C9 | `server/hub_test.go` `TestRejoinAtLastPosition` | yes - a blocked last position falling back to spawn is untested |
| C10 | `server/main_test.go` `TestSilentPeerLeaves` | yes with a caveat - runs with 50/150 ms; the 3 s + 7 s defaults are only checked as constants by `TestDefaultTimingsMeetSpec` |
| C11 | `ext_scene.rs` `viewport_keeps_margin` | yes - both axes, exact 3 vs 2 tiles, clamps |
| C12 | `e2e_test.go` `TestSecondLoginReplacesFirst` | yes - hub rule in `TestSameNameReplacesOlder` |
| C13 | `setup_test.go` `TestSetupSavedPrivate` | yes - 0600 on create and on overwrite of a 0644 file |
| C14 | `main_test.go` `TestChangeSetupDisconnectsAndPrefills` | yes |
| C15 | `server/main_test.go` `TestServerAcceptsOnConfiguredAddress` | yes |
| C16 | `server/config_test.go` `TestConfigRejectsShortToken` | no - 31 ASCII bytes only; the server counts bytes (`config.go:38`), a 16-char multi-byte token passes (probed) |
| C17 | `server/config_test.go` `TestConfigRequiresTLSOffLoopback` | yes - 0.0.0.0, empty host, LAN, hostname, cert without key; `[::]` probed OK |
| C18 | `server/main_test.go` `TestWrongTokenRefusedAndLoggedWithoutToken` | yes |
| C19 | `server/main_test.go` `TestRateLimitDropsPast30` | yes on 40 messages; the window reset after 1 s is not tested |
| C20 | `server/main_test.go` `TestOversizeMessageCloses` | yes - 16384 passes, 16385 closes with message too big |
| C21 | `server/hub_test.go` `TestFiftyFirstRefused` | yes - 50 admitted, 51st refused, a seat frees up; the extension's display of "The office is full" is not tested |
| C22 | `server/hub_test.go` `TestAuthenticate` | yes - values and ids; constant-time compare is not observable by a test |
| C23 | `server/hub_test.go` `TestIllegalMoveSnapsBack` | yes - wall, 2 tiles, diagonal, same tile, far, outside, desk |
| C24 | `server/main_test.go` `TestLogsConnectDisconnectRefusal` | yes - one line each, no token; full/replaced log lines untested |
| C25 | `ext_panel.rs` `scene_draws_tiles_and_sprites` | no - label and badge are only `shown()`, their text and tone are never asserted |
| C26 | `ext_scene.rs` `tween_takes_150ms` | yes at the pure-function level the Matrix names; the redraw loop that makes it visible is untested |
| C27 | `ext_panel.rs` `scene_keys_reach_extension` | no - asserts scene and key at the `PanelKey` event; the panel id is added in `ext_host.rs` and no test joins them; no test that an unfocused scene sends nothing |
| C28 | `ext_panel.rs` `scene_click_takes_focus` | yes |
| C29 | `ext_scene.rs` `viewport_centres_first_time` | yes - pure function; `draw` using it is not tested |
| C30 | `panel.rs` `scene_rules_unsupported` | no - 15 cases but at-limit and over-limit are missing for 65 patterns, 129 columns, badge 25 and 100 sprites; only `id` is tried as a missing field |
| C31 | `grep` of `docs/extensions.md` | yes - the proof only checks the strings exist |
| C32 | `views_test.go` `TestPresenceListsNearby` | yes - dx 3 diagonal vs dx 4, then 3 |
| C33 | `main_test.go` `TestEnterOfficeOpensPane` | yes |
| C34 | `views_test.go` `TestPresenceShowsSetupForm` | yes |
| C35 | `views_test.go` `TestPresenceOfflineReasons` | no - builds `view{Reason}` by hand; the `onServer` mapping of Refused(full/replaced) and unreachable to a reason is not tested at the extension level |
| C36 | `views_test.go` `TestPresenceAlone` | yes |
| C37 | `office/rules_test.go` `TestDeriveFocused` | yes |
| C38 | `office/rules_test.go` `TestDeriveNeedsYouWins` | yes |
| C39 | `office/rules_test.go` `TestDeriveNone` | yes - nil, empty, idle and done; `error` is not named |
| C40 | `main_test.go` `TestStatusCarriesNoSessionData` | yes - exact frames |
| C41 | `e2e_test.go` `TestStatusReachesOthersWithin1s` | no - one transition (to Needs you); Focused and clearing are not timed |
| C42 | `client_test.go` `TestReconnectAfterServerRestart` | yes |
| C43 | `office/map_test.go` `TestMapWithoutSpawnRefused` | no - tests `ParseMap`, not the server; the Matrix names the server layer (`TestConfigBadMapRefused` does but is not the proof) |
| C44 | `ext_scene.rs` `shared_tile_labels_stack` | no - tests only the `label_rows` index; that both avatars and both labels are drawn at distinct places is not tested |
| C45 | `office/rules_test.go` `TestNameRejectsControlChars` | no - only Cc; U+2028 and zero-width / bidi characters pass `ValidName` (probed) |

## Findings

| Finding | Evidence | Fix | Closed |
| --- | --- | --- | --- |
| Name check accepts Unicode newlines and invisible characters | probe: `ValidName("a b")`, `"​"`, `"a‮b"` all true; `office/rules.go:70` uses `unicode.IsControl` only (OFC-63 says outside printable Unicode, newlines) | reject with `!unicode.IsPrint(r)` (allow ASCII space), add U+2028/200B/202E cases to `TestNameRejectsControlChars` | 76d467d - C45 |
| Server token length counts bytes, not characters | probe: `{"token":"éé…"}` (16 chars, 32 bytes) loads with no error; `server/config.go:38` `len(c.Token)` vs OFC-21 "32 characters"; C16 tests ASCII only | use `utf8.RuneCountInString`, or `len` and reword the message to bytes; add a multi-byte case | f611460 - C16 |
| A name with a trailing or leading newline is accepted by the form | probe: save with `"Ana\n"` stores `Ana`; `main.go:229` `TrimSpace` runs before `validate`; C2 calls `validate` directly | either state trimming in the spec or validate the untrimmed name; test through `onAction("save")` | 737613c - C2 |
| C4 never asserts "Cannot reach <address>" | `client_test.go:185` checks only the retry gap; `main.go:189-193` mapping untested | in an extension test, point a saved setup at a closed port and `waitTree` for "Cannot reach "+server | 737613c - C4 |
| C35 and OFC-26: Refused full / replaced / unreachable to panel reason is untested | `views_test.go:158` builds `view.Reason` by hand; `main.go:179-188` untested for full | extension-level test against `fakeOffice` with `refuse = full` expecting "The office is full" and "Offline" | 737613c - C35 |
| C27 does not prove the wire carries the panel id, nor that an unfocused scene sends nothing | `ext_panel.rs:3170` clicks first, then types; no test types before the click; `ext_host.rs` wiring (`panel_key(&panel,..)`) has no test | add a keystroke before the click and assert empty; add a host-level test from a panel key event to `panel/key` params | bbd1842 - C27 |
| C30 and C25 proven on too few cases | `panel.rs:1308`: no 65th pattern, 129 columns, badge 25 or 100 sprites at-limit; only `id` for missing field; probe confirms the code is right today; `ext_panel.rs:3093` never asserts label or badge text | add the at-limit and over-limit pairs and a missing `cell`, `map`, `sprites`; assert label and badge text | 1c64955, bbd1842 - C30, C25 |
| Scene limits allow a 16384 x 16384 px raster | probe: 128 x 128 map, `cell` 32, `scale` 4 parses as a scene; `ext_scene.rs:59` `rasterise` allocates 16384 x 16384 RGBA (about 1 GiB) | cap `map tiles x cell x scale` (or total pixels) in `panel.rs`, test over and at the cap | 1c64955 - docs |
| C41 times one transition | `e2e_test.go:85` only "Needs you" | loop Focused, Needs you, none and assert each within 1 s | 2bc4564 - C41 |
| C43 at the wrong layer | `office/map_test.go:61` is `ParseMap`; Matrix says Server integration | point C43's proof at `TestConfigBadMapRefused`, or add a `loadConfig` no-spawn test named `TestMapWithoutSpawnRefused` | f611460 - C43 |
| C44 does not prove both avatars and labels are drawn apart | `ext_scene.rs` `shared_tile_labels_stack` tests the index list only | add a `scene_*` test with two sprites on one tile asserting two `scene-sprite` and two `scene-label` with different tops | bbd1842 - C44 |
| Saved setup is not re-validated before dialling | `main.go:268-277` loads `setup.json` and calls `connect()`; the `ws://`-to-remote refusal is only in `validate` (`setup.go:41`) | call `validate` on the loaded setup and show its message instead of dialling | 737613c - saved setup |
| Mistyped optional scene fields are silently ignored | probe: `"label": 42` and `"scale": "big"` give a scene; docs say a mistyped field is Unsupported | reject a present field of the wrong type, or reword the docs | 1c64955 - mistyped fields |
