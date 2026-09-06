export interface ImagePreviewLayoutInput {
  imageWidth: number;
  imageHeight: number;
  canvasWidth: number;
  canvasHeight: number;
  scale: number;
  rotation: number;
  padding?: number;
}

export interface ImagePreviewLayout {
  imageWidth: number;
  imageHeight: number;
  stageWidth: number;
  stageHeight: number;
}

export interface ImagePreviewHeightInput {
  imageWidth: number;
  imageHeight: number;
  containerWidth: number;
  rotation: number;
  padding?: number;
  minimum?: number;
  maximum?: number;
}

export function imagePreviewHeight({
  imageWidth,
  imageHeight,
  containerWidth,
  rotation,
  padding = 12,
  minimum = 260,
  maximum = 560,
}: ImagePreviewHeightInput) {
  if (imageWidth <= 0 || imageHeight <= 0 || containerWidth <= 0) return Math.min(maximum, 420);
  const quarterTurn = Math.abs(rotation % 180) === 90;
  const rotatedWidth = quarterTurn ? imageHeight : imageWidth;
  const rotatedHeight = quarterTurn ? imageWidth : imageHeight;
  const contentWidth = Math.max(1, containerWidth - padding * 2);
  const fittedHeight = contentWidth * rotatedHeight / rotatedWidth + padding * 2;
  return Math.max(minimum, Math.min(maximum, fittedHeight));
}

export function imagePreviewLayout({
  imageWidth,
  imageHeight,
  canvasWidth,
  canvasHeight,
  scale,
  rotation,
  padding = 12,
}: ImagePreviewLayoutInput): ImagePreviewLayout | null {
  if (imageWidth <= 0 || imageHeight <= 0 || canvasWidth <= 0 || canvasHeight <= 0) return null;

  const quarterTurn = Math.abs(rotation % 180) === 90;
  const rotatedWidth = quarterTurn ? imageHeight : imageWidth;
  const rotatedHeight = quarterTurn ? imageWidth : imageHeight;
  const availableWidth = Math.max(1, canvasWidth - padding * 2);
  const availableHeight = Math.max(1, canvasHeight - padding * 2);
  const fitScale = Math.min(2, availableWidth / rotatedWidth, availableHeight / rotatedHeight);
  const displayedWidth = imageWidth * fitScale * scale;
  const displayedHeight = imageHeight * fitScale * scale;
  const boundsWidth = quarterTurn ? displayedHeight : displayedWidth;
  const boundsHeight = quarterTurn ? displayedWidth : displayedHeight;

  return {
    imageWidth: displayedWidth,
    imageHeight: displayedHeight,
    stageWidth: Math.max(canvasWidth, boundsWidth + padding * 2),
    stageHeight: Math.max(canvasHeight, boundsHeight + padding * 2),
  };
}
