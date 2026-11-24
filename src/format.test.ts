import { describe, expect, it } from "vitest";
import {
  formatRelativeTime,
  formatTokenCount,
  resolveBubbleText,
  shortenId,
  stateLabel,
} from "./format";
import type { DockSnapshot } from "./types";

function makeSnapshot(overrides: Partial<DockSnapshot> = {}): DockSnapshot {
  return {
    status: {
      antigravityRunning: true,
      hookInstalled: true,
      relayInstalled: true,
      sidecarInstalled: true,
      sidecarConnected: false,
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
      glassStyle: "regular",
      glassTintOpacity: 0.42,
      cardOpacity: 0.50,
      glassBlurPx: 12,
    },
    activeEvent: null,
    recentEvents: [],
    selectedConversationId: null,
    conversations: [],
    availableModels: [],
    sessionUsage: null,
    queuedMessages: [],
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
        sidecarInstalled: false,
        sidecarConnected: false,
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

  it("formats token counts concisely", () => {
    expect(formatTokenCount(450)).toBe("450");
    expect(formatTokenCount(1450)).toBe("1.5k");
    expect(formatTokenCount(42300)).toBe("42k");
    expect(formatTokenCount(1250000)).toBe("1.3M");
  });
});
