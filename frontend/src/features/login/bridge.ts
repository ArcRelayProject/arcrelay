import type { LoginRequest, LoginResponse } from "../../ipc/generated";
import { loginErrorsZh } from "../../locales/login-errors";
const native = () => "__TAURI_INTERNALS__" in window;
function localizeError(error: unknown): string {
  const message = String(error);
  for (const [diagnostic, translation] of Object.entries(loginErrorsZh).sort(
    (a, b) => b[0].length - a[0].length,
  )) {
    if (message.includes(diagnostic)) return translation;
  }
  return message;
}
export async function loginRequest(request: LoginRequest): Promise<LoginResponse> {
  try {
    let response: LoginResponse;
    if (native()) {
      const { invoke } = await import("../../ipc/client");
      response = await invoke("login_request", { request });
    } else {
      const { request: mock } = await import("./bridge.mock");
      response = await mock(request);
    }
    if (response.status?.error) response.status.error = localizeError(response.status.error);
    return response;
  } catch (error) {
    throw localizeError(error);
  }
}
export async function onLoginLocked(handler: () => void): Promise<() => void> {
  if (!native()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("login-vault-locked", handler);
}
export async function loginApps() {
  if (native()) {
    const { invoke } = await import("../../ipc/client");
    return invoke("list_installed_apps", { refresh: false });
  }
  return [
    {
      name: "Google Chrome",
      path: "/Applications/Google Chrome.app",
      identifier: "com.google.Chrome",
      version: null,
      iconDataUrl: null,
    },
    {
      name: "Safari",
      path: "/Applications/Safari.app",
      identifier: "com.apple.Safari",
      version: null,
      iconDataUrl: null,
    },
    {
      name: "Visual Studio Code",
      path: "/Applications/Visual Studio Code.app",
      identifier: "com.microsoft.VSCode",
      version: null,
      iconDataUrl: null,
    },
    {
      name: "Slack",
      path: "/Applications/Slack.app",
      identifier: "com.tinyspeck.slackmacgap",
      version: null,
      iconDataUrl: null,
    },
  ];
}
export function applicationId(app: { identifier: string | null; path: string }): string {
  return /Win/.test(navigator.platform) ? app.path.toLowerCase() : (app.identifier ?? app.path);
}

export async function onLoginContext(
  handler: (context: import("../../ipc/generated").LoginContext) => void,
): Promise<() => void> {
  if (!native()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<import("../../ipc/generated").LoginContext>("login-context-changed", (event) =>
    handler(event.payload),
  );
}
