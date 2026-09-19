export type ClipboardDragMode = "auto" | "plain_text" | "rich_text" | "text_file";
export interface ClipboardDragRequest {
  ids: number[];
  mode: ClipboardDragMode;
  selection?: { id: number; text: string };
}
export interface PreparedClipboardDrag {
  token: string;
  kind: "text" | "files";
  count: number;
}
export interface ClipboardDragEnded {
  token: string;
  outcome: "dropped" | "cancelled" | "failed";
  error?: string | null;
}
export type ClipboardDragPhase = "idle" | "preparing" | "ready" | "dragging";

export function clipboardDragIds(source: number, selected: readonly number[]): number[] {
  return selected.includes(source) ? [...new Set(selected)] : [source];
}

interface Gesture {
  request: ClipboardDragRequest;
  x: number;
  y: number;
  pointerId: number;
  nativeSelection: boolean;
  wantsStart: boolean;
  held: boolean;
  preparing: boolean;
  prepared?: PreparedClipboardDrag;
  starting: boolean;
}

/** Owns the async preparation race. A resolved start command is not a completed drag. */
export function createClipboardDragSession(options: {
  prepare: (request: ClipboardDragRequest) => Promise<PreparedClipboardDrag>;
  start: (token: string) => Promise<void>;
  cancel: (token: string) => Promise<void>;
  state: (phase: ClipboardDragPhase, count: number) => void;
  error: (error: unknown) => void;
  dropped?: () => void;
}) {
  let gesture: Gesture | undefined;
  let suppressUntil = 0;
  let disposed = false;
  const cancelToken = (token: string) => {
    void options.cancel(token).catch(() => {});
  };

  function reset() {
    if (gesture?.wantsStart) suppressUntil = Date.now() + 600;
    gesture = undefined;
    options.state("idle", 0);
  }

  function cancelPending() {
    if (!gesture || gesture.starting) return;
    gesture.held = false;
    if (gesture.prepared) cancelToken(gesture.prepared.token);
    reset();
  }

  function start(current: Gesture) {
    if (
      gesture !== current ||
      !current.held ||
      !current.wantsStart ||
      !current.prepared ||
      current.starting
    )
      return;
    current.starting = true;
    suppressUntil = Date.now() + 600;
    options.state("dragging", current.prepared.count);
    void options.start(current.prepared.token).catch((error) => {
      if (gesture !== current) return;
      cancelToken(current.prepared!.token);
      reset();
      options.error(error);
    });
  }

  async function prepare(current: Gesture) {
    if (current.preparing) return;
    current.preparing = true;
    options.state("preparing", current.request.ids.length);
    try {
      const prepared = await options.prepare(current.request);
      if (disposed || gesture !== current || !current.held) {
        cancelToken(prepared.token);
        return;
      }
      current.prepared = prepared;
      options.state("ready", prepared.count);
      start(current);
    } catch (error) {
      if (gesture !== current) return;
      reset();
      options.error(error);
    }
  }

  return {
    begin(
      request: ClipboardDragRequest,
      event: Pick<
        PointerEvent,
        "button" | "buttons" | "clientX" | "clientY" | "pointerId" | "pointerType"
      >,
      nativeSelection = false,
      prewarm = false,
    ) {
      if (
        disposed ||
        gesture?.starting ||
        event.button !== 0 ||
        !(event.buttons & 1) ||
        event.pointerType === "touch" ||
        request.ids.length === 0
      )
        return;
      cancelPending();
      const current: Gesture = {
        request,
        x: event.clientX,
        y: event.clientY,
        pointerId: event.pointerId,
        nativeSelection,
        wantsStart: false,
        held: true,
        preparing: false,
        starting: false,
      };
      gesture = current;
      if (prewarm) void prepare(current);
    },
    move(event: Pick<PointerEvent, "buttons" | "clientX" | "clientY" | "pointerId">) {
      const current = gesture;
      if (!current || current.starting || current.pointerId !== event.pointerId) return;
      if (!(event.buttons & 1)) {
        cancelPending();
        return;
      }
      if (
        current.nativeSelection ||
        Math.hypot(event.clientX - current.x, event.clientY - current.y) < 6
      )
        return;
      current.wantsStart = true;
      suppressUntil = Date.now() + 600;
      if (!current.preparing) void prepare(current);
      start(current);
    },
    nativeDragStart() {
      const current = gesture;
      if (!current || current.starting || !current.held) return;
      current.wantsStart = true;
      suppressUntil = Date.now() + 600;
      if (!current.preparing) void prepare(current);
      start(current);
    },
    release(pointerId?: number) {
      if (pointerId !== undefined && gesture?.pointerId !== pointerId) return;
      cancelPending();
    },
    cancel() {
      if (gesture?.starting && gesture.prepared) cancelToken(gesture.prepared.token);
      else cancelPending();
    },
    ended(event: ClipboardDragEnded) {
      if (gesture?.prepared?.token !== event.token) return;
      suppressUntil = Date.now() + 600;
      reset();
      if (event.outcome === "failed") options.error(event.error ?? "Drag failed");
      if (event.outcome === "dropped") options.dropped?.();
    },
    suppressClick: () => Date.now() < suppressUntil,
    destroy() {
      disposed = true;
      if (gesture?.prepared) cancelToken(gesture.prepared.token);
      reset();
    },
  };
}
