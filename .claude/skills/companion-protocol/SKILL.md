---
name: companion-protocol
description: Kotilo Deck <-> Kotilo desktop WebSocket protocol - message schema, versioning, reconnect rules, and how to add a message type on both sides. Use when working on the companion server, Agent Status widget, or pomodoro/todo companion hooks.
---

# Companion protocol

`protocol.md` (repo root, created in Phase 3) is the single source of truth and is copied verbatim into the desktop repo. This skill summarises it; if they disagree, `protocol.md` wins.

## Transport
- Deck is the **server** on `ws://127.0.0.1:47831`; the desktop app connects.
- Auth: token in `~/.kotilo/companion.token`, created by whichever side starts first, user-only readable. Sent as the first message.
- Client reconnects with exponential backoff (1 s, 2 s, 4 s ... capped at 30 s).

## Envelope
```json
{ "v": 1, "type": "agents.state", "payload": { } }
```
Unknown `type` is logged and ignored. A breaking change bumps `v`.

## Messages (v1)
Desktop to Deck: `agents.state`, `agent.approval.requested`, `usage.today`, `pomodoro.suggest`, `workspace.switch` (only if the user enabled it).
Deck to Desktop: `approval.respond`, `pomodoro.state`, `todo.added`.

## Adding a message type
1. Write it in `protocol.md` first: direction, payload schema, example.
2. Add the Rust serde type + codec test (round trip).
3. Handle it in the Deck; ignore gracefully when the companion is off.
4. Extend the mock-desktop integration test.
5. Note it for the desktop repo (copy `protocol.md`).
