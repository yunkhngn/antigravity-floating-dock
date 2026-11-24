# Antigravity Floating Dock

A lightweight, standalone macOS companion app (Tauri 2 + Rust + React/TypeScript) that monitors Antigravity (AGY) agent lifecycle events through documented hooks and exposes a compact always-on-top **Progress Bubble** and **Floating Chat** overlay.

## Implemented vs Deferred Capabilities

### Implemented (MVP)
- **Standalone macOS `.app` & `.dmg` (`aarch64-apple-darwin`)** running as a menu-bar accessory (`ActivationPolicy::Accessory`) so hiding or closing floating windows never stops the companion.
- **Native macOS material:** Uses `window-vibrancy` (`apply_liquid_glass` via `NSGlassEffectView` on macOS 26+ with interactive mode on macOS 27+, falling back to `apply_vibrancy` `NSVisualEffectMaterial::HudWindow`, then CSS backdrop blur).
- **Progress Bubble (`bubble` window):** Top-right translucent pill that displays hook-driven activity (`Working`, `Idle`, `Needs attention`, `Error`, `Unknown`) without stealing keyboard focus or fabricating percentages.
- **Floating Chat (`chat` window):** Global shortcut toggle (`Option + Space` default), `Esc` to hide, expandable size, active session metadata, bounded recent events list, and safe read-only JSONL transcript tail (strictly restricted to `~/.gemini/antigravity*` directories).
- **Local-only authenticated IPC:** Unix domain socket (`~/Library/Application Support/com.antigravity.floating-dock/run/dock.sock`, `0600`) with per-run random token (`dock.token`, `0600`), stale socket cleanup, 64 KiB frame cap, event deduplication, bounded 50-event ring buffer, and secret redaction.
- **Short-lived Hook Relay (`agy-dock-hook-relay`):** Fast Rust binary with 150ms connect/read/write timeouts that forwards hook payloads and exits immediately with `{}` so Antigravity is never blocked.
- **Safe Hook Installer:** Backs up `~/.gemini/config/hooks.json` before modifying, merges/removes only the `"antigravity-floating-dock"` key, preserves existing user hooks, and provides a copyable manual JSON snippet.
- **Honest Session Continuation Fallback (§9):** Modular `SessionAdapter` trait with clear UI notice that sending directly into an existing Antigravity 2.0 session is unsupported, paired with **Copy** and **Open Antigravity** (`⌘+Enter` copies draft and focuses Antigravity).

### Deferred / Out of Scope for MVP
- **Same-session prompt sending from Floating Chat:** Deferred until Antigravity 2.0 exposes a supported external continuation API (see [docs/feasibility-report.md](docs/feasibility-report.md)).
- **Code signing & Apple notarization:** Unsigned local build for personal Apple Silicon use only.
- **Launch at login auto-registration:** Opt-in behavior left to macOS Login Items (`System Settings → General → Login Items`) rather than mutating system launch agents silently.

## Build Instructions (Apple Silicon)

### Prerequisites
- macOS with Apple Command Line Tools (`xcode-select --install`)
- Rust toolchain (`rustup`, `rustc`, `cargo` targeting `aarch64-apple-darwin`)
- Node.js + `pnpm`

### Development & Tests
```bash
pnpm install
pnpm typecheck
pnpm test
pnpm build
cd src-tauri && cargo test --all-targets
```

### Build `.app` and `.dmg`
```bash
pnpm build:relay
pnpm tauri build --target aarch64-apple-darwin
```
Actual bundle outputs are written under:
- `src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Antigravity Floating Dock.app`
- `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Antigravity Floating Dock_1.1.0_aarch64.dmg`

## Setup & Hook Configuration

1. Launch **Antigravity Floating Dock** (an accessory icon appears in the macOS menu bar).
2. Press `Option + Space` (or click **Toggle Floating Chat** from the menu bar icon).
3. Open the **Setup & Settings** tab and click **Install Hook**.
   - This copies `agy-dock-hook-relay` to `~/Library/Application Support/com.antigravity.floating-dock/bin/agy-dock-hook-relay`.
   - It backs up `~/.gemini/config/hooks.json` (if present) to `hooks.json.bak.<timestamp>` and adds only the `"antigravity-floating-dock"` entry (`SessionStart`, `PreInvocation`, `PostToolUse`, `PostInvocation`, `Stop`).
4. Click **Verify** to confirm both the hook entry and relay binary are ready, or use the **Test Working / Idle Stop / Error Stop** buttons to verify bubble & IPC rendering before triggering live Antigravity runs.

## Permissions & Unsigned Build Notes

- **Gatekeeper (unsigned local build):** Because MVP builds are unsigned, if macOS warns when launching a downloaded/copied `.app`, right-click the app in Finder and choose **Open** once (or inspect its build provenance). Do not disable Gatekeeper globally.
- **Global Shortcuts / Input Monitoring:** macOS may require allowing Accessibility/Input Monitoring if a custom shortcut is intercepted by the OS. If a shortcut conflicts with another app (e.g., Spotlight or Raycast on `Option + Space`), the UI displays the registration error so you can remap it in **Setup & Settings**.

## Troubleshooting

- **"Antigravity is not running":** Start Antigravity and keep it running in the background.
- **"Hook not installed":** Open **Setup & Settings** → **Install Hook** (or paste the **Copy JSON** snippet into `~/.gemini/config/hooks.json`).
- **Stale IPC socket:** Restarting the companion automatically cleans up and re-binds `dock.sock` with a fresh `dock.token`.
- **Same-session chat disabled:** Expected for MVP; see [docs/feasibility-report.md](docs/feasibility-report.md). Use `⌘+Enter` or **Copy & Open AGY**.

## Uninstall

1. In **Setup & Settings**, click **Uninstall** to cleanly remove only `"antigravity-floating-dock"` from `~/.gemini/config/hooks.json` (preserving all other hooks).
2. Quit the app from the menu bar (**Quit Antigravity Floating Dock**).
3. Remove the app bundle and local support data if desired:
   ```bash
   rm -rf "/Applications/Antigravity Floating Dock.app"
   rm -rf "$HOME/Library/Application Support/com.antigravity.floating-dock"
   ```

## Credits & License

- **Author:** [@yunkhngn](https://github.com/yunkhngn)
- **Support:** [Buy Me a Coffee](https://buymeacoffee.com/yunkhngn)
- **License:** [MIT](LICENSE)
