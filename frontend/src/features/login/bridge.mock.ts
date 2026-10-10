// Browser preview fixtures. This adapter is never imported by the native credential service.
import type {
  LoginRequest,
  LoginResponse,
  LoginSummary,
  LoginVaultStatus,
} from "../../ipc/generated";
let status: LoginVaultStatus = {
  sessionVersion: 1,
  configured: true,
  unlocked: true,
  expiresInSeconds: 300,
  vaultId: "preview-only",
  error: null,
  settings: { unlockSeconds: 300, lockOnClose: false, clearSeconds: 30 },
};
let enabled = true;
let entries: LoginSummary[] = [
  {
    id: "preview-github",
    title: "GitHub · 工作账号",
    address: "github.com",
    username: "alex@acme.design",
    hasPassword: true,
    hasTotp: true,
    totpAlgorithm: "SHA1",
    totpDigits: 6,
    totpPeriod: 30,
    tags: ["工作"],
    favorite: true,
    apps: [{ id: "com.google.Chrome", name: "Google Chrome", enabled: true, priority: 80 }],
    matched: true,
    priority: 80,
    lastUsedAtMs: 0,
  },
  {
    id: "preview-figma",
    title: "Figma",
    address: "figma.com",
    username: "alex@acme.design",
    hasPassword: true,
    hasTotp: false,
    totpAlgorithm: null,
    totpDigits: null,
    totpPeriod: null,
    tags: ["工作"],
    favorite: false,
    apps: [],
    matched: false,
    priority: 0,
    lastUsedAtMs: 0,
  },
  {
    id: "preview-google",
    title: "Google · 个人账号",
    address: "accounts.google.com",
    username: "alex@gmail.com",
    hasPassword: true,
    hasTotp: true,
    totpAlgorithm: "SHA1",
    totpDigits: 6,
    totpPeriod: 30,
    tags: ["个人"],
    favorite: false,
    apps: [],
    matched: false,
    priority: 0,
    lastUsedAtMs: 0,
  },
];
export async function request(req: LoginRequest): Promise<LoginResponse> {
  const result: LoginResponse = {
    status: null,
    context: null,
    entries: null,
    tags: null,
    otp: null,
    value: null,
    preview: null,
    token: null,
    count: null,
    file: null,
    application: null,
    deviceAvailable: true,
    deviceEnabled: enabled,
  };
  if (req.type === "copyDraft") {
    /* In-memory preview does not write the system clipboard. */
  } else if (req.type === "pickApp")
    result.application = { id: "com.apple.Safari", name: "Safari", enabled: true, priority: 80 };
  else if (req.type === "openSettings") {
    window.open("/?page=settings&settingsTab=clipboard&login-security=1", "_blank");
  } else if (req.type === "lock") {
    status.unlocked = false;
    status.sessionVersion++;
  } else if (
    req.type === "unlock" ||
    req.type === "initialize" ||
    req.type === "unlockDevice" ||
    req.type === "recover"
  ) {
    status.configured = true;
    status.unlocked = true;
    status.sessionVersion++;
  } else if (req.type === "context")
    result.context = { id: "com.google.Chrome", name: "Google Chrome", token: "preview-target" };
  else if (!status.unlocked && req.type !== "status")
    throw new Error("Login information is locked; unlock first");
  else if (req.type === "list")
    result.entries = entries
      .filter(
        (e) =>
          (!req.tag || e.tags.includes(req.tag)) &&
          [e.title, e.address, e.username].some((v) =>
            v.toLowerCase().includes(req.search.toLowerCase()),
          ),
      )
      .map((e) => ({ ...e }));
  else if (req.type === "otp")
    result.otp = { code: "482916", expiresAtMs: (Math.floor(Date.now() / 30000) + 1) * 30000 };
  else if (req.type === "reveal") result.value = "preview-password";
  else if (req.type === "save") {
    const d = req.draft;
    const old = entries.find((e) => e.id === d.id);
    const id = d.id ?? crypto.randomUUID();
    const row: LoginSummary = {
      id,
      title: d.title,
      address: d.address,
      username: d.username,
      hasPassword: d.password === null ? (old?.hasPassword ?? false) : !!d.password,
      hasTotp: d.keepTotp ? (old?.hasTotp ?? false) : !!d.totp,
      totpAlgorithm: d.totp?.algorithm ?? old?.totpAlgorithm ?? null,
      totpDigits: d.totp?.digits ?? old?.totpDigits ?? null,
      totpPeriod: d.totp?.period ?? old?.totpPeriod ?? null,
      tags: d.tags,
      favorite: d.favorite,
      apps: d.apps,
      matched: d.apps.some((a) => a.enabled && a.id === "com.google.Chrome"),
      priority: 80,
      lastUsedAtMs: 0,
    };
    entries = [...entries.filter((e) => e.id !== id), row];
    result.value = id;
  } else if (req.type === "remove") entries = entries.filter((e) => e.id !== req.id);
  else if (req.type === "settings") status.settings = req.settings;
  else if (req.type === "device") enabled = req.enabled;
  else if (req.type === "changePassword") enabled = false;
  else if (req.type === "export") result.file = "ArcRelay-logins.arclogin";
  else if (req.type === "previewRestore") {
    result.preview = { count: entries.length, titles: entries.map((e) => e.title) };
    result.token = "preview-restore";
  } else if (req.type === "restore") result.count = entries.length;
  result.tags = [...new Set(entries.flatMap((e) => e.tags))];
  result.status = structuredClone(status);
  result.deviceEnabled = enabled;
  return result;
}
