import type { DisplaySurface } from "../../types.ts";
export type DeskBounds = { x: number; y: number; width: number; height: number };
export function workspaceBounds(displays: DisplaySurface[]): DeskBounds {
  if (!displays.length) return { x: 0, y: 0, width: 1, height: 1 };
  const x = Math.min(...displays.map((d) => d.deskRectUm.x));
  const y = Math.min(...displays.map((d) => d.deskRectUm.y));
  return {
    x,
    y,
    width: Math.max(1, Math.max(...displays.map((d) => d.deskRectUm.x + d.deskRectUm.width)) - x),
    height: Math.max(1, Math.max(...displays.map((d) => d.deskRectUm.y + d.deskRectUm.height)) - y),
  };
}
export function workspaceProjection(
  bounds: DeskBounds,
  width: number,
  height: number,
  zoom = 1,
  pan = { x: 0, y: 0 },
) {
  const padding = Math.min(72, width * 0.12, height * 0.15);
  const scale = Math.max(
    Number.EPSILON,
    Math.min((width - padding * 2) / bounds.width, (height - padding * 2) / bounds.height) * zoom,
  );
  return {
    scale,
    x: (width - bounds.width * scale) / 2 - bounds.x * scale + pan.x,
    y: (height - bounds.height * scale) / 2 - bounds.y * scale + pan.y,
  };
}
