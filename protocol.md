# Kotilo companion protocol — v1

Local WebSocket protocol between **Kotilo Deck** (server) and the **Kotilo desktop app** (client).
This file is the single source of truth and is copied verbatim into the desktop repo. If code on either
side disagrees with it, the code is wrong.

## 1. Transport

- Deck listens on `ws://127.0.0.1:47831` (loopback only; never `0.0.0.0`). Path `/`.
- One desktop client at a time. A second authenticated client replaces the first (the old socket is closed
  with code `4409 replaced`).
- Text frames only, UTF-8 JSON, one message per frame, max 1 MiB per frame.
- Keep-alive: Deck sends a WebSocket ping every 15 s; a client that misses 3 pongs is dropped.
- Deck must be fully usable with no client connected; the desktop must be fully usable with no Deck.

## 2. Token & handshake

- Shared secret in `~/.kotilo/companion.token`: 64 lowercase hex chars (32 random bytes), no newline required
  (readers trim whitespace). Created by **whichever side starts first** if missing (write to a temp file in
  the same folder, then rename; never overwrite an existing valid token). The file must be readable only by
  the current user (POSIX `0600`; on Windows the default ACL of the user profile is sufficient).
- The client's **first frame** must be `hello` within 5 s, otherwise the server closes with `4401`.

```json
{ "v": 1, "type": "hello", "payload": { "token": "<64 hex>", "client": "kotilo-desktop", "clientVersion": "1.4.0" } }
```

Server replies:

```json
{ "v": 1, "type": "hello.ok", "payload": { "server": "kotilo-deck", "serverVersion": "0.0.1", "protocol": 1 } }
```

Close codes: `4401` bad/missing token or no hello in time · `4400` malformed first frame · `4409` replaced by
a newer client · `4426` unsupported `v` (the server speaks only `v: 1`) · `1001` Deck shutting down.

Clients reconnect with exponential backoff 1 s, 2 s, 4 s, … capped at 30 s, reset after a successful `hello.ok`.
On `4401` the client re-reads the token file before retrying.

## 3. Envelope

```json
{ "v": 1, "type": "<dotted.name>", "payload": { } }
```

- `v` is the protocol version (integer). A breaking change bumps it; additive fields do not.
- Unknown `type`: log at debug level and ignore. Unknown payload fields: ignore. Missing optional fields: use
  the documented default. A message that fails validation is logged and ignored — never fatal.
- All timestamps are Unix epoch **milliseconds** (integers). Durations are seconds unless named `…Ms`.

## 4. After the handshake

1. Deck sends its current `pomodoro.state`.
2. Desktop sends `agents.state` and `usage.today` (current snapshots), then deltas as things change.

Snapshots are authoritative: `agents.state` always carries the full list, so a missed message heals on the next one.

## 5. Desktop → Deck

### `agents.state`
Full snapshot of known agents and pending approvals.

```json
{ "v": 1, "type": "agents.state", "payload": {
  "agents": [
    { "id": "a1", "repo": "kotilo-deck", "name": "claude", "status": "working", "task": "Phase 3 widgets", "since": 1793733000000 },
    { "id": "a2", "repo": "shell", "name": "claude", "status": "waiting", "task": "Run cargo build?", "since": 1793733100000 }
  ],
  "approvals": [
    { "id": "ap-17", "agentId": "a2", "summary": "Run cargo build", "detail": "cargo build --release", "requestedAt": 1793733100000 }
  ]
} }
```

- `status`: `"idle" | "working" | "waiting"` (waiting = needs the user). Unknown values are shown as `idle`.
- `task` (optional string), `since` (optional, when the status started).
- `approvals[]` lists every approval still pending; anything absent is no longer pending.

### `agent.approval.requested`
A new approval, sent in addition to the next `agents.state` so the Deck can react immediately (attention
border, optional OS notification). Payload = one approval object as in `approvals[]`.

### `agent.approval.resolved`
An approval was answered (on the desktop, on the Deck, or it expired).

```json
{ "v": 1, "type": "agent.approval.resolved", "payload": { "id": "ap-17", "decision": "approve", "by": "deck" } }
```

`decision`: `"approve" | "deny" | "expired"`; `by`: `"deck" | "desktop" | "agent"`.

### `usage.today`

```json
{ "v": 1, "type": "usage.today", "payload": { "tokens": 1840000, "costUsd": 4.12, "budgetUsd": 10 } }
```

`budgetUsd` optional (no budget → omitted or `null`).

### `pomodoro.suggest`
The desktop suggests a focus block (e.g. an agent started a long task).

```json
{ "v": 1, "type": "pomodoro.suggest", "payload": { "reason": "Agent started a long task", "minutes": 25 } }
```

`minutes` optional (Deck default focus length). The Deck shows a dismissible banner for 60 s; it never
starts a timer on its own.

### `workspace.switch`

```json
{ "v": 1, "type": "workspace.switch", "payload": { "slug": "agents" } }
```

Honoured only if the user enabled "Allow desktop to switch workspace" in Deck settings; otherwise ignored.
Unknown slug → ignored.

## 6. Deck → Desktop

### `approval.respond`

```json
{ "v": 1, "type": "approval.respond", "payload": { "id": "ap-17", "decision": "approve" } }
```

`decision`: `"approve" | "deny"`. The desktop applies it and answers with `agent.approval.resolved`
(`by: "deck"`). If the approval is no longer pending, the desktop sends `agent.approval.resolved` with the
real outcome so the Deck can correct its UI.

### `pomodoro.state`
Sent on connect and on every change (start, pause, resume, skip, phase end, settings change). Not sent per tick:
the desktop computes the countdown from `endsAt`.

```json
{ "v": 1, "type": "pomodoro.state", "payload": {
  "phase": "focus", "running": true, "endsAt": 1793734500000, "remaining": 1500,
  "duration": 1500, "session": 2, "sessionsPerCycle": 4
} }
```

- `phase`: `"idle" | "focus" | "short-break" | "long-break"`.
- `running`: false when paused or idle. `endsAt`: present only while running (else `null`).
- `remaining`: seconds left (authoritative while paused). `duration`: full length of the phase in seconds.
- `session`: 1-based index of the current/next focus session in the cycle.

A desktop may mute its notifications while `phase == "focus" && running`.

### `todo.added`
Sent when the user adds a todo on the Deck (optional for the desktop to act on, e.g. mirror it into Kotilo tasks).

```json
{ "v": 1, "type": "todo.added", "payload": { "id": 42, "text": "deploy", "due": 1793770000000 } }
```

`due` optional / `null`.

## 7. Versioning & extension

- New message types and new optional fields are additive within `v: 1`.
- Renaming/removing a field or changing its meaning requires `v: 2`; a server supporting both versions
  answers in the client's version.
- To add a type: document it here first (direction, payload, example), then add the serde type + a codec
  round-trip test on the Deck, then extend the mock-desktop integration test.

## 8. Reference client

`scripts/mock-desktop.mjs` (Node ≥ 22, no dependencies) connects, authenticates, sends sample agents/usage,
prints every Deck message and can request an approval from the command line. It doubles as the manual
acceptance test for Phase 3.
