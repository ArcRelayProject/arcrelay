import type { DisplaySurface } from "../../types";
const MINIMUM_PORTAL_SPAN_UM = 5_000;
const MINIMUM_DISPLAY_DIMENSION_UM = 80_000;
const MAXIMUM_DISPLAY_DIMENSION_UM = 3_000_000;

export function snapDisplayPosition(
  displayId: string,
  x: number,
  y: number,
  displays: Record<string, DisplaySurface>,
  thresholdUm: number,
) {
  const moving = displays[displayId];
  if (!moving) return { x, y, snapped: false };
  const deltaX = x - moving.deskRectUm.x;
  const deltaY = y - moving.deskRectUm.y;
  const movingGroup = Object.values(displays).filter((display) => display.deviceId === moving.deviceId);
  const stationary = Object.values(displays).filter((display) => display.deviceId !== moving.deviceId);
  const candidates: { x: number; y: number; distance: number; target: string }[] = [];
  for (const member of movingGroup) {
    const memberX = member.deskRectUm.x + deltaX;
    const memberY = member.deskRectUm.y + deltaY;
    for (const other of stationary) {
      const otherRect = other.deskRectUm;
      const horizontalPositions = [
        otherRect.x + otherRect.width,
        otherRect.x - member.deskRectUm.width,
      ];
      for (const candidateX of horizontalPositions) {
        const overlap = intervalOverlap(memberY, member.deskRectUm.height, otherRect.y, otherRect.height);
        const distance = Math.abs(memberX - candidateX);
        if (overlap >= MINIMUM_PORTAL_SPAN_UM && distance <= thresholdUm) {
          candidates.push({ x: x + candidateX - memberX, y, distance, target: other.displayId });
        }
      }
      const verticalPositions = [
        otherRect.y + otherRect.height,
        otherRect.y - member.deskRectUm.height,
      ];
      for (const candidateY of verticalPositions) {
        const overlap = intervalOverlap(memberX, member.deskRectUm.width, otherRect.x, otherRect.width);
        const distance = Math.abs(memberY - candidateY);
        if (overlap >= MINIMUM_PORTAL_SPAN_UM && distance <= thresholdUm) {
          candidates.push({ x, y: y + candidateY - memberY, distance, target: other.displayId });
        }
      }
    }
  }
  candidates.sort((left, right) => left.distance - right.distance || left.target.localeCompare(right.target));
  return candidates[0] ? { x: candidates[0].x, y: candidates[0].y, snapped: true } : { x, y, snapped: false };
}

export function moveDisplayGroup(
  displayId: string,
  x: number,
  y: number,
  displays: Record<string, DisplaySurface>,
) {
  const moving = displays[displayId];
  if (!moving) return;
  const deltaX = x - moving.deskRectUm.x;
  const deltaY = y - moving.deskRectUm.y;
  for (const display of Object.values(displays)) {
    if (display.deviceId !== moving.deviceId) continue;
    display.deskRectUm.x += deltaX;
    display.deskRectUm.y += deltaY;
  }
}

export function resizeDisplay(
  displayId: string,
  requestedWidthUm: number,
  displays: Record<string, DisplaySurface>,
) {
  const display = displays[displayId];
  if (!display || !Number.isFinite(requestedWidthUm) || display.deskRectUm.width <= 0 || display.deskRectUm.height <= 0) return null;

  const minimumScale = Math.max(
    MINIMUM_DISPLAY_DIMENSION_UM / display.deskRectUm.width,
    MINIMUM_DISPLAY_DIMENSION_UM / display.deskRectUm.height,
  );
  const maximumScale = Math.min(
    MAXIMUM_DISPLAY_DIMENSION_UM / display.deskRectUm.width,
    MAXIMUM_DISPLAY_DIMENSION_UM / display.deskRectUm.height,
  );
  const requestedScale = requestedWidthUm / display.deskRectUm.width;
  const scale = Math.min(maximumScale, Math.max(minimumScale, requestedScale));
  const width = Math.round(display.deskRectUm.width * scale / 1_000) * 1_000;
  const height = Math.round(display.deskRectUm.height * scale / 1_000) * 1_000;

  display.deskRectUm.width = width;
  display.deskRectUm.height = height;
  display.physicalSizeUm.width = width;
  display.physicalSizeUm.height = height;
  display.geometryConfidence = "UserCalibrated";
  return { width, height };
}

function overlapInterval(startA: number, lengthA: number, startB: number, lengthB: number) {
  const start = Math.max(startA, startB);
  const end = Math.min(startA + lengthA, startB + lengthB);
  return end > start ? { start, end } : null;
}

function intervalOverlap(startA: number, lengthA: number, startB: number, lengthB: number) {
  const overlap = overlapInterval(startA, lengthA, startB, lengthB);
  return overlap ? overlap.end - overlap.start : 0;
}
