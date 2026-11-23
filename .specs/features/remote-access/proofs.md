# remote-access proofs

**HEAD**: 7357bf0
**Tree**: clean
**Shell**: C:/Program Files/Git\bin\bash.exe
**Result**: 80 pass, 0 fail

| Check | Proof | Result |
| --- | --- | --- |
| C1 | `run wai / extensions_view::tests::consent_lists_listen ... ok` | pass |
| C2 | `run remoto / --- PASS: TestPanelShowsSettingsAndServeCommand` | pass |
| C3 | `run remoto / --- PASS: TestStartRejectsPortOutOfRange` | pass |
| C4 | `run remoto / --- PASS: TestStartRejectsNonHTTPSTunnel` | pass |
| C5 | `run remoto / --- PASS: TestStartListensOnLoopbackOnly` | pass |
| C6 | `run remoto / --- PASS: TestStartReportsPortInUse` | pass |
| C7 | `run remoto / --- PASS: TestNothingListensBeforeStart` | pass |
| C8 | `run remoto / --- PASS: TestPairShowsQRWithURLAndCode` | pass |
| C9 | `run remoto / --- PASS: TestQRRowsHaveFinderPatterns` | pass |
| C10 | `run remoto / --- PASS: TestPairIssuesTokenAndUsesCode` | pass |
| C11 | `run remoto / --- PASS: TestPairRoute` | pass |
| C12 | `run remoto / --- PASS: TestPanelListsPairedDevice` | pass |
| C13 | `run remoto / --- PASS: TestStopClosesListenerAndStreams` | pass |
| C14 | `run mobile / ✅ PairScreen shows Scan QR code on first launch` | pass |
| C15 | `run mobile / ✅ PairScreen stores url and token and opens sessions` | pass |
| C16 | `run mobile / ✅ PairScreen shows Pairing failed on 401` | pass |
| C17 | `run wai / workspace::tests::remote_read_returns_transcript ... ok` | pass |
| C18 | `run wai / ext_sessions::tests::tool_item_has_no_output ... ok` | pass |
| C19 | `run wai / workspace::tests::remote_turn_streams_session_update ... ok` | pass |
| C20 | `run wai / ext_sessions::tests::feed_flushes_at_most_every_100ms ... ok` | pass |
| C21 | `run wai / workspace::tests::remote_read_unknown_session_errors ... ok` | pass |
| C22 | `run mobile / ✅ SessionsStore labels title agent and state` | pass |
| C23 | `run remoto / --- PASS: TestSessionsListForwardsHost` | pass |
| C24 | `run mobile / ✅ SessionsStore applies session_state` | pass |
| C25 | `run remoto / --- PASS: TestEventsForwardHostEvents` | pass |
| C26 | `run mobile / ✅ SessionScreen shows agent output as text` | pass |
| C27 | `run mobile / ✅ TranscriptStore applies session_update` | pass |
| C28 | `run mobile / ✅ SessionsScreen shows No open sessions in wai` | pass |
| C29 | `run mobile / ✅ SessionsScreen shows can't reach wai with Retry` | pass |
| C30 | `run mobile / ✅ App returns to pairing when wai-remoto answers 401` | pass |
| C31 | `run wai / workspace::tests::remote_permission_requested_event ... ok` | pass |
| C32 | `run wai / workspace::tests::remote_permission_answer_reaches_agent ... ok` | pass |
| C33 | `run wai / workspace::tests::remote_permission_resolved_event ... ok` | pass |
| C34 | `run wai / workspace::tests::remote_permission_answer_twice_errors ... ok` | pass |
| C35 | `run wai / workspace::tests::remote_cancel_ends_turn_cancelled ... ok` | pass |
| C36 | `run ext / test content_and_control_methods_need_permissions ... ok` | pass |
| C37 | `run ext / test content_events_skip_read_only_extension ... ok` | pass |
| C38 | `run wai / extensions_view::tests::consent_lists_sessions_content ... ok` | pass |
| C39 | `run remoto / --- PASS: TestPromptForwardsToIdleSession` | pass |
| C40 | `run mobile / ✅ SessionScreen sends a prompt` | pass |
| C41 | `run mobile / ✅ SessionScreen disables send and shows Cancel while working` | pass |
| C42 | `run remoto / --- PASS: TestCancelForwards` | pass |
| C43 | `run mobile / ✅ SessionScreen Cancel calls cancel` | pass |
| C44 | `run mobile / ✅ SessionScreen shows one button per permission option` | pass |
| C45 | `run remoto / --- PASS: TestPermissionAnswerForwards` | pass |
| C46 | `run mobile / ✅ SessionScreen sends the tapped option` | pass |
| C47 | `run mobile / ✅ TranscriptStore marks a permission answered in wai` | pass |
| C48 | `run remoto / --- PASS: TestPromptRateLimit429` | pass |
| C49 | `run mobile / ✅ TranscriptStore reports busy and rate limit` | pass |
| C50 | `run wai / workspace::tests::remote_question_requested_event ... ok` | pass |
| C51 | `run wai / workspace::tests::remote_question_answer_reaches_agent ... ok` | pass |
| C52 | `run wai / workspace::tests::remote_question_resolved_event ... ok` | pass |
| C53 | `run wai / workspace::tests::remote_question_answer_twice_errors ... ok` | pass |
| C54 | `run wai / workspace::tests::remote_question_answer_invalid_stays_pending ... ok` | pass |
| C55 | `run wai / ext_sessions::tests::check_answer_rejects_values_outside_kind ... ok` | pass |
| C56 | `run mobile / ✅ QuestionForm shows one control per field kind` | pass |
| C57 | `run mobile / ✅ QuestionState disables Submit while a required field is empty` | pass |
| C58 | `run remoto / --- PASS: TestQuestionAnswerForwards` | pass |
| C59 | `run mobile / ✅ QuestionForm Submit sends values` | pass |
| C60 | `run mobile / ✅ QuestionForm Decline and Cancel send decline and cancel` | pass |
| C61 | `run mobile / ✅ TranscriptStore marks a question answered in wai` | pass |
| C62 | `run remoto / --- PASS: TestPanelCountsConnectedDevices` | pass |
| C63 | `run remoto / --- PASS: TestRevokeClosesStreams` | pass |
| C64 | `run remoto / --- PASS: TestPanelRevokeAndSignOutAll` | pass |
| C65 | `run remoto / --- PASS: TestTokenExpiresAfter30DaysIdle` | pass |
| C66 | `run remoto / --- PASS: TestTokenUseRenewsExpiry` | pass |
| C67 | `run mobile / ✅ Unpair revokes and returns to pairing` | pass |
| C68 | `run remoto / --- PASS: TestUnpairRevokesOwnToken` | pass |
| C69 | `run remoto / --- PASS: TestOriginHeaderIs403` | pass |
| C70 | `run remoto / --- PASS: TestUnknownHostIs421` | pass |
| C71 | `run remoto / --- PASS: TestMissingOrUnknownTokenIs401` | pass |
| C72 | `run remoto / --- PASS: TestBodyOver64KBIs413` | pass |
| C73 | `run remoto / --- PASS: TestFifthStreamIs429` | pass |
| C74 | `run remoto / --- PASS: TestTwentyFailuresNotifyOnce` | pass |
| C75 | `walk - Extensions → wai-remoto → Disable while started; Test-NetConnection 127.0.0.1 -Port 7717 fails afterwar` | pass (Walked) |
| C76 | `run remoto / --- PASS: TestPromptToBusySessionIs409` | pass |
| C77 | `run remoto / --- PASS: TestResponsesAreNoStore` | pass |
| C78 | `run mobile / ✅ TranscriptStore reads the state again after a reconnect` | pass |
| Build | `cargo fmt --all --check` | pass |
| Build | `cargo clippy --workspace --all-targets -- -D warnings` | pass |

## Output

`Run wai` - exit 0

```
test workspace::tests::remote_permission_answer_reaches_agent ... ok
test workspace::tests::remote_read_unknown_session_errors ... ok
test workspace::tests::remote_permission_answer_twice_errors ... ok
test workspace::tests::remote_read_returns_transcript ... ok
test workspace::tests::remote_question_answer_reaches_agent ... ok
test workspace::tests::remote_turn_streams_session_update ... ok
test extensions_view::tests::consent_dialog_lists_permissions ... ok
test chat::view::tests::remote_card_events_follow_clicks ... ok

test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 710 filtered out; finished in 1.14s

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.32s
   Compiling wai v0.6.2 (C:\Users\guilh\Project\wai\crates\wai)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.85s
     Running unittests src\main.rs (target\debug\deps\wai-8c9436bf221ee07c.exe)
```

`Run remoto` - exit 0

```
--- PASS: TestStartReportsPortInUse (0.00s)
=== RUN   TestNothingListensBeforeStart
--- PASS: TestNothingListensBeforeStart (0.00s)
=== RUN   TestStopClosesListenerAndStreams
--- PASS: TestStopClosesListenerAndStreams (0.01s)
=== RUN   TestStoreKeepsOnlyTokenHash
--- PASS: TestStoreKeepsOnlyTokenHash (0.00s)
=== RUN   TestTokenExpiresAfter30DaysIdle
--- PASS: TestTokenExpiresAfter30DaysIdle (0.00s)
=== RUN   TestTokenUseRenewsExpiry
--- PASS: TestTokenUseRenewsExpiry (0.00s)
=== RUN   TestRevokeOneAndAll
--- PASS: TestRevokeOneAndAll (0.00s)
PASS
ok  	wai-remoto	(cached)
```

`Run mobile` - exit 0

```
✅ C:/Users/guilh/Project/wai-mobile/test/screens/question_form_test.dart: QuestionForm shows one control per field kind
✅ C:/Users/guilh/Project/wai-mobile/test/screens/sessions_screen_test.dart: SessionsScreen lists sessions
✅ C:/Users/guilh/Project/wai-mobile/test/screens/sessions_screen_test.dart: SessionsScreen shows No open sessions in wai
✅ C:/Users/guilh/Project/wai-mobile/test/screens/unpair_test.dart: Unpair revokes and returns to pairing
✅ C:/Users/guilh/Project/wai-mobile/test/screens/sessions_screen_test.dart: SessionsScreen shows can't reach wai with Retry
✅ C:/Users/guilh/Project/wai-mobile/test/screens/session_screen_test.dart: SessionScreen sends a prompt
✅ C:/Users/guilh/Project/wai-mobile/test/screens/session_screen_test.dart: SessionScreen disables send and shows Cancel while working
✅ C:/Users/guilh/Project/wai-mobile/test/screens/question_form_test.dart: QuestionForm Submit sends values
✅ C:/Users/guilh/Project/wai-mobile/test/screens/session_screen_test.dart: SessionScreen Cancel calls cancel
✅ C:/Users/guilh/Project/wai-mobile/test/screens/session_screen_test.dart: SessionScreen shows one button per permission option
✅ C:/Users/guilh/Project/wai-mobile/test/screens/question_form_test.dart: QuestionForm Decline and Cancel send decline and cancel
✅ C:/Users/guilh/Project/wai-mobile/test/screens/session_screen_test.dart: SessionScreen sends the tapped option
::endgroup::

🎉 39 tests passed.
```

`Run ext` - exit 0

```
test parses_full_manifest ... ok
test prompt_only_command_needs_no_process ... ok
test api_version_is_checked_first ... ok
test panel_icon_and_location ... ok
test required_plugins_parse ... ok
test svg_icon_paths ... ok
test template_typos_rejected ... ok
test required_plugins_are_checked ... ok
test unknown_fields_and_permissions_rejected ... ok

test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.52s
     Running tests\host.rs (target\debug\deps\host-0f7c1a156179cff5.exe)
     Running tests\manifest.rs (target\debug\deps\manifest-fdb9ea59e5c09d5a.exe)
```
