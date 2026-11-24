import type { AgentActivityState, DockSnapshot } from "./types";

export function formatRelativeTime(timestampMs: number | null, nowMs = Date.now()): string {
  if (!timestampMs) return "No events yet";
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 5) return "Just now";
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  return `${diffHr}h ago`;
}

export function shortenId(value: string | null, keep = 8): string {
  if (!value) return "Unknown session";
  if (value.length <= keep) return value;
  return `${value.slice(0, keep)}…`;
}

export function formatTokenCount(count: number): string {
  if (count >= 1_000_000) {
    const val = (Math.round(count / 100_000) / 10).toFixed(1);
    return `${val.replace(/\.0$/, "")}M`;
  }
  if (count >= 1_000) {
    if (count >= 10_000) {
      return `${Math.round(count / 1_000)}k`;
    }
    const val = (Math.round(count / 100) / 10).toFixed(1);
    return `${val.replace(/\.0$/, "")}k`;
  }
  return String(count);
}

export function stateLabel(state: AgentActivityState | undefined): string {
  switch (state) {
    case "working":
      return "Working";
    case "idle":
      return "Idle";
    case "needsAttention":
      return "Needs attention";
    case "error":
      return "Error";
    case "unknown":
    default:
      return "Unknown";
  }
}

export function resolveBubbleText(snapshot: DockSnapshot): {
  title: string;
  subtitle: string;
  tone: AgentActivityState;
} {
  const { status, activeEvent } = snapshot;

  if (!status.antigravityRunning && !activeEvent) {
    return {
      title: "Antigravity offline",
      subtitle: "Open Antigravity to stream events",
      tone: "needsAttention",
    };
  }

  if (!status.hookInstalled && !activeEvent) {
    return {
      title: "Hook not installed",
      subtitle: "Click to set up AGY hook",
      tone: "needsAttention",
    };
  }

  if (!activeEvent) {
    return {
      title: "Listening for AGY",
      subtitle: "Pinned · Ready",
      tone: "idle",
    };
  }

  const subtitleParts: string[] = [];
  if (activeEvent.detail) {
    subtitleParts.push(activeEvent.detail);
  } else if (activeEvent.modelName) {
    subtitleParts.push(activeEvent.modelName);
  }
  if (!status.antigravityRunning) {
    subtitleParts.push("AGY offline");
  } else {
    subtitleParts.push(formatRelativeTime(activeEvent.timestampMs));
  }

  return {
    title: activeEvent.summary,
    subtitle: subtitleParts.join(" · "),
    tone: activeEvent.activityState,
  };
}
