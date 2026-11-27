# State

## Decisions

### AD-001
- **Decision**: The phone client is a Flutter app in its own repository (`wai-mobile`) and reaches wai only through wai-remoto's HTTP + SSE API, authenticated by a per-device token.
- **Reason**: User chose a native cross-platform app in a separate repository, and SSE over WebSocket to keep wai-remoto on Go's standard library.
- **Trade-off**: The API between wai-remoto and the app is a contract across two repositories: a change needs both sides released together.
- **Scope**: wai-remoto, wai-mobile, extension API 1.10
- **Date**: 2026-11-20
- **Status**: active

### AD-002: Extension art travels as palette + character patterns

- **Status**: active
- **Date**: 2026-11-26
- **Feature**: escritorio
- **Decision**: Colour art in the extension API (the `scene` node, API 1.14) is a palette of one-character keys to hex colours plus patterns of those characters, sent in the panel tree. wai rasterises and caches it; it does not load image files from extensions.
- **Rationale**: User picked "Paleta + padrões no JSON" over a PNG tileset and an incremental scene protocol. It extends the `pixels` node's model and keeps every render a whole tree.

### AD-003: Go extensions may take a small external module when the standard library lacks the protocol

- **Status**: active
- **Date**: 2026-11-26
- **Feature**: escritorio
- **Decision**: `wai-escritorio` depends on `github.com/coder/websocket`; `go.sum` is committed. Other Go extensions stay standard-library only unless the same reason applies.
- **Rationale**: The approved spec fixes WebSocket transport and Go's standard library has none.

## Handoff
