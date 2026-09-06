import { invoke } from './ipc/client';

export type FrontendLogLevel = "debug" | "info" | "warn" | "error";

type FrontendLogEntry = {
  level: FrontendLogLevel;
  event: string;
  detail: string;
  stack?: string;
};

const recentErrors = new Map<string, number>();
const minimumRepeatIntervalMs = 2_000;
let installed = false;

const inTauri = () => "__TAURI_INTERNALS__" in window;

function errorParts(error: unknown) {
  if (error instanceof Error) {
    return { detail: error.message || error.name, stack: error.stack ?? "" };
  }
  if (typeof error === "string") return { detail: error, stack: "" };
  try {
    return { detail: JSON.stringify(error), stack: "" };
  } catch {
    return { detail: String(error), stack: "" };
  }
}

function shouldSend(entry: FrontendLogEntry) {
  if (entry.level !== "error" && entry.level !== "warn") return true;
  const signature = `${entry.level}:${entry.event}:${entry.detail.slice(0, 200)}`;
  const now = Date.now();
  const previous = recentErrors.get(signature) ?? 0;
  recentErrors.set(signature, now);
  for (const [key, timestamp] of recentErrors) {
    if (now - timestamp > 60_000) recentErrors.delete(key);
  }
  return now - previous >= minimumRepeatIntervalMs;
}

export async function logFrontendEvent(entry: FrontendLogEntry) {
  if (!inTauri() || !shouldSend(entry)) return;
  try {
    await invoke("frontend_log", { entry: { ...entry, stack: entry.stack ?? null } });
  } catch {
    // Logging failures must never create another unhandled rejection.
  }
}

export async function observeFrontendOperation<T>(
  event: string,
  operation: () => Promise<T>,
): Promise<T> {
  const started = performance.now();
  try {
    const result = await operation();
    void logFrontendEvent({
      level: "info",
      event: `${event}.completed`,
      detail: `duration_ms=${Math.round(performance.now() - started)}`,
    });
    return result;
  } catch (error) {
    const parts = errorParts(error);
    void logFrontendEvent({
      level: "error",
      event: `${event}.failed`,
      detail: `${parts.detail}; duration_ms=${Math.round(performance.now() - started)}`,
      stack: parts.stack,
    });
    throw error;
  }
}

export function installFrontendLogging() {
  if (installed) return;
  installed = true;
  window.addEventListener("error", (event) => {
    const parts = errorParts(event.error ?? event.message);
    void logFrontendEvent({
      level: "error",
      event: "frontend.window_error",
      detail: parts.detail,
      stack: parts.stack,
    });
  });
  window.addEventListener("unhandledrejection", (event) => {
    const parts = errorParts(event.reason);
    void logFrontendEvent({
      level: "error",
      event: "frontend.unhandled_rejection",
      detail: parts.detail,
      stack: parts.stack,
    });
  });
  void logFrontendEvent({
    level: "info",
    event: "frontend.started",
    detail: `document=${document.documentElement.lang || "unset"}`,
  });
}
