import { useEffect, useRef, useState, type ClipboardEvent, type KeyboardEvent } from "react";
import {
  clearQueue,
  flushQueueItem,
  hideChat,
  installHooks,
  openAntigravity,
  quitApp,
  refreshConversations,
  removeQueueItem,
  requestAccessibilityPermission,
  saveAttachmentImage,
  selectConversation,
  sendMessageToAntigravity,
  setBubblePinned,
  setChatExpanded,
  uninstallHooks,
  updateSettings,
  verifyHooks,
} from "../api";
import { formatRelativeTime, shortenId, stateLabel } from "../format";
import {
  FilledCheckCircleIcon,
  FilledCloseIcon,
  FilledCollapseIcon,
  FilledCopyIcon,
  FilledExpandIcon,
  FilledExternalIcon,
  FilledImageIcon,
  FilledMinusIcon,
  FilledOrbIcon,
  FilledPinIcon,
  FilledPinOffIcon,
  FilledRefreshIcon,
  FilledSendIcon,
  FilledSettingsIcon,
  FilledShieldCheckIcon,
  FilledTrashIcon,
  FilledWaveIcon,
} from "./FilledIcons";
import { MessageText } from "./MessageText";
import type {
  AttachmentInfo,
  DockSettings,
  DockSnapshot,
  HookStatus,
} from "../types";

interface FloatingChatProps {
  snapshot: DockSnapshot;
  onRefresh: () => void;
}

type ActiveTab = "activity" | "settings";

interface AttachedImage extends AttachmentInfo {
  previewUrl: string;
}

export function FloatingChat({ snapshot, onRefresh }: FloatingChatProps) {
  const [tab, setTab] = useState<ActiveTab>("activity");
  const [expanded, setExpanded] = useState(false);
  const [showInspector, setShowInspector] = useState(false);
  const [expandedActivityKey, setExpandedActivityKey] = useState<Record<string, boolean>>({});
  const [liveStepsExpanded, setLiveStepsExpanded] = useState(true);
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<AttachedImage[]>([]);
  const [sending, setSending] = useState(false);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const [actionNotice, setActionNotice] = useState<string | null>(null);
  const [permissionNeeded, setPermissionNeeded] = useState(false);
  const [hookStatus, setHookStatus] = useState<HookStatus | null>(null);
  const [hookBusy, setHookBusy] = useState(false);
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
    void refreshConversations().catch(() => {});
  }, [snapshot.status.sidecarConnected]);

  useEffect(() => {
    if (tab === "settings") {
      void verifyHooks().then(setHookStatus).catch(() => {});
    }
  }, [tab]);

  useEffect(() => {
    if (tab !== "activity") return;
    const el = chatScrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [
    tab,
    snapshot.transcriptTail,
    snapshot.recentEvents,
    snapshot.selectedConversationId,
  ]);

  const useCssFallback = snapshot.status.vibrancyMode === "css-fallback";
  const {
    status,
    activeEvent,
    recentEvents,
    selectedConversationId,
    conversations,
    queuedMessages,
    transcriptTail,
    transcriptError,
    sessionCapabilities,
  } = snapshot;

  const targetConversationId =
    selectedConversationId ?? activeEvent?.conversationId ?? conversations[0]?.id ?? null;
  const selectedConversationMeta = conversations.find((c) => c.id === targetConversationId);
  const isTargetWorking =
    (activeEvent?.activityState === "working" &&
      (!targetConversationId ||
        !activeEvent.conversationId ||
        activeEvent.conversationId === targetConversationId)) ||
    selectedConversationMeta?.runState === "RUN_STATE_RUNNING";
  const targetRecentSteps = recentEvents
    .filter(
      (ev) =>
        !targetConversationId ||
        !ev.conversationId ||
        ev.conversationId === targetConversationId,
    )
    .slice(0, 12)
    .reverse();

  function composePayloadText(): string {
    const text = draft.trim();
    if (attachments.length === 0) return text;
    const refs = attachments
      .map((img) => `[Attached image: ${img.path}]`)
      .join("\n");
    return text ? `${text}\n\n${refs}` : `Please inspect the attached image(s):\n${refs}`;
  }

  async function handleAddImageFiles(files: FileList | File[]) {
    const list = Array.from(files).filter((f) => f.type.startsWith("image/"));
    if (list.length === 0) return;

    const remaining = Math.max(0, 4 - attachments.length);
    if (remaining === 0) {
      setActionNotice("Maximum 4 images per message.");
      return;
    }

    for (const file of list.slice(0, remaining)) {
      try {
        const dataUrl = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(String(reader.result || ""));
          reader.onerror = () => reject(new Error("Failed to read image"));
          reader.readAsDataURL(file);
        });
        const saved = await saveAttachmentImage(file.name || "pasted.png", dataUrl);
        setAttachments((prev) => [...prev, { ...saved, previewUrl: dataUrl }]);
      } catch (err) {
        setActionNotice(err instanceof Error ? err.message : String(err));
      }
    }
  }

  function handleComposerPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    const items = e.clipboardData?.items;
    if (!items) return;
    const imageFiles: File[] = [];
    for (const item of Array.from(items)) {
      if (item.kind === "file" && item.type.startsWith("image/")) {
        const file = item.getAsFile();
        if (file) imageFiles.push(file);
      }
    }
    if (imageFiles.length > 0) {
      e.preventDefault();
      void handleAddImageFiles(imageFiles);
    }
  }

  async function handleRefreshSessions() {
    setLoadingSessions(true);
    try {
      await refreshConversations();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingSessions(false);
    }
  }

  async function handleSelectSession(cid: string) {
    try {
      await selectConversation(cid === "__auto__" ? null : cid);
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleToggleExpand() {
    const next = !expanded;
    setExpanded(next);
    await setChatExpanded(next);
  }

  async function handleCopyDraft(alsoOpen: boolean) {
    const text = composePayloadText();
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
          ? "Copied draft to clipboard and focused Antigravity."
          : "Copied draft to clipboard.",
      );
      if (alsoOpen) {
        await openAntigravity(
          sessionCapabilities.sourceProduct,
          targetConversationId,
        );
      }
    } catch {
      setActionNotice("Could not copy to clipboard.");
    }
  }

  async function handleOpenAntigravity() {
    try {
      await openAntigravity(
        sessionCapabilities.sourceProduct,
        targetConversationId,
      );
      setActionNotice("Focused Antigravity.");
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleSendMessage() {
    const text = composePayloadText();
    if (!text || sending) return;

    setSending(true);
    setPermissionNeeded(false);
    setActionNotice(null);

    try {
      const result = await sendMessageToAntigravity({
        message: text,
        expectedConversationId: targetConversationId,
        expectedConversationTitle: selectedConversationMeta?.title ?? null,
        expectedWorkspacePath: activeEvent?.workspacePath ?? null,
        sourceProduct: sessionCapabilities.sourceProduct,
        idempotencyKey: `${Date.now()}-${text.length}`,
        allowKeyboardFallback: true,
        allowCoordinateFallback: true,
      });

      if (
        (result.status === "Success" && result.verifiedSubmitted) ||
        result.status === "Queued"
      ) {
        setDraft("");
        setAttachments([]);
      } else if (result.status === "PermissionDenied") {
        setPermissionNeeded(true);
        setActionNotice(result.permissionStatus.instructions || result.message);
      } else {
        setActionNotice(result.message);
      }
      onRefresh();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    } finally {
      setSending(false);
    }
  }

  async function handleGrantAccessibility() {
    try {
      const perms = await requestAccessibilityPermission();
      setPermissionNeeded(!perms.accessibilityTrusted);
      setActionNotice(perms.instructions);
      onRefresh();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    }
  }

  function applyGlassPreview(next: DockSettings) {
    setSettingsForm(next);
    const root = document.documentElement.style;
    root.setProperty("--glass-tint-alpha", String(next.glassTintOpacity));
    root.setProperty("--card-alpha", String(next.cardOpacity));
    root.setProperty("--glass-blur", `${next.glassBlurPx}px`);
  }

  async function persistSettings(next: DockSettings, notice?: string) {
    applyGlassPreview(next);
    try {
      await updateSettings(next);
      if (notice) setActionNotice(notice);
      onRefresh();
    } catch (err) {
      setActionNotice(err instanceof Error ? err.message : String(err));
    }
  }

  function handleComposerKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void handleSendMessage();
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
        : targetConversationId
          ? `Session ${shortenId(targetConversationId)}`
          : "Connected · Waiting for events";

  return (
    <div className={`chat-shell ${useCssFallback ? "css-fallback-backdrop" : ""}`}>
      <header className="chat-header" data-tauri-drag-region>
        <div className="chat-brand" data-tauri-drag-region>
          <div className="brand-icon">
            <FilledOrbIcon size={16} />
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
            {snapshot.bubblePinned ? <FilledPinOffIcon size={15} /> : <FilledPinIcon size={15} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            onClick={() => void handleToggleExpand()}
            title={expanded ? "Compact size" : "Expand panel"}
            aria-label={expanded ? "Compact size" : "Expand panel"}
          >
            {expanded ? <FilledCollapseIcon size={15} /> : <FilledExpandIcon size={15} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            onClick={() => void hideChat()}
            title="Hide panel (Esc)"
            aria-label="Hide panel"
          >
            <FilledMinusIcon size={15} />
          </button>
        </div>
      </header>

      <div className="tab-strip">
        <button
          type="button"
          className={`tab-btn ${tab === "activity" ? "active" : ""}`}
          onClick={() => setTab("activity")}
        >
          <FilledWaveIcon size={13} />
          <span>Session</span>
        </button>
        <button
          type="button"
          className={`tab-btn ${tab === "settings" ? "active" : ""}`}
          onClick={() => setTab("settings")}
        >
          <FilledSettingsIcon size={13} />
          <span>Settings</span>
        </button>
      </div>

      {tab === "activity" ? (
        <>
          <div className="session-toolbar">
            <div className="session-picker-row">
              <select
                className="session-select"
                value={selectedConversationId ?? "__auto__"}
                onChange={(e) => void handleSelectSession(e.target.value)}
                aria-label="Select conversation session"
              >
                <option value="__auto__">
                  Auto (
                  {activeEvent?.conversationId || conversations[0]?.id
                    ? shortenId(activeEvent?.conversationId ?? conversations[0]?.id ?? "")
                    : "latest"}
                  )
                </option>
                {conversations.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.title} {c.runState === "RUN_STATE_RUNNING" ? "●" : ""} ({shortenId(c.id)})
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="icon-btn"
                onClick={() => void handleRefreshSessions()}
                disabled={loadingSessions}
                title="Refresh Antigravity sessions"
              >
                <FilledRefreshIcon size={13} />
              </button>
              <button
                type="button"
                className={`pill-toggle ${showInspector ? "active" : ""}`}
                onClick={() => setShowInspector((prev) => !prev)}
                title="Toggle session details and recent events"
              >
                <span className={`badge ${activeEvent?.activityState ?? "unknown"}`}>
                  {stateLabel(activeEvent?.activityState)}
                </span>
                <span className="pill-toggle-label">
                  {showInspector ? "Hide info" : "Info"}
                </span>
              </button>
            </div>
            {(activeEvent?.summary || selectedConversationMeta?.title) && (
              <div className="session-status-line">
                <span>
                  {activeEvent?.summary ??
                    selectedConversationMeta?.title ??
                    "Ready"}
                </span>
                {activeEvent && (
                  <span className="session-status-time">
                    {formatRelativeTime(activeEvent.timestampMs)}
                  </span>
                )}
              </div>
            )}
          </div>

          <div className="chat-body" ref={chatScrollRef}>
            {!status.antigravityRunning && (
              <div className="alert-banner error">
                <div>
                  <strong>Antigravity is not running.</strong> Keep Antigravity open in the
                  background to stream live agent events.
                </div>
                <button type="button" className="btn" onClick={() => void handleOpenAntigravity()}>
                  <FilledExternalIcon size={13} />
                  <span>Open</span>
                </button>
              </div>
            )}

            {!status.hookInstalled && (
              <div className="alert-banner">
                <div>
                  <strong>Bridge not configured.</strong> Install the Antigravity Bridge in Settings
                  to stream live events and send in background.
                </div>
                <button type="button" className="btn" onClick={() => setTab("settings")}>
                  <FilledSettingsIcon size={13} />
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
              <div className={`alert-banner ${permissionNeeded ? "" : "info"}`}>
                <span>{actionNotice}</span>
                <div className="btn-row">
                  {permissionNeeded && (
                    <button
                      type="button"
                      className="btn primary"
                      onClick={() => void handleGrantAccessibility()}
                    >
                      <FilledShieldCheckIcon size={13} />
                      <span>Grant Access</span>
                    </button>
                  )}
                  <button
                    type="button"
                    className="icon-btn"
                    onClick={() => {
                      setActionNotice(null);
                      setPermissionNeeded(false);
                    }}
                  >
                    <FilledCloseIcon size={13} />
                  </button>
                </div>
              </div>
            )}

            {showInspector && (
              <section className="card inspector-card">
                <div className="card-title-row">
                  <span className="card-label">Session Metadata</span>
                  <button type="button" className="icon-btn" onClick={onRefresh} title="Refresh">
                    <FilledRefreshIcon size={13} />
                  </button>
                </div>
                {activeEvent || selectedConversationMeta ? (
                  <div className="kv-grid">
                    <span className="kv-key">Session</span>
                    <span className="kv-val">{targetConversationId ?? "Not supplied"}</span>
                    <span className="kv-key">Workspace</span>
                    <span className="kv-val">{activeEvent?.workspacePath ?? "Not supplied"}</span>
                    <span className="kv-key">Model</span>
                    <span className="kv-val">{activeEvent?.modelName ?? "Not supplied"}</span>
                  </div>
                ) : (
                  <div className="hint-text">No lifecycle events received yet.</div>
                )}

                {recentEvents.length > 0 && (
                  <div className="event-list">
                    <span className="card-label">Recent Events</span>
                    {recentEvents.slice(0, 5).map((ev) => (
                      <div key={ev.id} className="event-row">
                        <div className="event-row-main">
                          <span className="event-row-title">{ev.summary}</span>
                          {ev.detail && <span className="event-row-detail">{ev.detail}</span>}
                        </div>
                        <span className="event-row-time">{formatRelativeTime(ev.timestampMs)}</span>
                      </div>
                    ))}
                  </div>
                )}
              </section>
            )}

            {transcriptTail.length > 0 ? (
              <div className="transcript-stream">
                {transcriptTail.map((msg, idx) => {
                  const steps = msg.steps ?? [];
                  const stepKey = `${idx}-${msg.role}`;
                  const isOpen = !!expandedActivityKey[stepKey];
                  if (msg.role === "Activity") {
                    return (
                      <div key={stepKey} className="activity-trace">
                        <button
                          type="button"
                          className="activity-trace-toggle"
                          onClick={() =>
                            setExpandedActivityKey((prev) => ({
                              ...prev,
                              [stepKey]: !isOpen,
                            }))
                          }
                        >
                          <span>
                            {steps[steps.length - 1] ?? "Agent steps"} ({steps.length})
                          </span>
                          <span className="activity-chevron">{isOpen ? "⌄" : "›"}</span>
                        </button>
                        {isOpen && (
                          <div className="activity-step-list">
                            {steps.map((step, sIdx) => (
                              <div key={sIdx} className="activity-step-row">
                                {step}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    );
                  }
                  return (
                    <div
                      key={stepKey}
                      className={`chat-msg ${msg.role === "You" ? "user" : "assistant"}`}
                    >
                      {msg.role === "You" ? (
                        <div className="chat-msg-user-box">
                          <MessageText text={msg.text} />
                        </div>
                      ) : (
                        <div className="chat-msg-assistant-prose">
                          {steps.length > 0 && (
                            <div className="activity-trace">
                              <button
                                type="button"
                                className="activity-trace-toggle"
                                onClick={() =>
                                  setExpandedActivityKey((prev) => ({
                                    ...prev,
                                    [stepKey]: !isOpen,
                                  }))
                                }
                              >
                                <span>
                                  {steps.length} step{steps.length > 1 ? "s" : ""} ·{" "}
                                  {steps[steps.length - 1]}
                                </span>
                                <span className="activity-chevron">{isOpen ? "⌄" : "›"}</span>
                              </button>
                              {isOpen && (
                                <div className="activity-step-list">
                                  {steps.map((step, sIdx) => (
                                    <div key={sIdx} className="activity-step-row">
                                      {step}
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>
                          )}
                          <MessageText text={msg.text} />
                        </div>
                      )}
                    </div>
                  );
                })}
                {isTargetWorking && (
                  <div className="live-activity-card">
                    <button
                      type="button"
                      className="activity-trace-toggle live"
                      onClick={() => setLiveStepsExpanded((prev) => !prev)}
                    >
                      <span className="live-pulse-dot" />
                      <span>
                        {activeEvent?.summary ?? "Working…"}{" "}
                        {targetRecentSteps.length > 0
                          ? `· ${targetRecentSteps.length} steps`
                          : ""}
                      </span>
                      <span className="activity-chevron">
                        {liveStepsExpanded ? "⌄" : "›"}
                      </span>
                    </button>
                    {liveStepsExpanded && targetRecentSteps.length > 0 && (
                      <div className="activity-step-list">
                        {targetRecentSteps.map((ev) => (
                          <div key={ev.id} className="activity-step-row">
                            <span>{ev.summary}</span>
                            {ev.detail && (
                              <span className="activity-step-detail">{ev.detail}</span>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            ) : (
              <div className="chat-empty-state">
                <FilledOrbIcon size={20} />
                <div className="chat-empty-title">
                  {selectedConversationMeta?.title ?? "Antigravity Floating Dock"}
                </div>
                <div className="hint-text">
                  {transcriptError ??
                    "Select a session above or send a message below to chat with Antigravity."}
                </div>
                {conversations.length > 0 && (
                  <div className="btn-row" style={{ justifyContent: "center", marginTop: 6 }}>
                    {conversations.slice(0, 4).map((c) => (
                      <button
                        key={c.id}
                        type="button"
                        className="btn"
                        onClick={() => void handleSelectSession(c.id)}
                      >
                        <span>{c.title}</span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>

          <footer className="composer-area">
            {queuedMessages.length > 0 && (
              <div className="queue-panel">
                <div className="queue-header">
                  <span>Pending Queue ({queuedMessages.length})</span>
                  <button
                    type="button"
                    className="queue-clear-btn"
                    onClick={() => void clearQueue()}
                  >
                    Clear
                  </button>
                </div>
                <div className="queue-list">
                  {queuedMessages.map((q) => (
                    <div key={q.id} className={`queue-row ${q.status}`}>
                      <div className="queue-copy">
                        <span className="queue-preview">{q.preview}</span>
                        {q.error && <span className="queue-err">{q.error}</span>}
                      </div>
                      <div className="queue-actions">
                        {q.status !== "sending" && (
                          <button
                            type="button"
                            className="icon-btn"
                            onClick={() => void flushQueueItem(q.id)}
                            title="Send now"
                            aria-label="Send queued message now"
                          >
                            <FilledSendIcon size={12} />
                          </button>
                        )}
                        {q.status !== "sending" && (
                          <button
                            type="button"
                            className="icon-btn"
                            onClick={() => void removeQueueItem(q.id)}
                            title="Remove from queue"
                            aria-label="Remove from queue"
                          >
                            <FilledCloseIcon size={12} />
                          </button>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
            <div className="composer-box">
              {attachments.length > 0 && (
                <div className="attachment-strip">
                  {attachments.map((img) => (
                    <div key={img.path} className="attachment-chip">
                      <img src={img.previewUrl} alt={img.name} className="attachment-thumb" />
                      <span className="attachment-name">{img.name}</span>
                      <button
                        type="button"
                        className="icon-btn"
                        onClick={() =>
                          setAttachments((prev) => prev.filter((item) => item.path !== img.path))
                        }
                        aria-label={`Remove ${img.name}`}
                      >
                        <FilledCloseIcon size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <textarea
                className="composer-input"
                placeholder="Ask anything, @ to mention, ⌘+V to paste image…"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={handleComposerKeyDown}
                onPaste={handleComposerPaste}
                disabled={sending}
              />
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                hidden
                onChange={(e) => {
                  if (e.target.files) {
                    void handleAddImageFiles(e.target.files);
                  }
                  e.target.value = "";
                }}
              />
              <div className="composer-footer">
                <div className="btn-row">
                  <button
                    type="button"
                    className="composer-tool-btn"
                    disabled={sending || attachments.length >= 4}
                    onClick={() => fileInputRef.current?.click()}
                    title="Attach image (or paste with ⌘+V)"
                  >
                    <FilledImageIcon size={14} />
                    <span>Image</span>
                  </button>
                  <button
                    type="button"
                    className="composer-tool-btn"
                    disabled={(!draft.trim() && attachments.length === 0) || sending}
                    onClick={() => void handleCopyDraft(false)}
                    title="Copy draft"
                  >
                    <FilledCopyIcon size={13} />
                    <span>Copy</span>
                  </button>
                  <button
                    type="button"
                    className="composer-tool-btn"
                    onClick={() => void handleCopyDraft(true)}
                    title="Open Antigravity"
                  >
                    <FilledExternalIcon size={13} />
                    <span>
                      {draft.trim() || attachments.length > 0 ? "Copy & Open" : "Open AGY"}
                    </span>
                  </button>
                </div>
                <button
                  type="button"
                  className="btn primary composer-send-btn"
                  disabled={
                    (!draft.trim() && attachments.length === 0) ||
                    sending ||
                    !status.antigravityRunning
                  }
                  onClick={() => void handleSendMessage()}
                >
                  <FilledSendIcon size={13} />
                  <span>
                    {sending
                      ? "Sending…"
                      : activeEvent?.activityState === "working" || queuedMessages.length > 0
                        ? "Queue"
                        : "Send"}
                  </span>
                </button>
              </div>
            </div>
          </footer>
        </>
        ) : (
          <div className="chat-body">
            {actionNotice && (
              <div className="alert-banner info">
                <span>{actionNotice}</span>
                <button type="button" className="icon-btn" onClick={() => setActionNotice(null)}>
                  <FilledCloseIcon size={13} />
                </button>
              </div>
            )}

            <section className="card">
              <div className="card-title-row">
                <span className="card-label">Liquid Glass</span>
                <button
                  type="button"
                  className="btn"
                  onClick={() =>
                    void persistSettings(
                      {
                        ...settingsForm,
                        glassStyle: "regular",
                        glassTintOpacity: 0.42,
                        cardOpacity: 0.50,
                        glassBlurPx: 12,
                      },
                      "Reset Liquid Glass defaults.",
                    )
                  }
                >
                  <span>Reset</span>
                </button>
              </div>

              <div className="segmented-row">
                {(
                  [
                    { id: "clear", label: "Clear" },
                    { id: "regular", label: "Regular" },
                    { id: "vibrancy", label: "Classic Blur" },
                  ] as const
                ).map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    className={`seg-btn ${settingsForm.glassStyle === opt.id ? "active" : ""}`}
                    onClick={() =>
                      void persistSettings({ ...settingsForm, glassStyle: opt.id })
                    }
                  >
                    {opt.label}
                  </button>
                ))}
              </div>

              <div className="slider-row">
                <div className="slider-header">
                  <span>Surface Tint</span>
                  <span>{Math.round(settingsForm.glassTintOpacity * 100)}%</span>
                </div>
                <input
                  type="range"
                  min={0}
                  max={85}
                  value={Math.round(settingsForm.glassTintOpacity * 100)}
                  className="range-input"
                  onChange={(e) => {
                    const next = {
                      ...settingsForm,
                      glassTintOpacity: Number(e.target.value) / 100,
                    };
                    applyGlassPreview(next);
                  }}
                  onPointerUp={() => void persistSettings(settingsForm)}
                  onKeyUp={() => void persistSettings(settingsForm)}
                />
              </div>

              <div className="slider-row">
                <div className="slider-header">
                  <span>Card Opacity</span>
                  <span>{Math.round(settingsForm.cardOpacity * 100)}%</span>
                </div>
                <input
                  type="range"
                  min={5}
                  max={90}
                  value={Math.round(settingsForm.cardOpacity * 100)}
                  className="range-input"
                  onChange={(e) => {
                    const next = {
                      ...settingsForm,
                      cardOpacity: Number(e.target.value) / 100,
                    };
                    applyGlassPreview(next);
                  }}
                  onPointerUp={() => void persistSettings(settingsForm)}
                  onKeyUp={() => void persistSettings(settingsForm)}
                />
              </div>

              <div className="slider-row">
                <div className="slider-header">
                  <span>Backdrop Blur</span>
                  <span>{settingsForm.glassBlurPx}px</span>
                </div>
                <input
                  type="range"
                  min={0}
                  max={32}
                  value={settingsForm.glassBlurPx}
                  className="range-input"
                  onChange={(e) => {
                    const next = {
                      ...settingsForm,
                      glassBlurPx: Number(e.target.value),
                    };
                    applyGlassPreview(next);
                  }}
                  onPointerUp={() => void persistSettings(settingsForm)}
                  onKeyUp={() => void persistSettings(settingsForm)}
                />
              </div>
            </section>

            <section className="card">
              <div className="card-title-row">
                <span className="card-label">Antigravity Bridge</span>
                <span className="badge">
                  {status.sidecarConnected ? (
                    <>
                      <FilledCheckCircleIcon size={12} />
                      <span style={{ marginLeft: 4 }}>Connected</span>
                    </>
                  ) : hookStatus?.hookInstalled ? (
                    "Restart AGY"
                  ) : (
                    "Not installed"
                  )}
                </span>
              </div>
              {hookStatus?.error && <div className="alert-banner error">{hookStatus.error}</div>}
              <div className="btn-row">
                <button
                  type="button"
                  className="btn primary"
                  disabled={hookBusy}
                  onClick={() => void handleInstallHooks()}
                >
                  <FilledShieldCheckIcon size={13} />
                  <span>{hookStatus?.hookInstalled ? "Update Bridge" : "Install Bridge"}</span>
                </button>
                <button
                  type="button"
                  className="btn danger"
                  disabled={hookBusy || !hookStatus?.hookInstalled}
                  onClick={() => void handleUninstallHooks()}
                >
                  <FilledTrashIcon size={13} />
                  <span>Uninstall</span>
                </button>
              </div>
            </section>

            <section className="card">
              <div className="card-title-row">
                <span className="card-label">Shortcuts</span>
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="chat-shortcut">
                  Toggle Floating Chat
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
                <button type="button" className="btn danger" onClick={() => void quitApp()}>
                  <span>Quit App</span>
                </button>
              </div>
            </section>
          </div>
        )}
    </div>
  );
}
