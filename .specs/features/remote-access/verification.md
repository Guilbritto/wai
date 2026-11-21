# Remote access verification

**Verdict**: FAIL
**Proofs**: `proofs.md` at 97bcf3f
**Verifier**: independent sub-agent (author != verifier)

## Checks

| Check | Test | Holds |
| --- | --- | --- |
| C1 | `extensions_view.rs` `consent_lists_listen` | yes |
| C2 | `panel_test.go` `TestPanelShowsSettingsAndServeCommand` | yes - fields, Start, command for the new port, and each refusal's alert |
| C3 | `server_test.go` `TestStartRejectsPortOutOfRange` | yes - exact message, no listener, nothing saved, 1024/65535 accepted; the panel alert is held by C2's test |
| C4 | `server_test.go` `TestStartRejectsNonHTTPSTunnel` | yes - five bad URLs, port refused; panel alert held by C2's test |
| C5 | `server_test.go` `TestStartListensOnLoopbackOnly` | yes - status text, `[::1]` and a LAN address refused |
| C6 | `server_test.go` `TestStartReportsPortInUse` | yes - exact message; Start kept is held by C2's test |
| C7 | `server_test.go` `TestNothingListensBeforeStart` | yes |
| C8 | `panel_test.go` `TestPairShowsQRWithURLAndCode` | yes - rows equal the payload's, 5:00, 2:30, new code, expiry |
| C9 | `qr_test.go` `TestQRRowsHaveFinderPatterns` | yes |
| C10 | `pair_test.go` `TestPairIssuesTokenAndUsesCode` | yes |
| C11 | `routes_test.go` `TestPairRoute` | yes - unknown, used and expired give 401 with no token |
| C12 | `panel_test.go` `TestPanelListsPairedDevice` | yes |
| C13 | `server_test.go` `TestStopClosesListenerAndStreams` | yes - streams and an idle connection, under 1 s |
| C14 | `pair_screen_test.dart` `PairScreen shows Scan QR code on first launch` | yes |
| C15 | `pair_screen_test.dart` `PairScreen stores url and token and opens sessions` | yes |
| C16 | `pair_screen_test.dart` `PairScreen shows Pairing failed on 401` | yes |
| C17 | `workspace.rs` `remote_read_returns_transcript` | yes - exact items in order, paging, no output |
| C18 | `ext_sessions.rs` `tool_item_has_no_output` | yes |
| C19 | `workspace.rs` `remote_turn_streams_session_update` | yes - the mirrored updates rebuild the transcript, more than one update; the permission filter is held by C37 |
| C20 | `ext_sessions.rs` `feed_flushes_at_most_every_100ms` | yes |
| C21 | `workspace.rs` `remote_read_unknown_session_errors` | yes |
| C22 | `sessions_store_test.dart` `SessionsStore labels title agent and state` | yes - all four states, untitled row |
| C23 | `routes_test.go` `TestSessionsListForwardsHost` | yes |
| C24 | `sessions_store_test.dart` `SessionsStore applies session_state` | yes |
| C25 | `events_test.go` `TestEventsForwardHostEvents` | yes - all seven events, unstreamed ones skipped |
| C26 | `session_screen_test.dart` `SessionScreen shows agent output as text` | yes |
| C27 | `transcript_store_test.dart` `TranscriptStore applies session_update` | yes |
| C28 | `sessions_screen_test.dart` `SessionsScreen shows No open sessions in wai` | yes |
| C29 | `sessions_screen_test.dart` `SessionsScreen shows can't reach wai with Retry` | yes |
| C30 | `sessions_store_test.dart` `SessionsStore unpairs on 401` | no - asserts the store's status and the deleted token, not that the pairing screen shows; `app_test.dart` `App returns to pairing when wai-remoto answers 401` holds it but no check cites it |
| C31 | `workspace.rs` `remote_permission_requested_event` | yes |
| C32 | `workspace.rs` `remote_permission_answer_reaches_agent` | yes - agent reply, card's answer, resolved event |
| C33 | `workspace.rs` `remote_permission_resolved_event` | yes - wai's click; the extension's answer is held by C32's test |
| C34 | `workspace.rs` `remote_permission_answer_twice_errors` | yes - unknown, raced and stale; agent keeps reject-once |
| C35 | `workspace.rs` `remote_cancel_ends_turn_cancelled` | yes |
| C36 | `tests/host.rs` `content_and_control_methods_need_permissions` | yes |
| C37 | `tests/host.rs` `content_events_skip_read_only_extension` | yes |
| C38 | `extensions_view.rs` `consent_lists_sessions_content` | yes |
| C39 | `routes_test.go` `TestPromptForwardsToIdleSession` | yes |
| C40 | `session_screen_test.dart` `SessionScreen sends a prompt` | yes |
| C41 | `session_screen_test.dart` `SessionScreen disables send and shows Cancel while working` | yes |
| C42 | `routes_test.go` `TestCancelForwards` | yes |
| C43 | `session_screen_test.dart` `SessionScreen Cancel calls cancel` | yes |
| C44 | `session_screen_test.dart` `SessionScreen shows one button per permission option` | yes |
| C45 | `answers_test.go` `TestPermissionAnswerForwards` | yes |
| C46 | `session_screen_test.dart` `SessionScreen sends the tapped option` | yes |
| C47 | `transcript_store_test.dart` `TranscriptStore marks a permission answered in wai` | yes - including the lost race; the screen text is also held by C44's test |
| C48 | `routes_test.go` `TestPromptRateLimit429` | yes - 30 pass, 31st and 32nd 429, other device free, window slides |
| C49 | `transcript_store_test.dart` `TranscriptStore reports busy and rate limit` | yes - store message; the screen text is held by C40's test |
| C50 | `workspace.rs` `remote_question_requested_event` | yes |
| C51 | `workspace.rs` `remote_question_answer_reaches_agent` | yes |
| C52 | `workspace.rs` `remote_question_resolved_event` | yes |
| C53 | `workspace.rs` `remote_question_answer_twice_errors` | yes |
| C54 | `workspace.rs` `remote_question_answer_invalid_stays_pending` | yes |
| C55 | `ext_sessions.rs` `check_answer_rejects_values_outside_kind` | yes |
| C56 | `question_form_test.dart` `QuestionForm shows one control per field kind` | yes |
| C57 | `question_state_test.dart` `QuestionState disables Submit while a required field is empty` | yes on its cases; blank text still counts as a value (finding 2) |
| C58 | `answers_test.go` `TestQuestionAnswerForwards` | yes |
| C59 | `question_form_test.dart` `QuestionForm Submit sends values` | yes |
| C60 | `question_form_test.dart` `QuestionForm Decline and Cancel send decline and cancel` | yes |
| C61 | `transcript_store_test.dart` `TranscriptStore marks a question answered in wai` | yes |
| C62 | `panel_test.go` `TestPanelCountsConnectedDevices` | yes |
| C63 | `events_test.go` `TestRevokeClosesStreams` | yes |
| C64 | `panel_test.go` `TestPanelRevokeAndSignOutAll` | yes |
| C65 | `store_test.go` `TestTokenExpiresAfter30DaysIdle` | yes - both sides of the 30-day bound, on disk too |
| C66 | `store_test.go` `TestTokenUseRenewsExpiry` | yes |
| C67 | `unpair_test.dart` `Unpair revokes and returns to pairing` | yes |
| C68 | `routes_test.go` `TestUnpairRevokesOwnToken` | yes |
| C69 | `auth_test.go` `TestOriginHeaderIs403` | yes - empty Origin too, pair route too |
| C70 | `auth_test.go` `TestUnknownHostIs421` | yes on its cases; a tunnel URL with `:443` refuses the phone (finding 1) |
| C71 | `auth_test.go` `TestMissingOrUnknownTokenIs401` | yes |
| C72 | `auth_test.go` `TestBodyOver64KBIs413` | yes - 64 KB passes, 64 KB + 1 refused, chunked too |
| C73 | `events_test.go` `TestFifthStreamIs429` | yes |
| C74 | `auth_test.go` `TestTwentyFailuresNotifyOnce` | yes - 19 quiet, 20th notifies, none more within the hour |
| C75 | walk - Extensions, wai-remoto, Disable while started; `Test-NetConnection 127.0.0.1 -Port 7717` fails | yes (walked) |
| C76 | `routes_test.go` `TestPromptToBusySessionIs409` | yes |
| C77 | `auth_test.go` `TestResponsesAreNoStore` | yes |
| C78 | `transcript_store_test.dart` `TranscriptStore reads the state again after a reconnect` | yes - added for the reconnect finding |

## Findings

| Finding | Evidence | Fix | Closed |
| --- | --- | --- | --- |
| A tunnel URL typed with its default port (`https://host:443`) answers 421 to the phone on every request (EDGE-02) | `auth.go` `hostOK` compares `r.Host` to `url.Parse(tunnel).Host`, which keeps `:443`; the phone's HTTP client sends `Host: host` for a default port. Throwaway test at 97bcf3f: tunnel `https://my-mac.tail0123ab.ts.net:443`, Host `my-mac.tail0123ab.ts.net` -> 421 | compare against `u.Hostname()` with the port dropped when it is 443 (or reject a tunnel URL with a port at Start); add the case to `TestUnknownHostIs421` | wai `1bba492` - C70 (`TestUnknownHostIs421` now covers a tunnel at `:443`) |
| Submit is enabled for a required text field holding only spaces, which wai treats as no value (QST-07) | `question_state.dart` `_filled`: `String s => s.isNotEmpty`; `ext_sessions.rs` `check_answer` counts blank text as no value. Throwaway test: required text `'   '` -> `canSubmit` true | `_filled` uses `s.trim().isNotEmpty` (and `accept()` leaves blank text out); add a blank case to `QuestionState disables Submit while a required field is empty` | wai-mobile `6729181` - C57 (blank text case) |
| A number box holding `NaN`, `Infinity` or `1e400` enables Submit, then Submit throws before any request and locks the question on the phone | `question_state.dart` `write`: `num.tryParse` returns NaN/Infinity; `client.dart` `jsonEncode` throws `JsonUnsupportedObjectError`, which `_reach` and `TranscriptStore._answer` do not catch, so `_mine` keeps the request. Throwaway test: 0 requests sent, `canAnswer` false after, error unhandled | treat a non-finite number as no value in `write` (`n?.isFinite == true ? n : null`); add the case to the QuestionState test | wai-mobile `6729181` - C57 (`NaN`, `Infinity`, `-Infinity`, `1e400` cases) |
| After the event stream reconnects, an open session keeps its old state: a `session_state` missed while the stream was down leaves Send disabled (or enabled) wrongly (REMO-09) | `transcript_store.dart` `apply(Connected)` calls `load()`, which reads only the transcript; `state` is set only by `SessionStateEvent` and by the list row at open. No check covers a reconnect | on `Connected` also read `sessions()` and take this session's state; test: open `working`, `Connected` with the list saying `idle`, Send enabled | wai-mobile `6729181` - C78 (new check: `TranscriptStore reads the state again after a reconnect`) |
| C30 asserts the store's state, not the pairing screen the criterion names (REMO-07) | `sessions_store_test.dart` `SessionsStore unpairs on 401` checks `status` and the deleted token only; the screen is held by `app_test.dart` `App returns to pairing when wai-remoto answers 401`, which no check cites | point C30's proof at `App returns to pairing when wai-remoto answers 401` | wai `6daa5fb` - C30's proof now cites `App returns to pairing when wai-remoto answers 401` |
