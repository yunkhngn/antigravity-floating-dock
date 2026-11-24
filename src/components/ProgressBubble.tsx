import type { MouseEvent } from "react";
import { Activity, AlertTriangle, Bot, Pin, PinOff, X } from "lucide-react";
import { hideBubble, setBubblePinned, showChat } from "../api";
import { resolveBubbleText } from "../format";
import type { DockSnapshot } from "../types";

interface ProgressBubbleProps {
  snapshot: DockSnapshot;
}

export function ProgressBubble({ snapshot }: ProgressBubbleProps) {
  const { title, subtitle, tone } = resolveBubbleText(snapshot);
  const useCssFallback = snapshot.status.vibrancyMode === "css-fallback";

  function handleOpenChat() {
    void showChat();
  }

  function handleTogglePin(e: MouseEvent) {
    e.stopPropagation();
    void setBubblePinned(!snapshot.bubblePinned);
  }

  function handleDismiss(e: MouseEvent) {
    e.stopPropagation();
    void hideBubble();
  }

  function renderOrbIcon() {
    if (tone === "working") return <Activity size={14} />;
    if (tone === "error" || tone === "needsAttention") return <AlertTriangle size={14} />;
    return <Bot size={14} />;
  }

  return (
    <div
      className={`bubble-shell ${useCssFallback ? "css-fallback-backdrop" : ""}`}
      data-tauri-drag-region
      onClick={handleOpenChat}
      role="button"
      tabIndex={0}
      aria-label="Open Antigravity Floating Dock"
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          handleOpenChat();
        }
      }}
    >
      <div className="bubble-left" data-tauri-drag-region>
        <div className={`status-orb ${tone}`}>{renderOrbIcon()}</div>
        <div className="bubble-copy" data-tauri-drag-region>
          <div className="bubble-title">{title}</div>
          <div className="bubble-subtitle">{subtitle}</div>
        </div>
      </div>

      <div className="bubble-actions">
        <button
          type="button"
          className={`icon-btn ${snapshot.bubblePinned ? "active" : ""}`}
          onClick={handleTogglePin}
          title={snapshot.bubblePinned ? "Unpin bubble" : "Pin bubble"}
          aria-label={snapshot.bubblePinned ? "Unpin bubble" : "Pin bubble"}
        >
          {snapshot.bubblePinned ? <PinOff size={13} /> : <Pin size={13} />}
        </button>
        <button
          type="button"
          className="icon-btn"
          onClick={handleDismiss}
          title="Dismiss bubble"
          aria-label="Dismiss bubble"
        >
          <X size={13} />
        </button>
      </div>
    </div>
  );
}
