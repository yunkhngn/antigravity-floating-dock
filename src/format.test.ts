import { describe, expect, it } from "vitest";
import { formatRelativeTime, resolveBubbleText, shortenId, stateLabel } from "./format";
import type { DockSnapshot } from "./types";

function makeSnapshot(overrides: Partial<DockSnapshot> = {}): DockSnapshot {
  return {
    status: {
      antigravityRunning: true,
      hookInstalled: true,
      relayInstalled: true,
      ipcListening: true,
      ipcSocketPath: "/tmp/dock.sock",
      lastEventAtMs: null,
      vibrancyMode: "liquid-glass",
      shortcutErrors: [],
    },
    settings: {
      toggleChatShortcut: "Alt+Space",
      toggleBubbleShortcut: "Alt+Shift+Space",
      pinBubble: false,
      autoHideIdleMs: 8000,
      launchAtLoginHintAcknowledged: false,
    },
    activeEvent: null,
    recentEvents: [],
    transcriptTail: [],
    transcriptError: null,
    sessionCapabilities: {
      sourceProduct: "antigravity",
      canSendMessage: false,
      limitationReason: "Read-only in MVP",
      supportsOpenInAntigravity: true,
      supportsCopyFallback: true,
    },
    bubblePinned: false,
    ...overrides,
  };
}

describe("format helpers", () => {
  it("formats relative times concisely", () => {
    expect(formatRelativeTime(null, 10_000)).toBe("No events yet");
    expect(formatRelativeTime(9_000, 10_000)).toBe("Just now");
    expect(formatRelativeTime(1_000, 31_000)).toBe("30s ago");
  });

  it("shortens conversation identifiers", () => {
    expect(shortenId(null)).toBe("Unknown session");
    expect(shortenId("abc")).toBe("abc");
    expect(shortenId("ec33ebf9-0cba-4100-8142")).toBe("ec33ebf9…");
  });

  it("reports disconnected state when Antigravity is offline and no event exists", () => {
    const snap = makeSnapshot({
      status: {
        antigravityRunning: false,
        hookInstalled: true,
        relayInstalled: true,
        ipcListening: true,
        ipcSocketPath: "/tmp/dock.sock",
        lastEventAtMs: null,
        vibrancyMode: "vibrancy",
        shortcutErrors: [],
      },
    });
    const res = resolveBubbleText(snap);
    expect(res.title).toBe("Antigravity offline");
    expect(res.tone).toBe("needsAttention");
  });

  it("never reports unknown state as completed", () => {
    expect(stateLabel("unknown")).toBe("Unknown");
    expect(stateLabel("idle")).toBe("Idle");
  });
});
