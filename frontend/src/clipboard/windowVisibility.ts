type Stop = () => void;
interface VisibilityPort {
  onShown(handler: () => void): Promise<Stop>;
  onHidden(handler: () => void): Promise<Stop>;
  visible(): Promise<boolean>;
}

/** Subscribe before reading state; a newer event wins over an in-flight query. */
export async function observeWindowVisibility(port: VisibilityPort, shown: Stop, hidden: Stop): Promise<Stop> {
  const stops: Stop[] = [];
  let revision = 0;
  let stopped = false;
  const apply = (visible: boolean) => {
    if (stopped) return;
    revision++;
    (visible ? shown : hidden)();
  };
  const stop = () => { stopped = true; stops.splice(0).forEach(fn => fn()); };
  try {
    stops.push(await port.onShown(() => apply(true)));
    stops.push(await port.onHidden(() => apply(false)));
    const before = revision;
    const visible = await port.visible();
    if (revision === before) apply(visible);
    return stop;
  } catch (error) {
    stop();
    throw error;
  }
}
