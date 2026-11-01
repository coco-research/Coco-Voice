// Word error rate for headless transcription JSONL.
// CLI: bun scripts/wer.ts <results.jsonl> [--totals-only]

import { readFileSync } from "node:fs";

export type WordErrorCounts = {
  sub: number;
  del: number;
  ins: number;
  refWords: number;
};

type Run = {
  clip: string;
  variant: string;
  model: string;
  ref: string;
  hyp: string;
  ok: boolean;
};

/**
 * Lowercase, map other punctuation to spaces, drop apostrophes, collapse
 * whitespace, and return the words.
 */
export function normalize(text: string): string[] {
  const cleaned = text
    .toLowerCase()
    .replace(/[^\p{L}\p{Nd}'\s]/gu, " ")
    .replace(/'/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return cleaned === "" ? [] : cleaned.split(" ");
}

/**
 * Word-level Levenshtein counts. On a cost tie, prefer a match or
 * substitution, then a deletion, then an insertion.
 */
export function wordErrors(ref: string, hyp: string): WordErrorCounts {
  const left = normalize(ref);
  const right = normalize(hyp);
  const n = left.length;
  const m = right.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () =>
    new Array<number>(m + 1).fill(0),
  );
  for (let i = 0; i <= n; i++) dp[i][0] = i;
  for (let j = 0; j <= m; j++) dp[0][j] = j;
  for (let i = 1; i <= n; i++) {
    for (let j = 1; j <= m; j++) {
      const subCost = left[i - 1] === right[j - 1] ? 0 : 1;
      dp[i][j] = Math.min(
        dp[i - 1][j] + 1,
        dp[i][j - 1] + 1,
        dp[i - 1][j - 1] + subCost,
      );
    }
  }

  let sub = 0;
  let del = 0;
  let ins = 0;
  let i = n;
  let j = m;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0) {
      const subCost = left[i - 1] === right[j - 1] ? 0 : 1;
      if (dp[i][j] === dp[i - 1][j - 1] + subCost) {
        if (subCost === 1) sub++;
        i--;
        j--;
        continue;
      }
    }
    if (i > 0 && dp[i][j] === dp[i - 1][j] + 1) {
      del++;
      i--;
      continue;
    }
    if (j > 0 && dp[i][j] === dp[i][j - 1] + 1) {
      ins++;
      j--;
      continue;
    }
    throw new Error("word alignment backtrace failed");
  }

  return { sub, del, ins, refWords: n };
}

/** (sub + del + ins) / refWords. Zero when the reference is empty and nothing was wrong. */
export function wer(errors: WordErrorCounts): number {
  const total = errors.sub + errors.del + errors.ins;
  if (errors.refWords === 0) return total === 0 ? 0 : Number.POSITIVE_INFINITY;
  return total / errors.refWords;
}

function errorsForRun(ref: string, hyp: string, ok: boolean): WordErrorCounts {
  if (!ok) {
    const refWords = normalize(ref).length;
    return { sub: 0, del: refWords, ins: 0, refWords };
  }
  return wordErrors(ref, hyp);
}

function formatPct(rate: number): string {
  if (!Number.isFinite(rate)) return "n/a";
  return `${(rate * 100).toFixed(1)}%`;
}

function readRuns(path: string): Run[] {
  const text = readFileSync(path, "utf8");
  const runs: Run[] = [];
  const lines = text.split("\n");
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index].trim();
    if (line === "") continue;
    const where = `${path}:${index + 1}`;
    let value: unknown;
    try {
      value = JSON.parse(line);
    } catch {
      throw new Error(`${where}: invalid JSON`);
    }
    if (typeof value !== "object" || value === null) {
      throw new Error(`${where}: expected an object`);
    }
    const row = value as Record<string, unknown>;
    if (
      typeof row.clip !== "string" ||
      typeof row.variant !== "string" ||
      typeof row.model !== "string" ||
      typeof row.ref !== "string" ||
      typeof row.hyp !== "string" ||
      typeof row.ok !== "boolean"
    ) {
      throw new Error(
        `${where}: need clip, variant, model, ref, hyp, and boolean ok`,
      );
    }
    runs.push({
      clip: row.clip,
      variant: row.variant,
      model: row.model,
      ref: row.ref,
      hyp: row.hyp,
      ok: row.ok,
    });
  }
  return runs;
}

type Group = {
  model: string;
  variant: string;
  clips: number;
  failed: number;
  sub: number;
  del: number;
  ins: number;
  refWords: number;
};

type ClipScore = {
  model: string;
  variant: string;
  clip: string;
  ref: string;
  hyp: string;
  rate: number;
};

function report(runs: Run[], totalsOnly: boolean): void {
  const modelOrder: string[] = [];
  const groups = new Map<string, Group>();
  const clips: ClipScore[] = [];

  for (const run of runs) {
    if (!modelOrder.includes(run.model)) modelOrder.push(run.model);
    const key = `${run.model}\0${run.variant}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        model: run.model,
        variant: run.variant,
        clips: 0,
        failed: 0,
        sub: 0,
        del: 0,
        ins: 0,
        refWords: 0,
      };
      groups.set(key, group);
    }
    const errors = errorsForRun(run.ref, run.hyp, run.ok);
    group.clips++;
    if (!run.ok) group.failed++;
    group.sub += errors.sub;
    group.del += errors.del;
    group.ins += errors.ins;
    group.refWords += errors.refWords;
    clips.push({
      model: run.model,
      variant: run.variant,
      clip: run.clip,
      ref: run.ref,
      hyp: run.hyp,
      rate: wer(errors),
    });
  }

  const rows: string[][] = [];
  for (const model of modelOrder) {
    const variants = ["clean", "noisy"];
    const extras = [...groups.values()]
      .filter(
        (group) => group.model === model && !variants.includes(group.variant),
      )
      .map((group) => group.variant);
    for (const variant of [...variants, ...extras]) {
      const group = groups.get(`${model}\0${variant}`);
      if (!group) continue;
      rows.push([
        group.model,
        group.variant,
        String(group.clips),
        String(group.failed),
        String(group.refWords),
        formatPct(wer(group)),
      ]);
    }
  }

  const header = ["model", "variant", "clips", "failed", "ref_words", "wer"];
  const widths = header.map((label, index) =>
    Math.max(label.length, ...rows.map((row) => row[index].length), 0),
  );
  const formatRow = (cols: string[]): string =>
    cols
      .map((col, index) =>
        index >= 2 ? col.padStart(widths[index]) : col.padEnd(widths[index]),
      )
      .join("  ");

  console.log(formatRow(header));
  for (const row of rows) console.log(formatRow(row));

  if (totalsOnly) return;

  for (const model of modelOrder) {
    const worst = clips
      .filter((clip) => clip.model === model)
      .sort(
        (a, b) =>
          b.rate - a.rate ||
          a.clip.localeCompare(b.clip) ||
          a.variant.localeCompare(b.variant),
      )
      .slice(0, 5);
    console.log("");
    console.log(`worst ${model}`);
    for (const clip of worst) {
      console.log(`${formatPct(clip.rate)}  ${clip.clip}  ${clip.variant}`);
      console.log(`  ref: ${clip.ref}`);
      console.log(`  hyp: ${clip.hyp}`);
    }
  }
}

if (import.meta.main) {
  const totalsOnly = process.argv.includes("--totals-only");
  const args = process.argv.slice(2).filter((arg) => arg !== "--totals-only");
  if (args.length !== 1) {
    console.error("Usage: bun scripts/wer.ts <results.jsonl> [--totals-only]");
    process.exit(1);
  }
  try {
    report(readRuns(args[0]), totalsOnly);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
