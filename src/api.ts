import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DockSettings, DockSnapshot, HookMutationResult, HookStatus } from "./types";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getSnapshot(): Promise<DockSnapshot> {
  return invoke<DockSnapshot>("get_snapshot");
}

export async function showChat(): Promise<void> {
  return invoke("show_chat");
}

export async function hideChat(): Promise<void> {
  return invoke("hide_chat");
}

export async function setChatExpanded(expanded: boolean): Promise<void> {
  return invoke("set_chat_expanded", { expanded });
}

export async function hideBubble(): Promise<void> {
  return invoke("hide_bubble");
}

export async function setBubblePinned(pinned: boolean): Promise<void> {
  return invoke("set_bubble_pinned", { pinned });
}

export async function updateSettings(settings: DockSettings): Promise<DockSnapshot> {
  return invoke<DockSnapshot>("update_settings", { settings });
}

export async function verifyHooks(): Promise<HookStatus> {
  return invoke<HookStatus>("verify_hooks");
}

export async function installHooks(): Promise<HookMutationResult> {
  return invoke<HookMutationResult>("install_hooks");
}

export async function uninstallHooks(): Promise<HookMutationResult> {
  return invoke<HookMutationResult>("uninstall_hooks");
}

export async function openAntigravity(sourceProduct?: string): Promise<void> {
  return invoke("open_antigravity", { sourceProduct: sourceProduct ?? null });
}

export async function emitSampleEvent(kind: "working" | "idle" | "error"): Promise<void> {
  return invoke("emit_sample_event", { kind });
}

export async function getDiagnostics(): Promise<string[]> {
  return invoke<string[]>("get_diagnostics");
}

export async function quitApp(): Promise<void> {
  return invoke("quit_app");
}

export async function subscribeStateUpdates(
  cb: (snapshot: DockSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<DockSnapshot>("dock://state-updated", (event) => {
    cb(event.payload);
  });
}
