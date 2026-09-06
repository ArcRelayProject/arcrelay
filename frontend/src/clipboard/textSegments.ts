export const TEXT_SLICE_LEVELS = [
  "document",
  "block",
  "sentence",
  "phrase",
  "word",
] as const;

export type TextSliceLevel = (typeof TEXT_SLICE_LEVELS)[number];

export type TextSlice = import('../ipc/generated').TextSlice;
export type TextSliceModel = import('../ipc/generated').TextSliceModel;

export type TextSlicePart =
  | { kind: "text"; text: string; start: number; end: number }
  | { kind: "slice"; slice: TextSlice; index: number };

type Range = Pick<TextSlice, "start" | "end">;

export function containingSlice(slices: TextSlice[], selection: Range) {
  return (
    slices
      .filter(
        (slice) => slice.start <= selection.start && slice.end >= selection.end,
      )
      .sort(
        (left, right) => left.end - left.start - (right.end - right.start),
      )[0] ?? null
  );
}

export function containedSlice(
  slices: TextSlice[],
  selection: Range,
  focusOffset: number,
) {
  const contained = slices.filter(
    (slice) => slice.start >= selection.start && slice.end <= selection.end,
  );
  return (
    contained.find(
      (slice) => slice.start <= focusOffset && slice.end >= focusOffset,
    ) ??
    contained[0] ??
    null
  );
}

/** Interleaves selectable ranges with untouched source text for an inline picker. */
export function textSliceParts(
  text: string,
  slices: TextSlice[],
): TextSlicePart[] {
  const parts: TextSlicePart[] = [];
  let cursor = 0;
  let index = 0;
  for (const slice of [...slices].sort(
    (left, right) => left.start - right.start || left.end - right.end,
  )) {
    if (slice.start < cursor || slice.end > text.length) continue;
    if (slice.start > cursor) {
      parts.push({
        kind: "text",
        text: text.slice(cursor, slice.start),
        start: cursor,
        end: slice.start,
      });
    }
    parts.push({ kind: "slice", slice, index });
    index += 1;
    cursor = slice.end;
  }
  if (cursor < text.length)
    parts.push({
      kind: "text",
      text: text.slice(cursor),
      start: cursor,
      end: text.length,
    });
  return parts;
}
