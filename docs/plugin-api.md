# Kotilo Deck — Widget plugin API v1

External widgets are plain folders. Drop one into `~/.kotilo/deck/widgets/` and it shows up
in the widget drawer without a rebuild or restart. This file is the single source of truth for
the plugin contract; `src-tauri/src/plugins/` (loader, file server) and `src/lib/plugins/`
(iframe host) implement it.

## Folder

```
~/.kotilo/deck/widgets/
  hello/                 folder name = plugin id, ^[a-z0-9][a-z0-9-]{0,31}$
    manifest.json        required, ≤ 64 KB
    index.html           entry (manifest "entry"), plus any css/js/images/fonts it loads
```

- In workspace files the instance's `widget` is `"ext.<folder>"` (e.g. `"ext.hello"`). Built-in ids never contain a dot, so there are no collisions.
- Folders starting with `.` or `_` are ignored. At most 64 plugins; at most 1 000 files and 20 MB per folder are served.
- If a plugin folder disappears, its instances keep their data and render the "Unknown widget" card until it comes back.

## manifest.json

Same fields as a built-in manifest (SPEC §4.3) plus `apiVersion`, `entry` and `topics`:

```json
{
  "apiVersion": 1,
  "id": "hello",
  "name": "Hello",
  "icon": "puzzle",
  "sizes": ["2x1", "2x2"],
  "defaultSize": "2x2",
  "capabilities": [],
  "multiInstance": true,
  "entry": "index.html",
  "topics": ["system"],
  "settings": {
    "type": "object",
    "properties": {
      "greeting": { "type": "string", "title": "Greeting", "default": "Hello" }
    }
  },
  "description": "Example widget",
  "author": "you",
  "version": "1.0.0"
}
```

Validation (the backend rejects the folder with a readable `error`, shown in Settings ▸ Widgets):

| Field | Rule |
|---|---|
| `apiVersion` | must be `1` |
| `id` | equals the folder name |
| `name` | 1–64 chars |
| `icon` | any string; unknown keys render the `puzzle` icon |
| `sizes` | 1–8 distinct `"WxH"`, 1 ≤ W,H ≤ 12 |
| `defaultSize` | one of `sizes` |
| `capabilities` | known `Capability` strings; default `[]` |
| `multiInstance` | boolean; default `true` |
| `entry` | relative path inside the folder, no `..`, no leading `/`, must exist; default `index.html` |
| `topics` | subset of `system`, `media`, `pomodoro`; default `[]` |
| `settings` | `{type:"object", properties}` using the generic schema form types (boolean / string+enum / number / string); default empty |

## Sandbox

- The entry renders in `<iframe sandbox="allow-scripts">` (no `allow-same-origin`): an opaque origin with no access to the deck, its storage, cookies or Tauri IPC.
- Files are served by the backend with
  `Content-Security-Policy: default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self'`.
  No `connect-src` → no network, no remote code. Plugins get data only through the messages below.
- Navigating the iframe to a remote page is blocked by the parent page's `frame-src` (`http://kotilo-widget.localhost kotilo-widget:` in `tauri.conf.json`, `'self'` in the LAN page CSP), not by the plugin CSP. That `frame-src` must never be loosened.
- Path traversal (`..`, absolute paths, symlinks leaving the folder) returns 404.
- URLs: `http://kotilo-widget.localhost/<folder>/<path>` (Windows), `kotilo-widget://localhost/<folder>/<path>` (Linux/macOS), `/plugins/<folder>/<path>` in LAN mode.

## Messages (`window.postMessage`)

Every message is `{ "kotilo": 1, "type": "...", ... }`. The deck ignores anything else and
only accepts messages whose `source` is its own iframe.

Deck → widget:

| type | fields | when |
|---|---|---|
| `init` | `size` ("2x2"), `settings` (object, defaults merged), `theme` (`{ variant: "dark"\|"light", tokens: { "bg": "#1e1e2e", … } }` — every `--k-*` value without the prefix, plus `radius`, `font-ui`, `font-mono`), `locale` ("tr-TR"), `editing` (bool), `available` (bool) | after the widget sends `ready` |
| `size` | `size` | size changed in edit mode |
| `settings` | `settings` | instance settings changed |
| `theme` | `theme` | theme / appearance changed |
| `editing` | `editing` | edit mode toggled (pause interactions while true) |
| `visibility` | `visible` (bool) | deck hidden/shown — stop timers while hidden |
| `data` | `topic`, `payload` | pushed for each subscribed topic: `system` = `SystemSample`, `media` = `MediaState`, `pomodoro` = `PomodoroState` (shapes from `src/lib/ipc/index.ts`) |

Widget → deck:

| type | fields | effect |
|---|---|---|
| `ready` | — | deck replies with `init` |
| `subscribe` | `topic` | starts `data` pushes; ignored unless the topic is in the manifest's `topics` |
| `unsubscribe` | `topic` | stops them |

## SDK

`<script src="../_sdk/kotilo-widget.js"></script>` (served by the deck, not copied into the
folder) applies the theme tokens as `--k-*` CSS variables on `:root`, sends `ready`, and
exposes:

```js
kotilo.on('init' | 'size' | 'settings' | 'theme' | 'editing' | 'visibility', fn)
kotilo.subscribe('system', (sample) => { ... })   // also unsubscribe(topic)
kotilo.size        // last known "WxH"
kotilo.settings    // last known settings
```

Plugins should use only `var(--k-*)` colours so they follow every theme, like built-in widgets.

## Versioning

`apiVersion` bumps only for breaking changes. New message types and fields are additive;
widgets must ignore unknown types and fields.
