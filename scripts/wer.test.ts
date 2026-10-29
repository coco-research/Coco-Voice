import { expect, test } from "bun:test";
import { normalize, wer, wordErrors } from "./wer.ts";

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
