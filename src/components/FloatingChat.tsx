import { useEffect, useState, type KeyboardEvent } from "react";
import {
  Activity,
  AlertTriangle,
  Bot,
  CheckCircle2,
  Copy,
  ExternalLink,
  Maximize2,
  Minimize2,
  Minus,
  Pin,
  PinOff,
  RefreshCw,
  Send,
  Settings,
  ShieldCheck,
  Terminal,
  Trash2,
  Wrench,
} from "lucide-react";
import {
  emitSampleEvent,
  getDiagnostics,
  hideChat,
  installHooks,
  openAntigravity,
  quitApp,
  setBubblePinned,
  setChatExpanded,
  uninstallHooks,
  updateSettings,
  verifyHooks,
} from "../api";
import { formatRelativeTime, shortenId, stateLabel } from "../format";
import type { DockSettings, DockSnapshot, HookStatus } from "../types";

interface FloatingChatProps {
  snapshot: DockSnapshot;
  onRefresh: () => void;
}

type ActiveTab = "activity" | "settings";

export function FloatingChat({ snapshot, onRefresh }: FloatingChatProps) {
  const [tab, setTab] = useState<ActiveTab>("activity");
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const [actionNotice, setActionNotice] = useState<string | null>(null);
  const [hookStatus, setHookStatus] = useState<HookStatus | null>(null);
  const [hookBusy, setHookBusy] = useState(false);
  const [diagnostics, setDiagnostics] = useState<string[]>([]);
  const [settingsForm, setSettingsForm] = useState<DockSettings>(snapshot.settings);

  useEffect(() => {
    setSettingsForm(snapshot.settings);
  }, [snapshot.settings]);

  useEffect(() => {
    function handleKeyDown(e: globalThis.KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        void hideChat();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    if (tab === "settings") {
      void verifyHooks().then(setHookStatus).catch(() => {});
      void getDiagnostics().then(setDiagnostics).catch(() => {});
    }
  }, [tab]);

  const useCssFallback = snapshot.status.vibrancyMode === "css-fallback";
  const { status, activeEvent, recentEvents, transcriptTail, transcriptError, sessionCapabilities } =
    snapshot;

  async function handleToggleExpand() {
    const next = !expanded;
    setExpanded(next);
    await setChatExpanded(next);
  }

  async function handleCopyDraft(alsoOpen: boolean) {
    const text = draft.trim();
    if (!text) {
      if (alsoOpen) {
        await handleOpenAntigravity();
      }
      return;
    }

    try {
      await navigator.clipboard.writeText(text);
      setActionNotice(
        alsoOpen
          ? "Copied draft to clipboard and focused Antigravity (direct same-session send is not supported)."
          : "Copied draft to clipboard.",
      );
      if (alsoOpen) {
        await openAntigravity(sessionCapabilities.sourceProduct);
      }
    } catch {
      setActionNotice("Could not copy to clipboard.");
    }
  }

  async function handleOpenAntigravity() {
    try {
      await openAntigravity(sessionCapabilities.sourceProduct);
      setActionNotice("Focused Antigravity.");
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    }
  }

  function handleComposerKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      if (!sessionCapabilities.canSendMessage) {
        void handleCopyDraft(true);
      }
    }
  }

  async function handleInstallHooks() {
    setHookBusy(true);
    try {
      const res = await installHooks();
      setHookStatus(res.status);
      setActionNotice(
        res.backupPath ? `${res.message} Backup: ${res.backupPath}` : res.message,
      );
      onRefresh();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    } finally {
      setHookBusy(false);
    }
  }

  async function handleUninstallHooks() {
    setHookBusy(true);
    try {
      const res = await uninstallHooks();
      setHookStatus(res.status);
      setActionNotice(res.message);
      onRefresh();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    } finally {
      setHookBusy(false);
    }
  }

  async function handleSaveSettings() {
    try {
      await updateSettings(settingsForm);
      setActionNotice("Settings saved.");
      onRefresh();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleCopySnippet() {
    if (!hookStatus?.snippet) return;
    try {
      await navigator.clipboard.writeText(hookStatus.snippet);
      setActionNotice("Copied hook JSON snippet.");
    } catch {
      setActionNotice("Failed to copy snippet.");
    }
  }

  const connectionDotClass = !status.antigravityRunning
    ? "err"
    : !status.hookInstalled || !status.ipcListening
      ? "warn"
      : "ok";

  const connectionLabel = !status.antigravityRunning
    ? "Antigravity not running"
    : !status.hookInstalled
      ? "Hook not installed"
      : !status.ipcListening
        ? "IPC offline"
        : activeEvent?.conversationId
          ? `Session ${shortenId(activeEvent.conversationId)}`
          : "Connected · Waiting for events";

  return (
    <div className={`chat-shell ${useCssFallback ? "css-fallback-backdrop" : ""}`}>
      <header className="chat-header" data-tauri-drag-region>
        <div className="chat-brand" data-tauri-drag-region>
          <div className="brand-icon">
            <Bot size={14} />
          </div>
          <div className="brand-meta" data-tauri-drag-region>
            <div className="brand-title">Antigravity Floating Dock</div>
            <div className="brand-status">
              <span className={`dot ${connectionDotClass}`} />
              <span>{connectionLabel}</span>
            </div>
          </div>
        </div>

        <div className="header-controls">
          <button
            type="button"
            className={`icon-btn ${snapshot.bubblePinned ? "active" : ""}`}
            onClick={() => void setBubblePinned(!snapshot.bubblePinned)}
            title={snapshot.bubblePinned ? "Unpin status bubble" : "Pin status bubble"}
            aria-label={snapshot.bubblePinned ? "Unpin status bubble" : "Pin status bubble"}
          >
            {snapshot.bubblePinned ? <PinOff size={14} /> : <Pin size={14} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            onClick={() => void handleToggleExpand()}
            title={expanded ? "Compact size" : "Expand panel"}
            aria-label={expanded ? "Compact size" : "Expand panel"}
          >
            {expanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            onClick={() => void hideChat()}
            title="Hide panel (Esc)"
            aria-label="Hide panel"
          >
            <Minus size={14} />
          </button>
        </div>
      </header>

      <div className="tab-strip">
        <button
          type="button"
          className={`tab-btn ${tab === "activity" ? "active" : ""}`}
          onClick={() => setTab("activity")}
        >
          <Activity size={12} />
          <span>Session</span>
        </button>
        <button
          type="button"
          className={`tab-btn ${tab === "settings" ? "active" : ""}`}
          onClick={() => setTab("settings")}
        >
          <Settings size={12} />
          <span>Setup &amp; Settings</span>
        </button>
      </div>

      {tab === "activity" ? (
        <>
          <div className="chat-body">
            {!status.antigravityRunning && (
              <div className="alert-banner error">
                <div>
                  <strong>Antigravity is not running.</strong> Keep Antigravity open in the
                  background to stream live agent events.
                </div>
                <button type="button" className="btn" onClick={() => void handleOpenAntigravity()}>
                  <ExternalLink size={12} />
                  <span>Open</span>
                </button>
              </div>
            )}

            {!status.hookInstalled && (
              <div className="alert-banner">
                <div>
                  <strong>Hook bridge not configured.</strong> Install the companion entry in{" "}
                  <code>~/.gemini/config/hooks.json</code> to receive live lifecycle events.
                </div>
                <button type="button" className="btn" onClick={() => setTab("settings")}>
                  <Wrench size={12} />
                  <span>Setup</span>
                </button>
              </div>
            )}

            {status.shortcutErrors.length > 0 && (
              <div className="alert-banner">
                <div>
                  <strong>Shortcut conflict:</strong> {status.shortcutErrors[0]}
                </div>
                <button type="button" className="btn" onClick={() => setTab("settings")}>
                  <span>Fix</span>
                </button>
              </div>
            )}

            {actionNotice && (
              <div className="alert-banner info">
                <span>{actionNotice}</span>
                <button type="button" className="icon-btn" onClick={() => setActionNotice(null)}>
                  <Minus size={12} />
                </button>
              </div>
            )}

            <section className="card">
              <div className="card-title-row">
                <span className="card-label">Active Session</span>
                <span className={`badge ${activeEvent?.activityState ?? "unknown"}`}>
                  {stateLabel(activeEvent?.activityState)}
                </span>
              </div>

              {activeEvent ? (
                <div className="kv-grid">
                  <span className="kv-key">Status</span>
                  <span className="kv-val">{activeEvent.summary}</span>
                  <span className="kv-key">Session</span>
                  <span className="kv-val">{activeEvent.conversationId ?? "Not supplied"}</span>
                  <span className="kv-key">Workspace</span>
                  <span className="kv-val">{activeEvent.workspacePath ?? "Not supplied"}</span>
                  <span className="kv-key">Model</span>
                  <span className="kv-val">{activeEvent.modelName ?? "Not supplied"}</span>
                  <span className="kv-key">Updated</span>
                  <span className="kv-val">{formatRelativeTime(activeEvent.timestampMs)}</span>
                </div>
              ) : (
                <div className="hint-text">
                  No lifecycle events received yet. Start a turn in Antigravity or emit a sample
                  event in Setup &amp; Settings.
                </div>
              )}
            </section>

            <section className="card">
              <div className="card-title-row">
                <span className="card-label">Recent Events</span>
                <button type="button" className="icon-btn" onClick={onRefresh} title="Refresh">
                  <RefreshCw size={12} />
                </button>
              </div>
              {recentEvents.length > 0 ? (
                <div className="event-list">
                  {recentEvents.slice(0, 8).map((ev) => (
                    <div key={ev.id} className="event-row">
                      <div className="event-row-main">
                        <span className="event-row-title">{ev.summary}</span>
                        {ev.detail && <span className="event-row-detail">{ev.detail}</span>}
                      </div>
                      <span className="event-row-time">{formatRelativeTime(ev.timestampMs)}</span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="hint-text">Bounded event buffer is empty.</div>
              )}
            </section>

            <section className="card">
              <div className="card-title-row">
                <span className="card-label">Transcript Context (Read-Only)</span>
                <span className="hint-text">{sessionCapabilities.sourceProduct}</span>
              </div>
              {transcriptTail.length > 0 ? (
                <div className="transcript-list">
                  {transcriptTail.map((msg, idx) => (
                    <div key={`${msg.role}-${idx}`} className="transcript-item">
                      <div className="transcript-role">{msg.role}</div>
                      <div>{msg.text}</div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="hint-text">
                  {transcriptError ??
                    "No readable JSONL transcript available for the current event."}
                </div>
              )}
            </section>
          </div>

          <footer className="composer-area">
            <div className="limitation-note">
              <AlertTriangle size={11} style={{ verticalAlign: "text-bottom", marginRight: 4 }} />
              {sessionCapabilities.limitationReason}
            </div>
            <textarea
              className="composer-input"
              placeholder="Draft a follow-up prompt (⌘+Enter copies & focuses Antigravity)…"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={handleComposerKeyDown}
            />
            <div className="composer-footer">
              <div className="btn-row">
                <button
                  type="button"
                  className="btn"
                  disabled={!draft.trim()}
                  onClick={() => void handleCopyDraft(false)}
                >
                  <Copy size={12} />
                  <span>Copy</span>
                </button>
                <button type="button" className="btn" onClick={() => void handleCopyDraft(true)}>
                  <ExternalLink size={12} />
                  <span>{draft.trim() ? "Copy & Open AGY" : "Open Antigravity"}</span>
                </button>
              </div>
              <button
                type="button"
                className="btn primary"
                disabled
                title={sessionCapabilities.limitationReason}
              >
                <Send size={12} />
                <span>Send (Unavailable)</span>
              </button>
            </div>
          </footer>
        </>
      ) : (
        <div className="chat-body">
          {actionNotice && (
            <div className="alert-banner info">
              <span>{actionNotice}</span>
              <button type="button" className="icon-btn" onClick={() => setActionNotice(null)}>
                <Minus size={12} />
              </button>
            </div>
          )}

          <section className="card">
            <div className="card-title-row">
              <span className="card-label">Antigravity Hook Bridge</span>
              <span className="badge">
                {hookStatus?.hookInstalled ? (
                  <>
                    <CheckCircle2 size={11} style={{ verticalAlign: "text-bottom", marginRight: 3 }} />
                    Installed
                  </>
                ) : (
                  "Not installed"
                )}
              </span>
            </div>
            <div className="hint-text">
              Adds only the <code>antigravity-floating-dock</code> entry to{" "}
              <code>{hookStatus?.hooksFilePath ?? "~/.gemini/config/hooks.json"}</code> after
              backing up existing hooks.
            </div>
            {hookStatus?.error && <div className="alert-banner error">{hookStatus.error}</div>}
            <div className="btn-row">
              <button
                type="button"
                className="btn primary"
                disabled={hookBusy}
                onClick={() => void handleInstallHooks()}
              >
                <ShieldCheck size={12} />
                <span>{hookStatus?.hookInstalled ? "Repair / Update Hook" : "Install Hook"}</span>
              </button>
              <button
                type="button"
                className="btn"
                disabled={hookBusy}
                onClick={() => void verifyHooks().then(setHookStatus)}
              >
                <RefreshCw size={12} />
                <span>Verify</span>
              </button>
              <button
                type="button"
                className="btn danger"
                disabled={hookBusy || !hookStatus?.hookInstalled}
                onClick={() => void handleUninstallHooks()}
              >
                <Trash2 size={12} />
                <span>Uninstall</span>
              </button>
              <button type="button" className="btn" onClick={() => void handleCopySnippet()}>
                <Copy size={12} />
                <span>Copy JSON</span>
              </button>
            </div>
            {hookStatus?.snippet && <pre className="code-block">{hookStatus.snippet}</pre>}
          </section>

          <section className="card">
            <div className="card-title-row">
              <span className="card-label">Shortcuts &amp; Surface</span>
              <span className="hint-text">Material: {status.vibrancyMode}</span>
            </div>
            <div className="field-group">
              <label className="field-label" htmlFor="chat-shortcut">
                Toggle Floating Chat (e.g. Alt+Space, CommandOrControl+Shift+Space)
              </label>
              <input
                id="chat-shortcut"
                className="text-input"
                value={settingsForm.toggleChatShortcut}
                onChange={(e) =>
                  setSettingsForm({ ...settingsForm, toggleChatShortcut: e.target.value })
                }
              />
            </div>
            <div className="field-group">
              <label className="field-label" htmlFor="bubble-shortcut">
                Toggle / Pin Progress Bubble
              </label>
              <input
                id="bubble-shortcut"
                className="text-input"
                value={settingsForm.toggleBubbleShortcut}
                onChange={(e) =>
                  setSettingsForm({ ...settingsForm, toggleBubbleShortcut: e.target.value })
                }
              />
            </div>
            <div className="btn-row">
              <button type="button" className="btn primary" onClick={() => void handleSaveSettings()}>
                <span>Save Shortcuts</span>
              </button>
            </div>
          </section>

          <section className="card">
            <div className="card-title-row">
              <span className="card-label">Fixture Verification &amp; Diagnostics</span>
              <span className="hint-text">IPC: {status.ipcListening ? "Listening" : "Offline"}</span>
            </div>
            <div className="btn-row">
              <button
                type="button"
                className="btn"
                onClick={() => void emitSampleEvent("working")}
              >
                <Terminal size={12} />
                <span>Test Working</span>
              </button>
              <button type="button" className="btn" onClick={() => void emitSampleEvent("idle")}>
                <span>Test Idle Stop</span>
              </button>
              <button type="button" className="btn" onClick={() => void emitSampleEvent("error")}>
                <span>Test Error Stop</span>
              </button>
            </div>
            <div className="hint-text">Socket: {status.ipcSocketPath}</div>
            {diagnostics.length > 0 && (
              <pre className="code-block">{diagnostics.slice(-12).join("\n")}</pre>
            )}
            <div className="btn-row">
              <button type="button" className="btn danger" onClick={() => void quitApp()}>
                <span>Quit Antigravity Floating Dock</span>
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
