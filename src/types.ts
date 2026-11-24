export type DockEventType =
  | "sessionStart"
  | "preInvocation"
  | "postToolUse"
  | "postInvocation"
  | "stop"
  | "unknown";

export type AgentActivityState =
  | "idle"
  | "working"
  | "needsAttention"
  | "error"
  | "unknown";

export interface DockEvent {
  id: string;
  eventType: DockEventType;
  timestampMs: number;
  conversationId: string | null;
  workspacePath: string | null;
  modelName: string | null;
  summary: string;
  detail: string | null;
  activityState: AgentActivityState;
  terminationReason: string | null;
  transcriptPath: string | null;
  sourceProduct: string;
  dedupeKey: string;
  redactedRaw: unknown;
}

export interface DockSettings {
  toggleChatShortcut: string;
  toggleBubbleShortcut: string;
  pinBubble: boolean;
  autoHideIdleMs: number;
  launchAtLoginHintAcknowledged: boolean;
}

export interface ConnectionStatus {
  antigravityRunning: boolean;
  hookInstalled: boolean;
  relayInstalled: boolean;
  ipcListening: boolean;
  ipcSocketPath: string;
  lastEventAtMs: number | null;
  vibrancyMode: string;
  shortcutErrors: string[];
}

export interface TranscriptMessage {
  role: string;
  text: string;
  timestamp: string | null;
}

export interface SessionCapabilities {
  sourceProduct: string;
  canSendMessage: boolean;
  limitationReason: string;
  supportsOpenInAntigravity: boolean;
  supportsCopyFallback: boolean;
}

export interface DockSnapshot {
  status: ConnectionStatus;
  settings: DockSettings;
  activeEvent: DockEvent | null;
  recentEvents: DockEvent[];
  transcriptTail: TranscriptMessage[];
  transcriptError: string | null;
  sessionCapabilities: SessionCapabilities;
  bubblePinned: boolean;
}

export interface HookStatus {
  hooksFilePath: string;
  hooksFileExists: boolean;
  hookInstalled: boolean;
  hookEnabled: boolean;
  relayPath: string;
  relayInstalled: boolean;
  snippet: string;
  error: string | null;
}

export interface HookMutationResult {
  status: HookStatus;
  backupPath: string | null;
  message: string;
}
