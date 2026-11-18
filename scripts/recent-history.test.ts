import { expect, test } from "bun:test";
import { observeRecentHistory } from "../src/components/home/recentHistory";

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("subscribes before reading the initial preview", async () => {
  const order: string[] = [];
  const values: string[][] = [];
  const stop = observeRecentHistory(
    async () => {
      order.push("load");
      return ["first"];
    },
    async () => {
      order.push("listen");
      return () => {};
    },
    (value) => values.push(value),
    () => {},
  );
  await flush();
  expect(order).toEqual(["listen", "load"]);
  expect(values).toEqual([["first"]]);
  stop();
});

test("a delete invalidates displayed text synchronously while refresh is pending", async () => {
  let refresh!: () => void;
  let visible = ["deleted"];
  const pending = deferred<string[]>();
  let calls = 0;
  const stop = observeRecentHistory(
    () => (++calls === 1 ? Promise.resolve(["deleted"]) : pending.promise),
    async (fn) => {
      refresh = fn;
      return () => {};
    },
    (value) => {
      visible = value;
    },
    () => {},
    () => {
      visible = [];
    },
  );
  await flush();
  expect(visible).toEqual(["deleted"]);
  refresh();
  expect(visible).toEqual([]);
  pending.resolve([]);
  await flush();
  expect(visible).toEqual([]);
  stop();
});

test("newer refresh wins even when an older read finishes last", async () => {
  let refresh!: () => void;
  const reads = [deferred<string[]>(), deferred<string[]>()];
  let calls = 0;
  const published: string[][] = [];
  const stop = observeRecentHistory(
    () =>
      calls++ === 0 ? Promise.resolve(["initial"]) : reads[calls - 2]!.promise,
    async (fn) => {
      refresh = fn;
      return () => {};
    },
    (value) => published.push(value),
    () => {},
  );
  await flush();
  refresh();
  refresh();
  reads[1]!.resolve(["new"]);
  await flush();
  reads[0]!.resolve(["old"]);
  await flush();
  expect(published).toEqual([["initial"], ["new"]]);
  stop();
});

test("failed refresh does not restore deleted text", async () => {
  let refresh!: () => void;
  let visible: string[] = [];
  let errors = 0,
    calls = 0;
  const stop = observeRecentHistory(
    () =>
      ++calls === 1
        ? Promise.resolve(["deleted"])
        : Promise.reject(new Error("fixture")),
    async (fn) => {
      refresh = fn;
      return () => {};
    },
    (value) => {
      visible = value;
    },
    () => {
      errors++;
    },
    () => {
      visible = [];
    },
  );
  await flush();
  refresh();
  await flush();
  expect(visible).toEqual([]);
  expect(errors).toBe(1);
  stop();
});

test("unmount fences pending reads and already queued refresh callbacks", async () => {
  const read = deferred<string[]>();
  let refresh!: () => void;
  let calls = 0,
    released = 0;
  const published: string[][] = [];
  const stop = observeRecentHistory(
    () => {
      calls++;
      return read.promise;
    },
    async (fn) => {
      refresh = fn;
      return () => {
        released++;
      };
    },
    (value) => published.push(value),
    () => {},
  );
  await flush();
  stop();
  refresh();
  read.resolve(["late"]);
  await flush();
  expect(published).toEqual([]);
  expect(calls).toBe(1);
  expect(released).toBe(1);
});

test("a listener that resolves after unmount is released without fetching", async () => {
  const listener = deferred<() => void>();
  let calls = 0,
    released = 0;
  const stop = observeRecentHistory(
    async () => {
      calls++;
      return [];
    },
    () => listener.promise,
    () => {},
    () => {},
  );
  await flush();
  stop();
  listener.resolve(() => {
    released++;
  });
  await flush();
  expect(calls).toBe(0);
  expect(released).toBe(1);
});

test("immediate unmount never starts a native subscription or read", async () => {
  let calls = 0;
  const stop = observeRecentHistory(
    async () => {
      calls++;
      return [];
    },
    async () => {
      calls++;
      return () => {};
    },
    () => {},
    () => {},
  );
  stop();
  await flush();
  expect(calls).toBe(0);
});

test("subscription failure does not publish an unobserved snapshot", async () => {
  let calls = 0,
    errors = 0;
  const published: string[][] = [];
  const stop = observeRecentHistory(
    async () => {
      calls++;
      return ["unobserved"];
    },
    async () => {
      throw new Error("fixture");
    },
    (value) => published.push(value),
    () => {
      errors++;
    },
  );
  await flush();
  expect(calls).toBe(0);
  expect(published).toEqual([]);
  expect(errors).toBe(1);
  stop();
});

test("a read started by an event cannot publish after subscription failure", async () => {
  const read = deferred<string[]>();
  const published: string[][] = [];
  let errors = 0;
  const stop = observeRecentHistory(
    () => read.promise,
    async (refresh) => {
      refresh();
      throw new Error("fixture");
    },
    (value) => published.push(value),
    () => {
      errors++;
    },
  );
  await flush();
  read.resolve(["unobserved"]);
  await flush();
  expect(published).toEqual([]);
  expect(errors).toBe(1);
  stop();
});
