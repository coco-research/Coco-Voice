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

const UNIT_WORDS = [
  "zero",
  "one",
  "two",
  "three",
  "four",
  "five",
  "six",
  "seven",
  "eight",
  "nine",
  "ten",
  "eleven",
  "twelve",
  "thirteen",
  "fourteen",
  "fifteen",
  "sixteen",
  "seventeen",
  "eighteen",
  "nineteen",
];

const TENS_WORDS = [
  "twenty",
  "thirty",
  "forty",
  "fifty",
  "sixty",
  "seventy",
  "eighty",
  "ninety",
];

const ORDINAL_UNIT_WORDS = [
  "zeroth",
  "first",
  "second",
  "third",
  "fourth",
  "fifth",
  "sixth",
  "seventh",
  "eighth",
  "ninth",
  "tenth",
  "eleventh",
  "twelfth",
  "thirteenth",
  "fourteenth",
  "fifteenth",
  "sixteenth",
  "seventeenth",
  "eighteenth",
  "nineteenth",
];

const ORDINAL_TENS_WORDS = [
  "twentieth",
  "thirtieth",
  "fortieth",
  "fiftieth",
  "sixtieth",
  "seventieth",
  "eightieth",
  "ninetieth",
];

const UNIT_VALUE = new Map<string, number>(
  UNIT_WORDS.map((word, index): [string, number] => [word, index]),
);
const DIGIT_VALUE = new Map<string, number>(
  UNIT_WORDS.slice(0, 10).map((word, index): [string, number] => [word, index]),
);
const TENS_VALUE = new Map<string, number>(
  TENS_WORDS.map((word, index): [string, number] => [word, (index + 2) * 10]),
);
const ORDINAL_UNIT_VALUE = new Map<string, number>(
  ORDINAL_UNIT_WORDS.map((word, index): [string, number] => [word, index]),
);
const ORDINAL_TENS_VALUE = new Map<string, number>(
  ORDINAL_TENS_WORDS.map((word, index): [string, number] => [
    word,
    (index + 2) * 10,
  ]),
);
const SCALE_VALUE = new Map<string, number>([
  ["hundred", 100],
  ["thousand", 1_000],
  ["million", 1_000_000],
]);

const DIGIT_TOKEN = /^\d+$/;
const DIGIT_ORDINAL = /^(\d+)(?:st|nd|rd|th)$/;

function isNumberContinuation(token: string | undefined): boolean {
  if (token === undefined) return false;
  return (
    UNIT_VALUE.has(token) ||
    TENS_VALUE.has(token) ||
    ORDINAL_UNIT_VALUE.has(token) ||
    ORDINAL_TENS_VALUE.has(token) ||
    SCALE_VALUE.has(token)
  );
}

/**
 * Whether a unit or tens word may continue the run. A unit word needs an
 * empty ones place; a teen, tens, or ordinal-tens word needs current to be a
 * multiple of 100.
 */
function continuesNumber(
  current: number,
  value: number,
  lastWasUnit: boolean,
): boolean {
  if (lastWasUnit) return false;
  return value < 10 ? current % 10 === 0 : current % 100 === 0;
}

type ParsedNumber = { value: number; next: number };

/** Parse a cardinal or ordinal number word run starting at `start`. */
function parseNumber(words: string[], start: number): ParsedNumber | null {
  let total = 0;
  let current = 0;
  let saw = false;
  let lastWasUnit = false;
  let index = start;
  if (words[index] === "a" && SCALE_VALUE.has(words[index + 1])) {
    current = 1;
    saw = true;
    index++;
  }
  while (index < words.length) {
    const token = words[index];
    if (token === "and") {
      if (!saw || !isNumberContinuation(words[index + 1])) break;
      index++;
      continue;
    }
    const ordinalUnit = ORDINAL_UNIT_VALUE.get(token);
    if (ordinalUnit !== undefined) {
      if (token === "second" && !saw) break;
      if (!continuesNumber(current, ordinalUnit, lastWasUnit)) break;
      current += ordinalUnit;
      saw = true;
      index++;
      break;
    }
    const ordinalTen = ORDINAL_TENS_VALUE.get(token);
    if (ordinalTen !== undefined) {
      if (!continuesNumber(current, ordinalTen, lastWasUnit)) break;
      current += ordinalTen;
      saw = true;
      index++;
      break;
    }
    const scale = SCALE_VALUE.get(token);
    if (scale !== undefined) {
      current = (current === 0 ? 1 : current) * scale;
      if (scale >= 1_000) {
        total += current;
        current = 0;
      }
      saw = true;
      lastWasUnit = false;
      index++;
      continue;
    }
    const unit = UNIT_VALUE.get(token);
    if (unit !== undefined) {
      if (!continuesNumber(current, unit, lastWasUnit)) break;
      current += unit;
      saw = true;
      lastWasUnit = true;
      index++;
      continue;
    }
    const ten = TENS_VALUE.get(token);
    if (ten !== undefined) {
      if (!continuesNumber(current, ten, lastWasUnit)) break;
      current += ten;
      saw = true;
      lastWasUnit = false;
      index++;
      continue;
    }
    if (DIGIT_TOKEN.test(token)) {
      if (saw) break;
      current += Number(token);
      saw = true;
      index++;
      if (!SCALE_VALUE.has(words[index])) break;
      continue;
    }
    break;
  }
  if (!saw) return null;
  return { value: total + current, next: index };
}

/** Digits after "point": digit words, digit runs, or both. */
function parseFraction(
  words: string[],
  start: number,
): { digits: string; next: number } | null {
  let digits = "";
  let index = start;
  while (index < words.length) {
    const token = words[index];
    const digit = DIGIT_VALUE.get(token);
    if (digit !== undefined) {
      digits += String(digit);
      index++;
      continue;
    }
    if (DIGIT_TOKEN.test(token)) {
      digits += token;
      index++;
      continue;
    }
    break;
  }
  return digits === "" ? null : { digits, next: index };
}

/**
 * Turn English number words into digit tokens so "four thousand four hundred
 * eighty one" and "4481" meet at the same token.
 */
export function normalizeNumbers(words: string[]): string[] {
  const out: string[] = [];
  let index = 0;
  while (index < words.length) {
    const token = words[index];
    const digitOrdinal = DIGIT_ORDINAL.exec(token);
    if (digitOrdinal) {
      out.push(String(Number(digitOrdinal[1])));
      index++;
      continue;
    }
    const parsed = parseNumber(words, index);
    if (!parsed) {
      out.push(token);
      index++;
      continue;
    }
    let text = String(parsed.value);
    let next = parsed.next;
    if (words[next] === "point") {
      const fraction = parseFraction(words, next + 1);
      if (fraction) {
        text = `${text}.${fraction.digits}`;
        next = fraction.next;
      }
    }
    out.push(text);
    index = next;
  }
  return out;
}

/**
 * normalize() for scoring: keep thousands commas and decimal points that
 * normalize() would otherwise split, expand `$`, then digit-ify number words.
 */
export function normalizeForScoring(text: string): string[] {
  const prepared = text
    .replace(/\$(\d[\d,.]*)/g, "$1 dollars")
    .replace(/(?<=\d),(?=\d{3}(?!\d))/g, "")
    .replace(/(\d)\.(\d)/g, "$1 point $2");
  return normalizeNumbers(normalize(prepared));
}

/**
 * Word-level Levenshtein counts over two word arrays. On a cost tie, prefer a
 * match or substitution, then a deletion, then an insertion.
 */
function wordErrorCounts(left: string[], right: string[]): WordErrorCounts {
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

/** Raw word errors: a spelled-out number counts as its own words. */
export function wordErrors(ref: string, hyp: string): WordErrorCounts {
  return wordErrorCounts(normalize(ref), normalize(hyp));
}

/** Number-aware word errors: digits and spelled-out numbers score as equal. */
export function wordErrorsNormalized(
  ref: string,
  hyp: string,
): WordErrorCounts {
  return wordErrorCounts(normalizeForScoring(ref), normalizeForScoring(hyp));
}

/** (sub + del + ins) / refWords. Zero when the reference is empty and nothing was wrong. */
export function wer(errors: WordErrorCounts): number {
  const total = errors.sub + errors.del + errors.ins;
  if (errors.refWords === 0) return total === 0 ? 0 : Number.POSITIVE_INFINITY;
  return total / errors.refWords;
}

function errorsForRun(
  ref: string,
  hyp: string,
  ok: boolean,
  toWords: (text: string) => string[],
): WordErrorCounts {
  if (!ok) {
    const refWords = toWords(ref).length;
    return { sub: 0, del: refWords, ins: 0, refWords };
  }
  return wordErrorCounts(toWords(ref), toWords(hyp));
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
  raw: WordErrorCounts;
  num: WordErrorCounts;
};

type ClipScore = {
  model: string;
  variant: string;
  clip: string;
  ref: string;
  hyp: string;
  rate: number;
  rateNum: number;
};

function emptyCounts(): WordErrorCounts {
  return { sub: 0, del: 0, ins: 0, refWords: 0 };
}

function addCounts(total: WordErrorCounts, errors: WordErrorCounts): void {
  total.sub += errors.sub;
  total.del += errors.del;
  total.ins += errors.ins;
  total.refWords += errors.refWords;
}

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
        raw: emptyCounts(),
        num: emptyCounts(),
      };
      groups.set(key, group);
    }
    const raw = errorsForRun(run.ref, run.hyp, run.ok, normalize);
    const num = errorsForRun(run.ref, run.hyp, run.ok, normalizeForScoring);
    group.clips++;
    if (!run.ok) group.failed++;
    addCounts(group.raw, raw);
    addCounts(group.num, num);
    clips.push({
      model: run.model,
      variant: run.variant,
      clip: run.clip,
      ref: run.ref,
      hyp: run.hyp,
      rate: wer(raw),
      rateNum: wer(num),
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
        String(group.raw.refWords),
        formatPct(wer(group.raw)),
        formatPct(wer(group.num)),
      ]);
    }
  }

  const header = [
    "model",
    "variant",
    "clips",
    "failed",
    "ref_words",
    "wer",
    "wer_num",
  ];
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
          b.rateNum - a.rateNum ||
          a.clip.localeCompare(b.clip) ||
          a.variant.localeCompare(b.variant),
      )
      .slice(0, 5);
    console.log("");
    console.log(`worst ${model}`);
    for (const clip of worst) {
      console.log(
        `${formatPct(clip.rateNum)} (raw ${formatPct(clip.rate)})  ${clip.clip}  ${clip.variant}`,
      );
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
