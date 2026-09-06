import type { TextSliceModel } from "./textSegments";
/** Static browser placeholder; the desktop host supplies real Unicode boundaries. */
export function mockModel(text: string): TextSliceModel {
  const levels = Object.fromEntries(["document", "block", "sentence", "phrase", "word"].map((level) =>
    [level, [{ id: `${level}-preview`, level, start: 0, end: text.length, text }]])) as TextSliceModel["levels"];
  return { text, version: "preview", levels, truncated: false };
}
