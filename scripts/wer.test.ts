import { expect, test } from "bun:test";
import {
  normalize,
  normalizeForScoring,
  normalizeNumbers,
  wer,
  wordErrors,
  wordErrorsNormalized,
} from "./wer.ts";

test("identical text has WER 0", () => {
  const errors = wordErrors(
    "Please open the pull request.",
    "Please open the pull request.",
  );
  expect(errors.sub + errors.del + errors.ins).toBe(0);
  expect(wer(errors)).toBe(0);
});

test("one substituted word", () => {
  const errors = wordErrors("open the file", "open that file");
  expect(errors).toEqual({ sub: 1, del: 0, ins: 0, refWords: 3 });
  expect(wer(errors)).toBe(1 / 3);
});

test("one inserted word", () => {
  const errors = wordErrors("open the file", "open the big file");
  expect(errors).toEqual({ sub: 0, del: 0, ins: 1, refWords: 3 });
  expect(wer(errors)).toBe(1 / 3);
});

test("one deleted word", () => {
  const errors = wordErrors("open the file", "open file");
  expect(errors).toEqual({ sub: 0, del: 1, ins: 0, refWords: 3 });
  expect(wer(errors)).toBe(1 / 3);
});

test("normalization makes Hello, World! equal hello world", () => {
  expect(normalize("Hello, World!")).toEqual(normalize("hello world"));
  expect(normalize("Hello, World!")).toEqual(["hello", "world"]);
  expect(wer(wordErrors("Hello, World!", "hello world"))).toBe(0);
});

test("empty hypothesis against 4 reference words gives WER 1.0", () => {
  const errors = wordErrors("one two three four", "");
  expect(errors).toEqual({ sub: 0, del: 4, ins: 0, refWords: 4 });
  expect(wer(errors)).toBe(1);
});

test("pooled WER over two lines sums errors and words", () => {
  const first = wordErrors("alpha beta gamma", "alpha beta delta");
  const second = wordErrors("one two", "one");
  const pooled = {
    sub: first.sub + second.sub,
    del: first.del + second.del,
    ins: first.ins + second.ins,
    refWords: first.refWords + second.refWords,
  };
  expect(pooled).toEqual({ sub: 1, del: 1, ins: 0, refWords: 5 });
  expect(wer(pooled)).toBe(2 / 5);
});

test("spelled-out numbers score the same as digits", () => {
  const errors = wordErrorsNormalized(
    "The invoice is number 4481 for 250 dollars and it is due on June 30.",
    "The invoice is number four thousand four hundred eighty-one for two hundred fifty dollars, and it is due on June thirtieth.",
  );
  expect(errors.sub + errors.del + errors.ins).toBe(0);
});

test("decimal and ordinal variants score the same", () => {
  const ref = "Version 1 point 4 shipped on January 9";
  const words = wordErrorsNormalized(
    ref,
    "Version one point four shipped on January ninth",
  );
  const digits = wordErrorsNormalized(
    ref,
    "Version 1.4 shipped on January 9th",
  );
  expect(words.sub + words.del + words.ins).toBe(0);
  expect(digits.sub + digits.del + digits.ins).toBe(0);
});

test("currency, thousands commas, and number words score the same", () => {
  const currency = wordErrorsNormalized("$250", "250 dollars");
  const comma = wordErrorsNormalized("4,481", "4481");
  const andWord = wordErrorsNormalized("two hundred and fifty", "250");
  const ordinal = wordErrorsNormalized("thirty first", "31st");
  const aHundred = wordErrorsNormalized("a hundred", "100");
  expect(currency.sub + currency.del + currency.ins).toBe(0);
  expect(comma.sub + comma.del + comma.ins).toBe(0);
  expect(andWord.sub + andWord.del + andWord.ins).toBe(0);
  expect(ordinal.sub + ordinal.del + ordinal.ins).toBe(0);
  expect(aHundred.sub + aHundred.del + aHundred.ins).toBe(0);
});

test("normalized scoring still counts a wrong digit", () => {
  const errors = wordErrorsNormalized("4481", "4482");
  expect(errors).toEqual({ sub: 1, del: 0, ins: 0, refWords: 1 });
});

test("raw word errors keep counting spelled-out numbers as errors", () => {
  const errors = wordErrors("4481", "four thousand four hundred eighty one");
  expect(errors.sub + errors.del + errors.ins).toBeGreaterThan(0);
});

test("adjacent numbers stay separate", () => {
  expect(normalizeForScoring("June 30, 2026")).toEqual(["june", "30", "2026"]);
  expect(normalizeNumbers(["four", "four", "eight", "one"])).toEqual([
    "4",
    "4",
    "8",
    "1",
  ]);
  expect(normalizeNumbers(["one", "two"])).toEqual(["1", "2"]);
  expect(normalizeNumbers(["twenty", "thirty"])).toEqual(["20", "30"]);
  const spelled = "four thousand four hundred eighty one";
  expect(normalizeForScoring(spelled)).toEqual(normalizeForScoring("4481"));
});

test("comma groups and the word second", () => {
  expect(normalizeForScoring("1,234,567")).toEqual(["1234567"]);
  expect(normalizeForScoring("4,481")).toEqual(["4481"]);
  expect(normalizeForScoring("wait a second")).toEqual(["wait", "a", "second"]);
  expect(normalizeForScoring("one second")).toEqual(["1", "second"]);
  expect(normalizeForScoring("the twenty second")).toEqual(["the", "22"]);
});
