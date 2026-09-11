export const languages = ["zhCn", "enUs", "jaJp", "koKr", "deDe", "frFr", "esEs", "ptBr"] as const;
export const themes = ["light", "dark"] as const;

export type VisualScene = {
  id: string;
  feature: string;
  surface: "main" | "clipboard" | "permission-guide" | "tray-transfer" | "privacy-overlay";
  path: string;
  width: number;
  height: number;
};

const main = (id: string, feature: string, query: string): VisualScene => ({
  id,
  feature,
  surface: "main",
  path: `/?${query}`,
  width: 1180,
  height: 760,
});

export const scenes: VisualScene[] = [
  main("actions.default", "actions", "page=actions"),
  main("notifications.default", "notifications", "page=notifications"),
  main("automation.default", "automation", "page=automations"),
  main("privacy.default", "privacy", "page=privacy"),
  main("transfer.default", "transfer", "page=transfer"),
  main("files.default", "files", "page=files"),
  main("print.default", "print", "page=printers"),
  main("input.overview", "input", "page=input&inputSection=overview"),
  main("input.workspace", "input", "page=input&inputSection=workspace"),
  main("input.keyboard", "input", "page=input&inputSection=keyboard"),
  main("input.diagnostics", "input", "page=input&inputSection=diagnostics"),
  main("input.onboarding", "input", "page=input&inputScenario=onboarding"),
  ...["connection", "files", "clipboard", "screenshot", "notifications", "general", "agent"].map((tab) =>
    main(`settings.${tab}`, "settings", `page=settings&settingsTab=${tab}`)),
  {
    id: "clipboard.default", feature: "clipboard", surface: "clipboard",
    path: "/clipboard.html", width: 540, height: 820,
  },
  {
    id: "clipboard.multi-select", feature: "clipboard", surface: "clipboard",
    path: "/clipboard.html?preview-multi-select=1", width: 540, height: 820,
  },
  {
    id: "clipboard.labels", feature: "clipboard", surface: "clipboard",
    path: "/clipboard.html?preview-dialog=labels", width: 540, height: 820,
  },
  {
    id: "clipboard.nearby", feature: "clipboard", surface: "clipboard",
    path: "/clipboard.html?preview-nearby=1", width: 540, height: 820,
  },
  {
    id: "permission-guide.missing", feature: "permission-guide", surface: "permission-guide",
    path: "/permission-guide.html?preview=missing", width: 430, height: 800,
  },
  {
    id: "permission-guide.settings", feature: "permission-guide", surface: "permission-guide",
    path: "/permission-guide.html?preview=settings", width: 430, height: 800,
  },
  ...[
    ["hover", 220], ["empty", 220], ["waiting", 520], ["sending", 520], ["complete", 520], ["failed", 520],
  ].map(([state, height]) => ({
    id: `tray-transfer.${state}`,
    feature: "tray-transfer",
    surface: "tray-transfer" as const,
    path: `/tray-transfer.html?state=${state}`,
    width: 440,
    height: height as number,
  })),
  {
    id: "privacy-overlay.frosted", feature: "privacy-overlay", surface: "privacy-overlay",
    path: "/privacy-overlay.html?style=frosted", width: 1180, height: 760,
  },
  {
    id: "privacy-overlay.solid", feature: "privacy-overlay", surface: "privacy-overlay",
    path: "/privacy-overlay.html?style=solid", width: 1180, height: 760,
  },
];
