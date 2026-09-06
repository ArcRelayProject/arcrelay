import { invoke, listen } from './ipc/client';

import type { TransferSnapshot, TransferProgress } from "./types";
import { SubscriptionScope, createInvalidationLoader } from "./subscriptions";
import { TransferProjection } from "./transferProjection";

export async function subscribeTransfer(listener: (value: TransferSnapshot) => void) {
  const scope = new SubscriptionScope();
  const projection = new TransferProjection();
  const publish = () => { if (!scope.disposed && projection.value) listener(projection.value); };
  const refresh = createInvalidationLoader(async () => {
    if (projection.snapshot(await invoke("get_transfer_state"))) publish();
  }, console.error);
  try {
    await Promise.all([
      scope.add(listen("transfer-state", (event) => { if (projection.snapshot(event.payload)) publish(); })),
      scope.add(listen("transfer-progress", (event) => {
        if (projection.progress(event.payload)) publish(); else void refresh.refresh();
      })),
    ]);
    void refresh.refresh();
    return () => { scope.dispose(); refresh.dispose(); };
  } catch (error) { scope.dispose(); refresh.dispose(); throw error; }
}
