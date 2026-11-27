# State

## Decisions

### AD-001: Extension art travels as palette + character patterns

- **Status**: active
- **Date**: 2026-11-26
- **Feature**: escritorio
- **Decision**: Colour art in the extension API (the `scene` node, API 1.10) is a palette of one-character keys to hex colours plus patterns of those characters, sent in the panel tree. wai rasterises and caches it; it does not load image files from extensions.
- **Rationale**: User picked "Paleta + padrões no JSON" over a PNG tileset and an incremental scene protocol. It extends the `pixels` node's model and keeps every render a whole tree.

### AD-002: Go extensions may take a small external module when the standard library lacks the protocol

- **Status**: active
- **Date**: 2026-11-26
- **Feature**: escritorio
- **Decision**: `wai-escritorio` depends on `github.com/coder/websocket`; `go.sum` is committed. Other Go extensions stay standard-library only unless the same reason applies.
- **Rationale**: The approved spec fixes WebSocket transport and Go's standard library has none.

## Handoff

(none)
