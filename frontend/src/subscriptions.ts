export type Stop = () => void;

/** Owns asynchronous listener registration, including unmount during registration. */
export class SubscriptionScope {
  private stops = new Set<Stop>();
  disposed = false;

  async add(registration: Promise<Stop>): Promise<void> {
    const stop = await registration;
    if (this.disposed) stop();
    else this.stops.add(stop);
  }

  dispose = () => {
    this.disposed = true;
    for (const stop of this.stops) stop();
    this.stops.clear();
  };
}

/** Register before reading and reject older snapshots from overlapping IPC calls. */
export async function observeSnapshot<T extends { revision: number }>(
  scope: SubscriptionScope,
  listen: (accept: (value: T) => void) => Promise<Stop>,
  read: () => Promise<T>,
  apply: (value: T) => void,
) {
  let revision = -1;
  const accept = (value: T) => {
    if (scope.disposed || value.revision <= revision) return;
    revision = value.revision;
    apply(value);
  };
  await scope.add(listen(accept));
  if (!scope.disposed) accept(await read());
}

/** Coalesces invalidations without dropping one arriving during an in-flight read. */
export function createInvalidationLoader(load: () => Promise<void>, onError: (error: unknown) => void) {
  let running = false;
  let dirty = false;
  let disposed = false;
  return {
    async refresh() {
      dirty = true;
      if (running || disposed) return;
      running = true;
      try {
        while (dirty && !disposed) {
          dirty = false;
          try { await load(); } catch (error) { onError(error); }
        }
      } finally { running = false; }
    },
    dispose() { disposed = true; },
  };
}
