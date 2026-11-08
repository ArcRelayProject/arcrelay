const MAX_TRAY_TRANSFER_PANEL_HEIGHT = 640;

export interface PanelBoxMetrics {
  renderedHeight: number;
  clientHeight: number;
  scrollHeight: number;
  contentClientHeight?: number;
  contentScrollHeight?: number;
}

/**
 * Measure the panel's intrinsic content instead of its viewport-clipped box.
 *
 * The native tray window and `.tray-panel` size each other. Once the panel is
 * clipped by `max-height: 100vh`, its bounding box alone can only report the
 * old window height and the window can never grow to reveal the send controls.
 */
export function trayTransferPanelHeight(metrics: PanelBoxMetrics): number {
  const borderHeight = Math.max(0, metrics.renderedHeight - metrics.clientHeight);
  // The unified scroll body no longer contributes its overflow to the outer
  // panel's scrollHeight. Include it to grow beyond the initial hover window.
  const contentOverflow = Math.max(
    0,
    (metrics.contentScrollHeight ?? 0) - (metrics.contentClientHeight ?? 0),
  );
  const intrinsicHeight = metrics.scrollHeight + borderHeight + contentOverflow;
  return Math.min(
    MAX_TRAY_TRANSFER_PANEL_HEIGHT,
    Math.ceil(Math.max(metrics.renderedHeight, intrinsicHeight)),
  );
}
