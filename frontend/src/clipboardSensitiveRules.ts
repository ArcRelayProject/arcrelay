export type KeywordIssue = "empty" | "tooLong" | "control" | "duplicate" | "tooMany";

// Match the host's literal keyword limits and ASCII-only case folding.
export function keywordIssue(
  draft: string,
  keywords: readonly string[],
  editingIndex: number | null,
): KeywordIssue | null {
  const value = draft.trim();
  if (!value) return "empty";
  if (new TextEncoder().encode(value).length > 256) return "tooLong";
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(value)) return "control";
  const fold = (word: string) => word.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
  if (keywords.some((word, index) => index !== editingIndex && fold(word) === fold(value)))
    return "duplicate";
  if (editingIndex === null && keywords.length >= 100) return "tooMany";
  return null;
}
