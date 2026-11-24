import type { Result } from "../../bindings.meetings";

export type OptimisticActionOutcome<T> = { ok: true; data: T } | { ok: false };

export async function runOptimisticCommand<T, E = string>(
  apply: () => void,
  run: () => Promise<Result<T, E>>,
  undo: () => void,
): Promise<OptimisticActionOutcome<T>> {
  apply();
  try {
    const res = await run();
    if (res.status === "ok") {
      return { ok: true, data: res.data };
    }
  } catch {
    // Undo optimistic update below on unexpected rejection.
  }
  undo();
  return { ok: false };
}
