---
name: tauri-platform-layer
description: Conventions for the Kotilo Deck platform layer - the Platform trait, platform::windows / platform::linux modules, and how to stub a platform not yet implemented. Use before editing src-tauri/src/platform/ or adding any OS-specific feature.
---

# Platform layer

Lives in `src-tauri/src/platform/`. Shared design with Kotilo Shell.

```
platform/
  mod.rs      Platform trait, Capability enum, PlatformError, current()
  windows.rs  #[cfg(target_os = "windows")]  WindowsPlatform
  linux.rs    #[cfg(target_os = "linux")]    LinuxPlatform
  stub.rs     everything else (macOS until implemented)
```

## Rules
1. Only these modules may import `windows`, D-Bus crates, or spawn `pactl`/`wpctl`.
2. Callers use `platform::current()` and check `has(Capability::X)` or handle `PlatformError::Unsupported`.
3. Every trait method has a default body returning `Err(PlatformError::Unsupported(cap))`, so a new OS only needs `capabilities()` to compile.
4. `capabilities()` lists only what is actually implemented; the frontend reads it via the `platform_capabilities` command to show "Not available on this OS".
5. Never block the async runtime: COM/D-Bus calls run on a dedicated thread or `spawn_blocking`.
6. Windows COM: initialise once per thread (`CoInitializeEx` with `COINIT_MULTITHREADED`) inside the platform module.

## Adding a capability
1. Add a variant to `Capability` (serialised kebab-case; mirror it in widget manifests).
2. Add a trait method with an `Unsupported` default.
3. Implement it in `windows.rs` (and `linux.rs` if in scope), add the variant to that platform's `capabilities()`.
4. Add a unit test for the default path and, where possible, an `#[ignore]` integration test for the real OS call.
