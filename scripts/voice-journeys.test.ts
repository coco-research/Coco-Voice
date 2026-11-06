import { describe, expect, test } from "bun:test";
import { parseCargoTestOutput, summarize } from "./voice-journeys.ts";

const journey = (id: string, tests: string[]) => ({
  id,
  journey: `journey ${id}`,
  check: "fixture",
  tests,
});

const cargo = (...lines: string[]) => lines.join("\n");

describe("parse + summarize", () => {
  test("all tests ok passes", () => {
    const results = parseCargoTestOutput(
      cargo(
        "test input::tests::alpha ... ok",
        "test input::tests::beta ... ok",
        "test result: ok. 2 passed; 0 failed; 0 ignored",
      ),
    );
    const s = summarize([journey("V1", ["alpha", "beta"])], results);
    expect(s.pass).toBe(1);
    expect(s.failing).toEqual([]);
  });

  test("one FAILED test fails the journey", () => {
    const results = parseCargoTestOutput(
      cargo(
        "test input::tests::alpha ... ok",
        "test input::tests::beta ... FAILED",
      ),
    );
    const s = summarize([journey("V2", ["alpha", "beta"])], results);
    expect(s.pass).toBe(0);
    expect(s.failing).toEqual(["V2"]);
  });

  test("missing test fails", () => {
    const results = parseCargoTestOutput(
      cargo("test input::tests::alpha ... ok"),
    );
    const s = summarize([journey("V3", ["alpha", "not_run"])], results);
    expect(s.pass).toBe(0);
    expect(s.failing).toEqual(["V3"]);
  });

  test("empty tests fails", () => {
    const s = summarize([journey("V4", [])], parseCargoTestOutput(""));
    expect(s.pass).toBe(0);
    expect(s.failing).toEqual(["V4"]);
  });

  test("pass plus failing count equals total", () => {
    const results = parseCargoTestOutput(
      cargo("test a ... ok", "test b ... FAILED", "test c ... ignored"),
    );
    const s = summarize(
      [
        journey("V5", ["a"]),
        journey("V6", ["b"]),
        journey("V7", ["c"]),
        journey("V8", []),
      ],
      results,
    );
    expect(s.pass).toBe(1);
    expect(s.failing).toEqual(["V6", "V7", "V8"]);
    expect(s.pass + s.failing.length).toBe(s.total);
    expect(s.total).toBe(4);
  });

  test("ts carries an offset", () => {
    const s = summarize([journey("V9", [])], parseCargoTestOutput(""));
    expect(s.ts).toMatch(/[+-]\d{2}:\d{2}$/);
    expect(Number.isNaN(Date.parse(s.ts))).toBe(false);
  });
});
