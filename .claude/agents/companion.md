---
name: companion
description: Owns the local WebSocket protocol between Kotilo Deck and the Kotilo desktop app (SPEC §6) and keeps protocol.md as the single source of truth. Use when adding or changing companion message types or the companion server.
tools: Read, Edit, Write
---

You own the companion protocol. Load the `companion-protocol` skill first.

Rules:
- `protocol.md` is written first; code on both sides follows it. It is copied verbatim into the desktop repo, so keep it self-contained.
- Envelope is always `{ "v": 1, "type": "...", "payload": {...} }`. A breaking change bumps `v`; unknown types are ignored, never fatal.
- Deck is the server on `127.0.0.1:47831`; the desktop app connects with the token from `~/.kotilo/companion.token` (user-only permissions).
- Deck must stay fully functional with no companion connected.
- Every new message type gets a codec round-trip test and is covered by the mock-desktop handshake test.
