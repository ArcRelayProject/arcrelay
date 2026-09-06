import type {
  ClipboardImageOcr,
  ClipboardOcrBlock,
  ClipboardOcrCharacter,
  ClipboardOcrPoint,
} from "./types";

export interface SelectableImageCharacter extends ClipboardOcrCharacter {
  blockIndex: number;
  characterIndex: number;
}

export interface ImageTextSelectionBand {
  blockIndex: number;
  start: number;
  end: number;
  points: ClipboardOcrCharacter["points"];
}

const lerpPoint = (start: ClipboardOcrPoint, end: ClipboardOcrPoint, position: number) => ({
  x: start.x + (end.x - start.x) * position,
  y: start.y + (end.y - start.y) * position,
});

function blockPoints(block: ClipboardOcrBlock): ClipboardOcrCharacter["points"] {
  if (block.points) return block.points;
  const right = block.left + block.width;
  const bottom = block.top + block.height;
  return [
    { x: block.left, y: block.top },
    { x: right, y: block.top },
    { x: right, y: bottom },
    { x: block.left, y: bottom },
  ];
}

function interpolatedCharacters(block: ClipboardOcrBlock): ClipboardOcrCharacter[] {
  const characters = Array.from(block.text);
  if (!characters.length) return [];
  const [startTop, endTop, endBottom, startBottom] = blockPoints(block);
  return characters.map((text, index) => {
    const start = index / characters.length;
    const end = (index + 1) / characters.length;
    return {
      text,
      confidence: block.confidence,
      points: [
        lerpPoint(startTop, endTop, start),
        lerpPoint(startTop, endTop, end),
        lerpPoint(startBottom, endBottom, end),
        lerpPoint(startBottom, endBottom, start),
      ],
    };
  });
}

export function selectableImageCharacters(ocr: ClipboardImageOcr): SelectableImageCharacter[] {
  return ocr.blocks.flatMap((block, blockIndex) => {
    const aligned = block.characters?.length ? block.characters : interpolatedCharacters(block);
    return aligned.map((character, characterIndex) => ({
      ...character,
      blockIndex,
      characterIndex,
    }));
  });
}

function pointInPolygon(point: ClipboardOcrPoint, polygon: ClipboardOcrPoint[]) {
  let inside = false;
  for (let index = 0, previous = polygon.length - 1; index < polygon.length; previous = index++) {
    const currentPoint = polygon[index];
    const previousPoint = polygon[previous];
    const crosses = (currentPoint.y > point.y) !== (previousPoint.y > point.y)
      && point.x < ((previousPoint.x - currentPoint.x) * (point.y - currentPoint.y))
        / (previousPoint.y - currentPoint.y) + currentPoint.x;
    if (crosses) inside = !inside;
  }
  return inside;
}

function distanceToSegment(
  point: ClipboardOcrPoint,
  start: ClipboardOcrPoint,
  end: ClipboardOcrPoint,
) {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const lengthSquared = dx * dx + dy * dy;
  const position = lengthSquared === 0
    ? 0
    : Math.max(0, Math.min(1, ((point.x - start.x) * dx + (point.y - start.y) * dy) / lengthSquared));
  const nearestX = start.x + position * dx;
  const nearestY = start.y + position * dy;
  return Math.hypot(point.x - nearestX, point.y - nearestY);
}

function distanceToPolygon(point: ClipboardOcrPoint, polygon: ClipboardOcrPoint[]) {
  if (pointInPolygon(point, polygon)) return 0;
  return Math.min(...polygon.map((start, index) => (
    distanceToSegment(point, start, polygon[(index + 1) % polygon.length])
  )));
}

export function hitTestImageCharacter(
  characters: SelectableImageCharacter[],
  point: ClipboardOcrPoint,
  tolerance = 0,
): number | null {
  let bestIndex: number | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  characters.forEach((character, index) => {
    const distance = distanceToPolygon(point, character.points);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestDistance <= tolerance ? bestIndex : null;
}

export function imageTextBoundaryAtPoint(
  characters: SelectableImageCharacter[],
  point: ClipboardOcrPoint,
  tolerance = 0,
): number | null {
  const index = hitTestImageCharacter(characters, point, tolerance);
  if (index === null) return null;

  const [startTop, endTop, endBottom, startBottom] = characters[index].points;
  const start = {
    x: (startTop.x + startBottom.x) / 2,
    y: (startTop.y + startBottom.y) / 2,
  };
  const end = {
    x: (endTop.x + endBottom.x) / 2,
    y: (endTop.y + endBottom.y) / 2,
  };
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const lengthSquared = dx * dx + dy * dy;
  const position = lengthSquared === 0
    ? 0
    : ((point.x - start.x) * dx + (point.y - start.y) * dy) / lengthSquared;
  return position < 0.5 ? index : index + 1;
}

export function orderedSelection(anchor: number, focus: number): [number, number] {
  return anchor <= focus ? [anchor, focus] : [focus, anchor];
}

export function imageTextSelectionRange(
  anchor: number | null,
  focus: number | null,
): [number, number] | null {
  if (anchor === null || focus === null || anchor === focus) return null;
  return orderedSelection(anchor, focus);
}

export function imageTextSelectionBands(
  characters: SelectableImageCharacter[],
  anchor: number | null,
  focus: number | null,
): ImageTextSelectionBand[] {
  const range = imageTextSelectionRange(anchor, focus);
  if (!range) return [];

  const [start, end] = range;
  const bands: ImageTextSelectionBand[] = [];
  let currentStart = start;
  for (let index = start + 1; index <= end; index += 1) {
    const blockChanged = index === end
      || characters[index]?.blockIndex !== characters[currentStart]?.blockIndex;
    if (!blockChanged) continue;

    const first = characters[currentStart];
    const last = characters[index - 1];
    if (first && last) {
      bands.push({
        blockIndex: first.blockIndex,
        start: currentStart,
        end: index,
        points: [first.points[0], last.points[1], last.points[2], first.points[3]],
      });
    }
    currentStart = index;
  }
  return bands;
}

export function imageTextLineRange(
  characters: SelectableImageCharacter[],
  index: number,
): [number, number] {
  const blockIndex = characters[index]?.blockIndex;
  if (blockIndex === undefined) return [index, index];
  let start = index;
  let end = index + 1;
  while (start > 0 && characters[start - 1].blockIndex === blockIndex) start -= 1;
  while (end < characters.length && characters[end].blockIndex === blockIndex) end += 1;
  return [start, end];
}

export function imageTextWordRange(
  characters: SelectableImageCharacter[],
  index: number,
  locale?: string,
): [number, number] {
  const [lineStart, lineEnd] = imageTextLineRange(characters, index);
  const line = characters.slice(lineStart, lineEnd);
  const text = line.map((character) => character.text).join("");
  const targetOffset = line
    .slice(0, Math.max(0, index - lineStart))
    .reduce((length, character) => length + character.text.length, 0);
  const segmenter = new Intl.Segmenter(locale, { granularity: "word" });
  const segment = [...segmenter.segment(text)].find((candidate) => (
    candidate.index <= targetOffset
      && targetOffset < candidate.index + candidate.segment.length
  ));
  if (!segment?.isWordLike) return [index, index + 1];

  const segmentEnd = segment.index + segment.segment.length;
  let offset = 0;
  let start = lineStart;
  let end = lineEnd;
  for (let lineIndex = 0; lineIndex < line.length; lineIndex += 1) {
    const nextOffset = offset + line[lineIndex].text.length;
    if (offset <= segment.index && segment.index < nextOffset) start = lineStart + lineIndex;
    if (offset < segmentEnd && segmentEnd <= nextOffset) {
      end = lineStart + lineIndex + 1;
      break;
    }
    offset = nextOffset;
  }
  return [start, end];
}

export function selectedImageText(
  characters: SelectableImageCharacter[],
  anchor: number | null,
  focus: number | null,
) {
  const range = imageTextSelectionRange(anchor, focus);
  if (!range) return "";
  const [start, end] = range;
  let text = "";
  let previousBlock: number | null = null;
  for (const character of characters.slice(start, end)) {
    if (previousBlock !== null && character.blockIndex !== previousBlock) text += "\n";
    text += character.text;
    previousBlock = character.blockIndex;
  }
  return text;
}

export function polygonPoints(points: ClipboardOcrPoint[]) {
  return points.map((point) => `${point.x},${point.y}`).join(" ");
}
