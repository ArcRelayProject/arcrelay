import { Change, diff } from "@codemirror/merge";
/** Whitespace comparison changes display coordinates only; the document is never normalized. */
export function whitespaceDiff(a: string, b: string): readonly Change[] {
  const strip = (source: string) => {
    const positions: number[] = [];
    let text = "";
    for (let i = 0; i < source.length; i++)
      if (source[i] !== " " && source[i] !== "\t") {
        positions.push(i);
        text += source[i];
      }
    positions.push(source.length);
    return { text, positions };
  };
  const left = strip(a),
    right = strip(b);
  return diff(left.text, right.text, { scanLimit: 1000, timeout: 50 }).map(
    (change) =>
      new Change(
        left.positions[change.fromA],
        left.positions[change.toA],
        right.positions[change.fromB],
        right.positions[change.toB],
      ),
  );
}
