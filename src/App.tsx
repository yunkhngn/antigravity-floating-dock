import { useCallback, useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getSnapshot, isTauriRuntime, subscribeStateUpdates } from "./api";
import { FloatingChat } from "./components/FloatingChat";
import { ProgressBubble } from "./components/ProgressBubble";
import type { DockSnapshot } from "./types";

function detectSurface(): "bubble" | "chat" {
  const params = new URLSearchParams(window.location.search);
  const fromQuery = params.get("surface");
  if (fromQuery === "bubble" || fromQuery === "chat") {
    return fromQuery;
  }
  if (isTauriRuntime()) {
    const label = getCurrentWebviewWindow().label;
    if (label === "bubble") return "bubble";
  }
  return "chat";
}

const PREVIEW_SNAPSHOT: DockSnapshot = {
  status: {
    antigravityRunning: false,
    hookInstalled: false,
    relayInstalled: false,
    ipcListening: false,
    ipcSocketPath: "~/Library/Application Support/com.antigravity.floating-dock/run/dock.sock",
    lastEventAtMs: null,
    vibrancyMode: "css-fallback",
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
    limitationReason:
      "Antigravity 2.0 does not expose a supported external API to send messages into an existing session.",
    supportsOpenInAntigravity: true,
    supportsCopyFallback: true,
  },
  bubblePinned: false,
};

export function App() {
  const [surface] = useState<"bubble" | "chat">(detectSurface);
  const [snapshot, setSnapshot] = useState<DockSnapshot>(PREVIEW_SNAPSHOT);

  const refresh = useCallback(() => {
    if (!isTauriRuntime()) return;
    void getSnapshot()
      .then(setSnapshot)
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    refresh();
    let unlisten: (() => void) | undefined;
    void subscribeStateUpdates((next) => setSnapshot(next)).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, [refresh]);

  if (surface === "bubble") {
    return <ProgressBubble snapshot={snapshot} />;
  }
  return <FloatingChat snapshot={snapshot} onRefresh={refresh} />;
}

export default App;
