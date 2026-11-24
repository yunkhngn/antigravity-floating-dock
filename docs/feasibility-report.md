# Phase 0 Feasibility Report — Antigravity Floating Dock

**Date:** 2026-11-25
**Target:** macOS Apple Silicon (`arm64-apple-darwin27.3.0`)

## 1. Environment & Toolchain Inspection

| Component | Status | Observed Version / Details |
| :--- | :--- | :--- |
| OS & Arch | Ready | macOS 27.3 (`Build 26C5047e`), `arm64` |
| Apple CLT / SDK | Ready | `/Library/Developer/CommandLineTools`, SDK `27.2`, Apple clang `21.0.0` |
| Node / pnpm / npm | Ready | Node `v26.13.1`, pnpm `9.12.0`, npm `11.20.0` |
| Antigravity Apps | Installed | `/Applications/Antigravity.app` (`2.35.0`, URL scheme `antigravity://`), `/Applications/Antigravity IDE.app`, `agy` CLI `1.5.15` |
| Rust (`rustc`, `cargo`, `rustup`) | **Missing (Blocker)** | Not installed (`~/.cargo/bin` absent; not installed via Homebrew). Required before Phase 1 Tauri 2 build. |
| Git repo | Not initialized | Workspace is not yet a git repository. |

## 2. Antigravity Hooks Verification

Verified against the bundled `agy-customizations` hooks reference (`docs/hooks.md`) and live Antigravity docs:

- **Config locations:** Global `~/.gemini/config/hooks.json` (currently absent on this machine) and workspace `.agents/hooks.json`.
- **Supported events:** `SessionStart`, `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, `Stop`.
- **Execution contract:** `type: "command"` only, executed synchronously via `sh -c` with JSON payload on `stdin` and JSON response on `stdout` (`{}` for `PostToolUse`, decision objects for `PreToolUse`/`Stop`, etc.).
- **Common input fields (camelCase):** `conversationId`, `workspacePaths`, `transcriptPath`, `artifactDirectoryPath`, `modelName`, `lastUserInput`, `hookEventName`, `cwd`.
- **Event-specific fields:**
  - `PreToolUse` / `PostToolUse`: `toolCall` (`id`, `name`, `args`), `stepIdx`, and optional `error` (`PostToolUse`).
  - `Stop`: `executionNum`, `terminationReason` (`NO_TOOL_CALL`, `MAX_INVOCATIONS`, `MAX_FORCED_INVOCATIONS`, `MAX_TOKEN_BUDGET_EXCEEDED`, `ERROR`, `USER_CANCELED`, `TERMINAL_STEP_TYPE`, `TERMINAL_CUSTOM_HOOK`, `INJECTED_RESPONSE`), `error`, `fullyIdle`, `continuationActive`.
- **Semantics constraint (§7.1):** `Stop` does not guarantee task success; `terminationReason` distinguishes `NO_TOOL_CALL` vs `ERROR` vs `USER_CANCELED`, while `PreToolUse` / `PreInvocation` / `PostToolUse` indicate active work. Unknown states must be represented explicitly as `unknown`.
- **Important latency constraint:** Because hooks run synchronously and block the agent loop, the hook relay binary/script must forward the event over local IPC with a short connect/write timeout and exit immediately without blocking Antigravity if the Floating Dock app is not running.

## 3. Windowing, Vibrancy & Liquid Glass (Tauri 2)

- **Tauri 2 floating windows:** Supported via `decorations: false`, `transparent: true`, `alwaysOnTop: true`, `skipTaskbar: true`, and `app.macOSPrivateApi: true` in `tauri.conf.json`.
- **Native material (`window-vibrancy` v0.8+):**
  - On macOS 26+ (including this machine on macOS 27.3), `window-vibrancy` exposes `apply_liquid_glass` with `LiquidGlassOptions` (`NSGlassEffectViewStyle::Clear` / `Regular`, `.radius(...)`, `.interactive(true)` on macOS 27+).
  - On older macOS versions or if Liquid Glass initialization fails, `apply_vibrancy` (`NSVisualEffectMaterial::HudWindow` or `Sidebar`) provides a stable native vibrancy fallback.
- **Global shortcuts:** Supported via `tauri-plugin-global-shortcut` (`2.x`). Conflicts or OS reservation failures return registration errors that can be surfaced cleanly in UI/settings.
- **Background lifecycle:** Tauri 2 supports tray/menu-bar icon + `ActivationPolicy::Accessory` (or hiding the dock icon / closing windows without exiting via `RunEvent::ExitRequested`).

## 4. Session Continuation Investigation (§9)

Checked Antigravity 2.0, `agy` CLI (`1.5.15`), `conversations` skill (`ConversationService`), and `antigravity-sdk-python`:

1. **Antigravity 2.0 (`Antigravity.app`) sessions (`~/.gemini/antigravity/`):**
   - `hooks.json` is notification/gating only; it has no inbound API to push a new user message into an existing session outside a hook turn.
   - `ConversationService` (documented in the `conversations` skill over Connect HTTP+JSON on `ANTIGRAVITY_LS_ADDRESS` with `ANTIGRAVITY_CSRF_TOKEN`) documents only `CreateConversation`, `ListConversations`, `GetConversation`, `UpdateConversation`, and `DeleteConversation`. There is **no** documented RPC to send a follow-up prompt into an existing Antigravity 2.0 session, and the address/CSRF token are ephemeral env vars injected into agent child shells rather than a stable external endpoint for standalone desktop apps.
   - `~/.gemini/antigravity/daemon/hub_*.json` and `.db` SQLite files are internal implementation artifacts; per §9, we must not reverse-engineer or mutate them.
2. **Antigravity CLI (`agy`) sessions (`~/.gemini/antigravity-cli/`):**
   - `agy --conversation <id> -p "<prompt>"` exists in `agy` `1.5.15`, scoped to the CLI store (`~/.gemini/antigravity-cli/conversations/`), separate from Antigravity 2.0's `~/.gemini/antigravity/conversations/`.
3. **Conclusion & Architecture Decision:**
   - **No verified, supported API exists to send a follow-up user message into an active Antigravity 2.0 desktop session from an external app.**
   - Per §9, the MVP will:
     - Isolate session operations behind a modular `SessionAdapter` trait in Rust.
     - Display live hook-driven session context, status, and safe read-only transcript tail (only from validated paths inside `~/.gemini/antigravity*/`) when available.
     - Keep direct same-session sending clearly marked as **unsupported for Antigravity 2.0 sessions** and provide honest fallbacks: **"Open in Antigravity"** (`open -a Antigravity` / `antigravity://`) and **"Copy message"** to clipboard.

## 5. Blockers & Next Action Required

1. **Toolchain Blocker:** Rust (`rustup`, `rustc`, `cargo`) is not installed on this Mac. Installing Rust via `rustup` (or `brew install rustup`) is required outside the repository before scaffolding and compiling Tauri 2.
2. **Repo Initialization (Optional):** Initialize `git` in the workspace if version control is desired before Phase 1.
