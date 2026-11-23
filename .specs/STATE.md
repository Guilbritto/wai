# State

## Decisions

### AD-001
- **Decision**: The phone client is a Flutter app in its own repository (`wai-mobile`) and reaches wai only through wai-remoto's HTTP + SSE API, authenticated by a per-device token.
- **Reason**: User chose a native cross-platform app in a separate repository, and SSE over WebSocket to keep wai-remoto on Go's standard library.
- **Trade-off**: The API between wai-remoto and the app is a contract across two repositories: a change needs both sides released together.
- **Scope**: wai-remoto, wai-mobile, extension API 1.10
- **Date**: 2026-11-20
- **Status**: active

## Handoff
