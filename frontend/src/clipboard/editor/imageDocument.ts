export type Tool =
  | "select"
  | "pan"
  | "crop"
  | "mosaic"
  | "cover"
  | "brush"
  | "rect"
  | "ellipse"
  | "arrow"
  | "line"
  | "text";
export type Mark = {
  id: string;
  type: Tool;
  x: number;
  y: number;
  width: number;
  height: number;
  points: number[];
  color: string;
  stroke: number;
  text: string;
  fontSize: number;
  fontFamily: string;
  background: string;
  src?: string;
  scaleX?: number;
  scaleY?: number;
  rotation?: number;
};
export type ImageDocument = { base: string; width: number; height: number; marks: Mark[] };
export function boundedRect(
  start: { x: number; y: number },
  end: { x: number; y: number },
  width: number,
  height: number,
  ratio = 0,
) {
  let w = Math.abs(end.x - start.x),
    h = Math.abs(end.y - start.y);
  if (ratio) {
    if (w / Math.max(h, 1) > ratio) h = w / ratio;
    else w = h * ratio;
  }
  const x = Math.max(0, end.x < start.x ? start.x - w : start.x);
  const y = Math.max(0, end.y < start.y ? start.y - h : start.y);
  const factor = Math.min(1, (width - x) / Math.max(w, 1), (height - y) / Math.max(h, 1));
  return {
    x: Math.round(x),
    y: Math.round(y),
    width: Math.max(1, Math.floor(w * factor)),
    height: Math.max(1, Math.floor(h * factor)),
  };
}
/** Shared snapshots reuse the immutable base string; history has both count and pixel-buffer bounds. */
export class ImageHistory {
  states: ImageDocument[];
  index = 0;
  constructor(initial: ImageDocument) {
    this.states = [structuredClone(initial)];
  }
  get current() {
    return structuredClone(this.states[this.index]);
  }
  get canUndo() {
    return this.index > 0;
  }
  get canRedo() {
    return this.index < this.states.length - 1;
  }
  push(next: ImageDocument) {
    this.states = this.states.slice(0, this.index + 1);
    this.states.push(structuredClone(next));
    while (this.states.length > 30 || (this.states.length > 2 && this.bytes() > 64 * 1024 * 1024))
      this.states.shift();
    this.index = this.states.length - 1;
  }
  private bytes() {
    const bases = new Set(this.states.flatMap((s) => [s.base, ...s.marks.map((m) => m.src ?? "")]));
    return [...bases].reduce((sum, s) => sum + s.length * 2, 0);
  }
  undo() {
    if (this.canUndo) this.index--;
    return this.current;
  }
  redo() {
    if (this.canRedo) this.index++;
    return this.current;
  }
}
