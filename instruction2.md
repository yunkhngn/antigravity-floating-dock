# Task: Implement Antigravity Composer Interaction for Floating Dock

You are working on the existing **Antigravity Floating Dock** macOS application.

## Objective

Implement a reliable way for Floating Dock to send a user message into the **currently active Antigravity desktop conversation**.

Instead of reverse-engineering Antigravity's undocumented internal messaging protocol, interact with its actual chat composer — the message input field and Send button — using macOS native automation.

The expected interaction should behave like a real user typing and sending a message in Antigravity.

**Do not redesign the UI. Do not modify the existing Floating Dock visual design. Focus exclusively on the message-sending pipeline.**

## 1. Inspect the Existing Architecture

Before implementing anything:

- Inspect the existing Rust backend and frontend.
- Locate `SessionAdapter`, conversation/session discovery, message sending, and existing fallback logic.
- Identify how the current `conversationId` is obtained.
- Understand how Floating Dock currently handles Send, loading, errors, and responses.
- Preserve existing interfaces wherever possible.

Do not replace working components unnecessarily.

## 2. Implement DesktopInteractionAdapter

Create a dedicated Rust module responsible for interacting with Antigravity's native chat composer.

Suggested structure:

```text
src-tauri/src/
  adapters/
    session_adapter.rs
    desktop_interaction/
      mod.rs
      accessibility.rs
      keyboard.rs
      coordinate_fallback.rs
```

Adapt this structure to the existing project conventions.

### Priority 1: macOS Accessibility API

Use macOS Accessibility APIs through `AXUIElement` to:

1. Locate the Antigravity application process.
2. Inspect its accessibility tree.
3. Identify the currently active conversation window.
4. Locate the actual chat composer.
5. Set or insert the requested message text.
6. Locate and trigger the Send button.
7. Verify that the message was actually submitted.

Requirements:

- Identify UI elements by accessibility role, title, description, and available attributes — never rely exclusively on fixed coordinates.
- Handle different composer states, including empty, populated, disabled, and loading.
- Avoid accidentally interacting with a new conversation.
- Verify the target window and active conversation before sending.
- Do not assume accessibility elements are always available or writable.
- Handle macOS Accessibility permission denial gracefully.

### Priority 2: Keyboard Automation

If Accessibility API can locate the composer but cannot reliably submit the message:

- Activate the correct Antigravity window.
- Focus the composer.
- Paste the message using the macOS pasteboard.
- Trigger the appropriate Send action.
- Restore Floating Dock's previous state if possible.

Prefer paste over simulated character-by-character typing to preserve Unicode, Vietnamese characters, multiline text, and code blocks.

Do not send Enter blindly if it may insert a newline instead of submitting.

### Priority 3: Coordinate-Based Fallback

Only if Accessibility and keyboard-based interaction cannot work:

- Determine the Antigravity window's actual screen bounds.
- Calculate the composer and Send button positions relative to that window.
- Account for window resizing, display scaling, and multiple monitors.
- Never hardcode absolute screen coordinates.
- Verify the target window is still correct immediately before interaction.

Coordinate-based automation must be isolated behind its own adapter.

## 3. Conversation Safety

This is critical.

Floating Dock must send messages to the **existing active conversation**, not create a new conversation.

Before every send:

- Verify Antigravity is running.
- Verify the target window.
- Verify the expected conversation identity when available.
- Check that the composer belongs to the intended conversation.
- Abort if the target is ambiguous.

Use the existing `conversationId` from the session discovery mechanism wherever possible.

If the active conversation cannot be identified confidently, fail safely and offer the user an option to open Antigravity manually.

Never silently send a message to an unknown conversation.

## 4. Message Submission Flow

Implement the following flow:

```text
Floating Dock Send
       |
       v
Validate message
       |
       v
Resolve active Antigravity window
       |
       v
Verify active conversation
       |
       v
Find chat composer
       |
       v
Insert message
       |
       v
Trigger Send
       |
       v
Verify submission
       |
       v
Return result to Floating Dock
```

Use explicit result states:

- `Success`: message was submitted successfully.
- `AppNotRunning`: Antigravity is not running.
- `PermissionDenied`: required macOS permissions are missing.
- `ComposerNotFound`: chat composer could not be located.
- `ConversationMismatch`: active conversation does not match the expected session.
- `SubmissionFailed`: message could not be submitted.
- `TargetAmbiguous`: multiple possible targets were found.

Do not report success merely because text was inserted into the composer.

## 5. Response and Status Handling

Keep message submission separate from response monitoring.

After a successful send:

- Preserve the existing hook/session monitoring mechanism.
- Continue using existing conversation identifiers and transcript information where available.
- Do not assume that submitting a message means the agent has started processing it.
- Report submission success separately from agent execution status.

Do not implement a second, competing conversation-monitoring system.

## 6. macOS Permissions

Check the required permissions for:

- Accessibility.
- Input Monitoring, if required by the selected automation mechanism.

Provide clear instructions to the user when permission is missing.

Do not automatically modify system privacy settings or attempt to bypass macOS security controls.

Do not disable Gatekeeper or weaken system security.

## 7. Testing

Test against the actual installed Antigravity application.

At minimum, verify:

1. Sending a simple English message.
2. Sending Vietnamese text with Unicode characters.
3. Sending multiline messages.
4. Sending code blocks and special characters.
5. Sending while Antigravity is minimized.
6. Sending while another application is focused.
7. Handling a new conversation.
8. Handling an unexpected conversation switch.
9. Handling a missing Accessibility permission.
10. Handling Antigravity not running.
11. Handling a disabled or unavailable composer.
12. Preventing duplicate submissions.

Use a disposable conversation for testing.

Do not send test messages into the user's actual working conversation.

## 8. Engineering Requirements

- Implement in Rust using the existing Tauri 2 architecture.
- Keep platform-specific code isolated under `#[cfg(target_os = "macos")]`.
- Use native macOS APIs where appropriate.
- Avoid unnecessary dependencies.
- Use asynchronous execution where UI interaction could block the application.
- Add bounded timeouts.
- Avoid duplicate submissions on retries.
- Sanitize logs: never log message contents, credentials, or sensitive conversation data.
- Provide meaningful error messages to the frontend.
- Preserve existing frontend APIs and UI behavior as much as possible.

## 9. Reverse Engineering Permission

You are explicitly allowed to investigate the locally installed Antigravity application to understand its UI and accessibility behavior.

You may inspect:

- Accessibility tree and element attributes.
- Running processes and window metadata.
- Application bundle metadata.
- Local logs relevant to UI interaction.
- Read-only local application state when necessary.

Do not patch or modify Antigravity's application bundle, bypass authentication, extract credentials, or tamper with its internal databases.

Prefer documented macOS APIs and observable UI behavior.

## 10. Final Deliverables

After implementation, report:

1. Root cause of the previous message-sending failure.
2. Automation strategy implemented.
3. Files created or modified.
4. Required macOS permissions.
5. Test cases executed and their results.
6. Known limitations.
7. Build and run instructions.

**Important:** Do not stop after creating an adapter interface or a mocked implementation. The objective is a working end-to-end message submission into the real Antigravity desktop composer.

If the preferred Accessibility approach fails, investigate why and implement the next fallback. Do not silently leave message sending unimplemented.