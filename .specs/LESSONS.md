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

## Quarantined (failed when applied - ignore)

A confirmed lesson that recurred alongside failure. Kept for the maintainer to review.

_none_
