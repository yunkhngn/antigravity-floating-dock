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
  glassStyle: string;
  glassTintOpacity: number;
  cardOpacity: number;
  glassBlurPx: number;
}

export interface ConnectionStatus {
  antigravityRunning: boolean;
  hookInstalled: boolean;
  relayInstalled: boolean;
  sidecarInstalled: boolean;
  sidecarConnected: boolean;
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
  steps?: string[];
}

export interface SessionCapabilities {
  sourceProduct: string;
  canSendMessage: boolean;
  limitationReason: string;
  supportsOpenInAntigravity: boolean;
  supportsCopyFallback: boolean;
}

export interface ConversationSummary {
  id: string;
  title: string;
  runState: string;
  updateTime: string;
  projectId: string | null;
}

export interface AttachmentInfo {
  path: string;
  name: string;
  sizeBytes: number;
}

export type QueuedMessageStatus = "pending" | "sending" | "failed";

export interface QueuedMessage {
  id: string;
  conversationId: string | null;
  conversationTitle: string | null;
  workspacePath: string | null;
  sourceProduct: string | null;
  preview: string;
  createdAtMs: number;
  status: QueuedMessageStatus;
  error: string | null;
}

export interface DockSnapshot {
  status: ConnectionStatus;
  settings: DockSettings;
  activeEvent: DockEvent | null;
  recentEvents: DockEvent[];
  selectedConversationId: string | null;
  conversations: ConversationSummary[];
  queuedMessages: QueuedMessage[];
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
  sidecarInstalled: boolean;
  sidecarConnected: boolean;
  snippet: string;
  error: string | null;
}

export interface HookMutationResult {
  status: HookStatus;
  backupPath: string | null;
  message: string;
}

export type SendOutcomeStatus =
  | "Success"
  | "Queued"
  | "AppNotRunning"
  | "PermissionDenied"
  | "ComposerNotFound"
  | "ConversationMismatch"
  | "SubmissionFailed"
  | "TargetAmbiguous";

export interface PermissionStatus {
  accessibilityTrusted: boolean;
  inputMonitoringNeeded: boolean;
  instructions: string;
}

export interface DesktopSendRequest {
  message: string;
  expectedConversationId: string | null;
  expectedConversationTitle: string | null;
  expectedWorkspacePath: string | null;
  sourceProduct: string | null;
  idempotencyKey: string | null;
  allowKeyboardFallback: boolean;
  allowCoordinateFallback: boolean;
}

export interface DesktopSendResult {
  status: SendOutcomeStatus;
  strategyUsed: string;
  message: string;
  verifiedSubmitted: boolean;
  targetWindowTitle: string | null;
  permissionStatus: PermissionStatus;
}

export interface AxElementSummary {
  role: string;
  title: string | null;
  description: string | null;
  placeholder: string | null;
  enabled: boolean | null;
}

export interface AxUiInspection {
  accessibilityTrusted: boolean;
  runningPids: number[];
  windowTitles: string[];
  interestingElements: AxElementSummary[];
}
