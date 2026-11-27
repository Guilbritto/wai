# LESSONS - auto-maintained by scripts/lessons.py

> Machine-owned. Do NOT hand-edit. Changes are overwritten on the next `lessons.py` write.
> Canonical state lives in `.specs/lessons.json`. Edit lessons only via the script.
> promote_threshold=2 distinct features · window_days=45 · quarantine_threshold=2

## Confirmed (load these at Plan/Checks)

Corroborated across multiple features. Safe to apply as guidance.

_none_

## Candidates (under observation - do NOT load as guidance yet)

Seen once or not yet corroborated. Tracked, not trusted.

### L-001 - When matching a Host header against a configured URL, drop the scheme's default port on both sides.
- signal: `spec_precision_gap` · recurrence: 1 feature(s) · scope: `extensions/wai-remoto` · harmful: 0
- features: remote-access
- evidence: EDGE-02 (extensions/wai-remoto)
- last seen: 2026-11-21T15:17:58Z

### L-002 - A client-side 'has a value' rule must use the same definition as the server's validator (blank text, non-finite numbers).
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `wai-mobile` · harmful: 0
- features: remote-access
- evidence: QST-07 (wai-mobile)
- last seen: 2026-11-21T15:17:58Z

### L-003 - On a stream reconnect, re-read every piece of state the stream keeps in step, not only the content.
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `wai-mobile` · harmful: 0
- features: remote-access
- evidence: REMO-09 (wai-mobile)
- last seen: 2026-11-21T15:17:58Z

### L-004 - A criterion that names a screen needs a widget/app test that checks the screen, not the store behind it.
- signal: `weak_test` · recurrence: 1 feature(s) · scope: `wai-mobile` · harmful: 0
- features: remote-access
- evidence: C30 (wai-mobile)
- last seen: 2026-11-21T15:17:58Z

### L-005 - Validate display names with unicode.IsPrint, not unicode.IsControl, so line separators and invisible or bidi characters are refused
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `go-extensions` · harmful: 0
- features: escritorio
- evidence: office/rules.go:70 (OFC-63) (go-extensions)
- last seen: 2026-11-27T02:08:33Z

### L-006 - Count length limits in characters, not bytes, and test a multi-byte value
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `go-extensions` · harmful: 0
- features: escritorio
- evidence: server/config.go:38 (C16) (go-extensions)
- last seen: 2026-11-27T02:08:33Z

### L-007 - Validate user input before normalising it, or state the normalisation in the spec, and test through the real form action
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `go-extensions` · harmful: 0
- features: escritorio
- evidence: main.go:229 (C2) (go-extensions)
- last seen: 2026-11-27T02:08:33Z

### L-008 - Assert the exact user-facing message a criterion names, driven through the real loop, not a hand-built view
- signal: `weak_test` · recurrence: 1 feature(s) · harmful: 0
- features: escritorio
- evidence: client_test.go:185 (C4, C35)
- last seen: 2026-11-27T02:08:33Z

### L-009 - Prove a focus-gated input rule with the negative case before focus and through the host wiring to the wire params
- signal: `weak_test` · recurrence: 1 feature(s) · scope: `panels` · harmful: 0
- features: escritorio
- evidence: ext_panel.rs:3170 (C27) (panels)
- last seen: 2026-11-27T02:08:33Z

### L-010 - Prove every bounded field with an at-limit and an over-limit pair, and every required field missing
- signal: `weak_test` · recurrence: 1 feature(s) · scope: `panels` · harmful: 0
- features: escritorio
- evidence: panel.rs:1308 (C30, C25) (panels)
- last seen: 2026-11-27T02:08:33Z

### L-011 - Cap the product of size parameters that decides an allocation, not each parameter alone
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `panels` · harmful: 0
- features: escritorio
- evidence: ext_scene.rs:59 raster (panels)
- last seen: 2026-11-27T02:08:33Z

### L-012 - Time every state transition a latency criterion covers, not one of them
- signal: `weak_test` · recurrence: 1 feature(s) · harmful: 0
- features: escritorio
- evidence: e2e_test.go:85 (C41)
- last seen: 2026-11-27T02:08:33Z

### L-013 - Point each check's proof at a test in the layer the test matrix names for it
- signal: `weak_test` · recurrence: 1 feature(s) · harmful: 0
- features: escritorio
- evidence: office/map_test.go:61 (C43)
- last seen: 2026-11-27T02:08:33Z

### L-014 - Prove a drawing rule on the rendered elements, not on the helper that computes their positions
- signal: `weak_test` · recurrence: 1 feature(s) · scope: `panels` · harmful: 0
- features: escritorio
- evidence: ext_scene.rs shared_tile_labels_stack (C44) (panels)
- last seen: 2026-11-27T02:08:33Z

### L-015 - Re-validate persisted settings on load with the same rules as the form before acting on them
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `go-extensions` · harmful: 0
- features: escritorio
- evidence: main.go:268-277 saved setup (go-extensions)
- last seen: 2026-11-27T02:08:33Z

### L-016 - Reject an optional field present with the wrong type when the docs promise it, and treat null as absent
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `panels` · harmful: 0
- features: escritorio
- evidence: panel.rs scene optional fields (panels)
- last seen: 2026-11-27T02:08:33Z

## Quarantined (failed when applied - ignore)

A confirmed lesson that recurred alongside failure. Kept for the maintainer to review.

_none_
