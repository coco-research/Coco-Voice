#!/usr/bin/env bun
import { mkdir, rename } from "node:fs/promises";
import { dirname, join } from "node:path";

export type Journey = {
  id: string;
  journey: string;
  check: string;
  tests: string[];
};

export type Summary = {
  product: "voice";
  ts: string;
  pass: number;
  total: number;
  failing: string[];
};

/** name -> "ok" | "FAILED" | "ignored", from `cargo test` harness lines. */
export function parseCargoTestOutput(output: string): Map<string, string> {
  const results = new Map<string, string>();
  for (const line of output.replace(/\x1b\[[0-9;]*m/g, "").split("\n")) {
    const m = line.match(/^test (.+?) \.\.\. (ok|FAILED|ignored)\b/);
    if (m) results.set(m[1].split("::").pop()!, m[2]);
  }
  return results;
}

export function isoTs(d = new Date()): string {
  const offset = -d.getTimezoneOffset();
  const pad = (n: number) => String(n).padStart(2, "0");
  const local = new Date(d.getTime() + offset * 60_000)
    .toISOString()
    .slice(0, 23);
  const sign = offset >= 0 ? "+" : "-";
  return `${local}${sign}${pad(Math.floor(Math.abs(offset) / 60))}:${pad(Math.abs(offset) % 60)}`;
}

export function summarize(
  journeys: Journey[],
  results: Map<string, string>,
  now = new Date(),
): Summary {
  const failing = journeys
    .filter(
      (j) =>
        j.tests.length === 0 || !j.tests.every((t) => results.get(t) === "ok"),
    )
    .map((j) => j.id);
  return {
    product: "voice",
    ts: isoTs(now),
    pass: journeys.length - failing.length,
    total: journeys.length,
    failing,
  };
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  let out: string | undefined;
  const cargoArgs: string[] = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--out") out = args[++i];
    else if (args[i] !== "--") cargoArgs.push(args[i]);
  }
  if (out === undefined && args.includes("--out")) {
    console.error("--out needs a path");
    process.exit(1);
  }

  const journeys: Journey[] = await Bun.file(
    join(import.meta.dir, "voice-journeys.json"),
  ).json();

  let proc: ReturnType<typeof Bun.spawn>;
  try {
    proc = Bun.spawn(["cargo", "test", "--lib", ...cargoArgs], {
      cwd: join(import.meta.dir, "..", "src-tauri"),
      stdout: "pipe",
      stderr: "inherit",
      env: process.env,
    });
  } catch (err) {
    console.error(`cargo could not run: ${err}`);
    process.exit(1);
  }

  const text = await new Response(proc.stdout).text();
  const code = await proc.exited;
  const results = parseCargoTestOutput(text);

  if (code !== 0) process.stderr.write(text);
  if (code !== 0 && results.size === 0) {
    console.error(`cargo test did not run (exit ${code})`);
    process.exit(1);
  }

  const json = JSON.stringify(summarize(journeys, results));
  if (out) {
    const tmp = `${out}.tmp-${process.pid}`;
    await mkdir(dirname(out), { recursive: true });
    await Bun.write(tmp, json + "\n");
    await rename(tmp, out);
  }
  console.log(json);
}

if (import.meta.main) await main();
