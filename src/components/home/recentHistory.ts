/** Keep a bounded history preview current without publishing stale responses. */
export function observeRecentHistory<T>(
  load: () => Promise<T>,
  listen: (refresh: () => void) => Promise<() => void>,
  publish: (value: T) => void,
  failed: () => void,
  invalidated: () => void = () => {},
): () => void {
  let active = true;
  let generation = 0;
  let unlisten: (() => void) | undefined;
  const refresh = async () => {
    if (!active) return;
    const request = ++generation;
    try {
      const value = await load();
      if (active && request === generation) publish(value);
    } catch {
      if (active && request === generation) failed();
    }
  };
  // Subscribe before the first read so an intervening delete cannot be missed.
  void Promise.resolve()
    .then(() => {
      if (!active) return;
      return listen(() => {
        if (!active) return;
        invalidated();
        void refresh();
      });
    })
    .then((stop) => {
      if (!stop) return;
      if (!active) {
        stop();
        return;
      }
      unlisten = stop;
      void refresh();
    })
    .catch(() => {
      if (active) {
        active = false;
        ++generation;
        failed();
      }
    });
  return () => {
    active = false;
    ++generation;
    const release = unlisten;
    unlisten = undefined;
    release?.();
  };
}
