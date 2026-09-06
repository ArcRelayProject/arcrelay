type ShortcutKeyEvent = Pick<KeyboardEvent,
  "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "repeat" | "isComposing"
>;

export type ShortcutCapture =
  | { type: "navigate" | "ignore" | "clear" }
  | { type: "shortcut"; value: string }
  | { type: "error"; message: string };

export function captureActionShortcut(event: ShortcutKeyEvent): ShortcutCapture {
  const modified = event.ctrlKey || event.metaKey || event.altKey;
  if (!modified && (event.key === "Tab" || (event.key === "Escape" && !event.shiftKey))) {
    return { type: "navigate" };
  }
  if (event.repeat || event.isComposing || ["Meta", "Control", "Alt", "Shift", "AltGraph"].includes(event.key)) {
    return { type: "ignore" };
  }
  if (!modified && !event.shiftKey && ["Backspace", "Delete"].includes(event.key)) {
    return { type: "clear" };
  }
  if (!modified) {
    return { type: "error", message: "请同时按下 Ctrl、Alt 或 Command / Super 与另一个按键。" };
  }
  // Physical codes preserve shifted punctuation, non-Latin layouts and numpad keys.
  const key = event.code || (event.key === " " ? "Space" : event.key);
  if (!key || ["Unidentified", "Dead", "Process"].includes(key)) {
    return { type: "error", message: "无法识别此按键，请尝试其他组合键。" };
  }
  const modifiers = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.metaKey) modifiers.push("Super");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  return { type: "shortcut", value: [...modifiers, key].join("+") };
}
