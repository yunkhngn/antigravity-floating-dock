# Antigravity Floating Dock — Implementation Instructions

## 1. Mission

Build **Antigravity Floating Dock**, a lightweight, standalone macOS desktop app that provides a small always-available interface for monitoring Antigravity (AGY) agent activity and opening a compact chat overlay.

The app must let the user keep working in other applications without bringing the full Antigravity window to the foreground. Antigravity itself must remain running in the background for its agents and sessions to continue executing.

This is a companion UI and integration layer, **not a replacement for Antigravity** and not a new autonomous coding agent.

## 2. Product goals

1. Run as a standalone macOS `.app`.
2. Stay lightweight while idle; avoid a large dashboard or unnecessary background services.
3. Show a compact progress bubble when AGY reports meaningful activity.
4. Open a floating chat panel through a global keyboard shortcut.
5. Allow the user to hide the UI without stopping the companion.
6. Receive AGY lifecycle events through supported Antigravity hooks.
7. Preserve task/session context where the supported AGY integration permits it.
8. Build locally for Apple Silicon (M-series Macs).
9. Support unsigned personal-use builds initially; code signing and notarization are out of scope for the first release.

## 3. Required technology

- **Desktop shell:** Tauri 2
- **Backend:** Rust
- **Frontend:** React + TypeScript + Vite
- **Styling:** CSS, with restrained transitions and translucent surfaces
- **Native macOS window effects:** `window-vibrancy` where compatible with the target macOS version
- **Global shortcut:** Tauri global-shortcut plugin or another maintained Tauri-compatible implementation
- **Local communication:** Prefer a local Unix domain socket or another narrowly scoped local IPC mechanism
- **Serialization:** Serde / JSON
- **Async runtime:** Tokio only where asynchronous I/O or process coordination requires it

Do not introduce a web server, cloud backend, account system, analytics, or database for the MVP.

## 4. Before implementation

1. Inspect the existing repository, project conventions, package manager, Rust toolchain, and installed Tauri version before changing files.
2. Reuse existing conventions and dependencies when reasonable.
3. Do not overwrite unrelated work.
4. Confirm the supported macOS deployment target and available SDK/toolchain.
5. Verify the current Antigravity Hooks documentation and actual hook payloads before implementing the integration.
6. Do not invent Antigravity commands, hook fields, APIs, session identifiers, or undocumented endpoints.
7. If a required AGY capability cannot be verified, isolate it behind an interface and document the limitation rather than faking success.

## 5. UX and window behavior

The app has two primary UI surfaces. They may be implemented as separate native windows or as one window with carefully managed compact/expanded states, depending on what is most reliable in Tauri on macOS.

### 5.1 Progress Bubble

A small, unobtrusive floating pill, preferably positioned near the top-right of the active display.

States:

- **Hidden / idle:** no bubble visible unless the user chooses to pin it.
- **Working:** show a subtle activity indicator and short status text.
- **Progress update:** show concise event text, such as “Editing files” or “Running tests”, only when this information is actually supplied by AGY.
- **Completed:** show a success state and a short completion notification.
- **Needs attention / error:** show a clear warning state without pretending the task succeeded.

Behavior:

- Keep the bubble above normal application windows when visible.
- Do not steal keyboard focus for routine progress updates.
- Avoid repeatedly showing identical events.
- Allow the user to click the bubble to open the chat panel.
- Allow the user to dismiss or hide it.
- Use subtle transitions; do not animate continuously when idle.
- Do not fabricate percentages. Show a determinate progress bar only if AGY provides a real numeric progress value.

### 5.2 Floating Chat

A compact translucent panel that can be shown or hidden independently of the Antigravity main window.

Required elements:

- Header with product name and current connection/session state.
- Conversation area for the selected AGY task/session, when available.
- Composer with send action.
- Minimize/hide and close-to-hide controls.
- Optional expand/collapse affordance.
- Clear indication when the companion is disconnected from AGY or cannot continue the current session.

Behavior:

- Open using the configured global shortcut.
- Close/hide using `Esc`.
- Do not quit the background companion when the panel is hidden.
- Preserve the current visible conversation during ordinary hide/show operations.
- Do not claim that a message was sent to AGY until the integration confirms it.
- If sending messages into the existing AGY session is not supported by a verified interface, show the limitation and provide a safe fallback (for example, open/focus Antigravity or copy the composed message). Do not silently create a separate AI conversation.

### 5.3 Visual direction

Aim for a native macOS Siri-like floating surface:

- Compact, rounded geometry.
- Translucent native material / vibrancy.
- Soft ambient shadow.
- Very subtle border and highlight.
- Restrained blue/purple ambient gradients if useful.
- Smooth opacity and scale transitions.
- High contrast and readable text in both light and dark environments.

Use native macOS vibrancy for the actual background material where supported. CSS `backdrop-filter` alone is not an acceptable substitute for native window vibrancy.

For macOS 26 and later, investigate whether the selected crate exposes a compatible Liquid Glass API. Treat newer Liquid Glass APIs as progressive enhancement, not as a hard requirement. Provide a stable vibrancy fallback for supported older macOS versions. Verify crate API signatures and OS availability at implementation time; do not copy unverified example APIs.

## 6. Keyboard shortcuts

Provide configurable shortcuts with sensible defaults:

- `Option + Space`: toggle Floating Chat.
- `Escape`: hide the currently focused overlay.
- `Command + Enter`: send the composed message when the composer is focused.
- `Option + Shift + Space`: toggle/pin the Progress Bubble (optional if this conflicts with system or user shortcuts).

Requirements:

- Register shortcuts through a supported global-shortcut mechanism.
- Handle shortcut registration failures gracefully.
- Do not silently steal a shortcut that is already unavailable.
- Provide a way to change shortcuts later; a minimal settings view is sufficient.
- Ensure shortcuts work while another application is frontmost.

## 7. Antigravity integration

### 7.1 Hook-based event intake

Use Antigravity's documented Hooks mechanism to notify the companion about agent lifecycle events.

Investigate the available events and payloads. A stop/idle-related hook may be useful for detecting when an agent turn ends, but do not assume that every stop means successful task completion.

The integration should distinguish, where the verified payload allows:

- Agent/task started
- Agent is working or emitted a progress update
- Agent turn stopped / became idle
- Successful completion (only if explicitly knowable)
- Error or failed execution (only if explicitly knowable)
- User input or approval required (only if explicitly knowable)

If the hook does not provide a specific state, represent it as **unknown** rather than guessing.

### 7.2 Hook installer / setup

Provide a setup flow or documented setup command that:

1. Explains which AGY hook configuration is needed.
2. Backs up any existing hook configuration before modifying it.
3. Adds only the minimum required hook entry.
4. Preserves existing user hooks and settings.
5. Can verify whether the hook is installed.
6. Can remove only the companion's own hook entry during uninstall.

Never replace the user's entire AGY configuration file.

If safe automatic installation is not feasible, provide a copyable configuration snippet and clear manual instructions.

### 7.3 Event payload

Normalize verified AGY hook data into an internal event model, for example:

- event type
- timestamp
- task/session identifier, if supplied
- conversation identifier, if supplied
- status/reason, if supplied
- summary/message, if supplied
- transcript path, if supplied
- raw payload reference for diagnostics, with secrets redacted

This is an internal model, not a claim that AGY provides all these fields. Populate only fields actually present in verified hook payloads.

Do not read arbitrary files or transcript paths without validating that they are accessible and relevant. Avoid sending transcript contents to logs.

## 8. Local IPC and process model

Suggested architecture:

```text
Antigravity (must remain running)
    |
    | documented Hooks
    v
Small hook relay / script
    |
    | local IPC
    v
Antigravity Floating Dock (Tauri 2)
    |
    +-- Rust event manager
    +-- Connection/session state
    +-- Window manager
    +-- Global shortcut handler
    |
    v
React UI
    +-- Progress Bubble
    +-- Floating Chat
```

Requirements:

- Bind IPC only to local machine resources.
- Prefer a Unix domain socket with restrictive filesystem permissions.
- Validate message size and structure.
- Do not expose a TCP listener on `0.0.0.0`.
- Authenticate local messages where practical (for example, a per-run random token or protected socket permissions).
- Handle stale sockets and companion restarts.
- Queue only a small bounded number of recent events; avoid unbounded memory growth.
- Deduplicate repeated events.
- Redact credentials, tokens, and sensitive environment values from logs.
- Do not execute arbitrary shell commands received over IPC.

The hook relay should be short-lived and reliable. It should send the event and exit rather than keeping a persistent process alive unless a verified AGY integration requires otherwise.

## 9. Session continuation: critical investigation

A core product requirement is to continue the **same Antigravity conversation/session** from the floating chat.

Do not assume that Antigravity Hooks provide a supported way to send a new user message into an existing session. Hooks may provide notifications and context paths without providing a bidirectional conversation API.

Before implementing chat sending:

1. Search official/current Antigravity documentation for a supported session continuation API, CLI command, or documented integration point.
2. Verify the interface with a minimal test.
3. Determine how the active session is selected and how replies/errors are returned.
4. Confirm whether sending a message can target the exact session that generated the event.
5. Document limitations and version dependencies.

Do not:
- Reverse-engineer private endpoints as if they were stable APIs.
- Write directly into undocumented internal databases.
- Modify Antigravity internal state files.
- Claim same-session chat works if the implementation only opens a new session.
- Treat a transcript file as a writable chat interface.

If no supported continuation interface exists, ship the event bubble and read-only task summary first. Keep the chat composer disabled or offer an explicit fallback such as “Open in Antigravity” / “Copy message”. Keep the session adapter modular so a supported API can be added later.

## 10. Background lifecycle

- The companion should support running without a visible main window.
- Closing/hiding the floating UI must not terminate the companion.
- Provide an explicit Quit action from the menu bar or app menu.
- Optionally support launch at login, but make it opt-in.
- Do not attempt to keep Antigravity alive if the user quits it.
- Clearly show when Antigravity is not running or the hook bridge is disconnected.
- Recover gracefully if Antigravity or the companion restarts.
- Avoid polling at high frequency. Prefer event-driven updates.

## 11. Security and privacy

- All MVP data stays local.
- No telemetry or analytics.
- No network listener.
- No automatic upload of transcripts or source code.
- Redact secrets from diagnostic logs.
- Validate all paths received from hook payloads before reading.
- Do not execute commands or scripts supplied by event text.
- Do not alter AGY configuration without user consent and a backup.
- Explain any permissions required by macOS.
- Keep the app's filesystem access as narrow as practical.

## 12. Error handling

Handle at least these cases:

- Antigravity is not running.
- Hook is not installed.
- Hook payload is malformed or from an unsupported version.
- IPC socket is unavailable or stale.
- Companion starts after an event was emitted.
- Global shortcut cannot be registered.
- Native vibrancy is unavailable on the current OS.
- The selected session cannot be identified.
- Same-session message sending is unsupported.
- Transcript path is missing or inaccessible.
- The agent stopped, but success/failure cannot be determined.

Use concise user-facing error messages. Keep technical details in local diagnostic logs, with sensitive data redacted.

## 13. Build and distribution

Primary target:

- macOS
- Apple Silicon: `aarch64-apple-darwin`
- Standalone `Antigravity Floating Dock.app`
- Optional `.dmg` installer

Expected Tauri build output is under the Tauri bundle output directory; verify actual paths from the repository's build configuration rather than hardcoding assumptions.

Initial distribution:

- Unsigned local build for personal use.
- Do not purchase or require an Apple Developer Program membership for MVP development.
- Document that Gatekeeper may warn or block an unsigned app downloaded from another source.
- Do not recommend disabling Gatekeeper globally.
- Code signing, Developer ID, and notarization are future distribution tasks.

The build still requires the macOS build toolchain and SDK appropriate for Tauri. Do not claim that native macOS compilation requires no Apple developer tools at all.

## 14. Suggested implementation phases

### Phase 0 — Feasibility spike

- Inspect toolchain and repository.
- Verify current AGY hook documentation and sample payload.
- Verify Tauri window behavior for transparent, always-on-top floating windows.
- Verify `window-vibrancy` compatibility and available material styles.
- Verify global shortcut registration.
- Investigate whether same-session message sending has a supported interface.
- Write a short feasibility report with confirmed capabilities and blockers.

### Phase 1 — App shell

- Create Tauri 2 + React + TypeScript app structure.
- Add tray/menu-bar lifecycle and Quit action.
- Implement a compact always-on-top transparent window.
- Add global shortcut toggle.
- Add native vibrancy with fallback.
- Build and run a local unsigned `.app`.

### Phase 2 — Progress Bubble

- Implement event model.
- Implement local IPC receiver.
- Implement hook relay.
- Add bubble states and event deduplication.
- Add connection/error states.
- Test with fixture events before connecting to AGY.

### Phase 3 — Antigravity hook integration

- Add safe hook setup/verification.
- Preserve existing hook configuration.
- Test real lifecycle events.
- Confirm stop/idle versus successful completion semantics.
- Test app restarts and missing companion cases.

### Phase 4 — Floating Chat

- Implement conversation UI.
- Implement session adapter only against a verified supported interface.
- If no supported interface exists, provide explicit open/copy fallback and document the limitation.
- Add send/loading/error states only when backed by real integration responses.

### Phase 5 — Polish and packaging

- Tune Liquid Glass/vibrancy, spacing, typography, and animation.
- Test light/dark appearance and multiple displays.
- Test shortcut conflicts.
- Check idle CPU and memory usage.
- Build Apple Silicon `.app` and `.dmg`.
- Write install, permissions, hook setup, troubleshooting, and uninstall instructions.

## 15. Acceptance criteria

The MVP is acceptable when:

1. It builds as a standalone macOS app for Apple Silicon.
2. It can run with no visible main window.
3. The floating chat can be toggled globally while another app is active.
4. Hiding the overlay does not quit the companion.
5. A verified AGY hook event reaches the companion over local IPC.
6. The progress bubble displays real event information and does not invent progress.
7. Completion is not inferred solely from a generic stop event.
8. The UI clearly indicates when AGY is disconnected.
9. Existing AGY hook configuration is preserved during setup.
10. IPC is local-only and does not expose a LAN-accessible service.
11. Logs do not contain secrets or full source/transcript contents.
12. The app has a stable vibrancy fallback if Liquid Glass is unavailable.
13. The app can be built unsigned for personal local use.
14. Same-session chat is either verified to work through a supported interface or clearly marked as unavailable with an honest fallback.

## 16. Non-goals for MVP

- Replacing Antigravity's agent runtime.
- Running agents after Antigravity has been quit.
- Building a general-purpose AI chatbot with a separate model provider.
- Supporting multiple coding-agent products.
- Cloud sync, user accounts, or remote access.
- A full dashboard, project manager, or analytics panel.
- Inventing progress percentages or task summaries not supplied by AGY.
- Private API reverse engineering or direct mutation of Antigravity internals.

## 17. Required deliverables

1. Working Tauri 2 + Rust + React/TypeScript project.
2. Progress Bubble and Floating Chat UI.
3. Local IPC event receiver and tested hook relay.
4. Safe AGY hook setup and verification instructions.
5. Feasibility report covering session continuation support.
6. Build instructions for Apple Silicon.
7. User-facing setup, troubleshooting, and uninstall documentation.
8. A clear list of implemented capabilities versus deferred capabilities.

## 18. Final instruction to the coding agent

Prioritize a small, reliable, native-feeling macOS companion over feature count. Verify every Antigravity integration point against current documentation or a real test. Never fake a successful session continuation. If a capability is blocked by the available AGY interface, implement the rest of the product cleanly, expose the limitation in the UI, and document the blocker.
