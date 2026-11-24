import type { MouseEvent } from "react";
import { hideBubble, setBubblePinned, showChat } from "../api";
import { resolveBubbleText } from "../format";
import type { DockSnapshot } from "../types";
import {
  FilledAlertIcon,
  FilledCloseIcon,
  FilledOrbIcon,
  FilledPinIcon,
  FilledPinOffIcon,
  FilledWaveIcon,
} from "./FilledIcons";

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
    if (tone === "working") return <FilledWaveIcon size={16} />;
    if (tone === "error" || tone === "needsAttention") return <FilledAlertIcon size={16} />;
    return <FilledOrbIcon size={16} />;
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
          {snapshot.bubblePinned ? <FilledPinOffIcon size={14} /> : <FilledPinIcon size={14} />}
        </button>
        <button
          type="button"
          className="icon-btn"
          onClick={handleDismiss}
          title="Dismiss bubble"
          aria-label="Dismiss bubble"
        >
          <FilledCloseIcon size={14} />
        </button>
      </div>
    </div>
  );
}
