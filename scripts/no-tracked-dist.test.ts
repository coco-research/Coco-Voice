import { describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const REPO_ROOT = path.resolve(import.meta.dir, "..");
const DIST_TRACKED_PATTERN = /^dist($|[-/])/;

describe("no-tracked-dist guard", () => {
  test("git tracks zero files under dist/ or dist-*/", () => {
    const out = execFileSync("git", ["-C", REPO_ROOT, "ls-files"], {
      encoding: "utf8",
    });
    const tracked = out
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && DIST_TRACKED_PATTERN.test(line));
    expect(tracked).toEqual([]);
  });

  test(".gitignore ignores both dist and dist-*/", () => {
    const ignoreLines = fs
      .readFileSync(path.join(REPO_ROOT, ".gitignore"), "utf8")
      .split(/\r?\n/)
      .map((line) => line.trim());
    expect(ignoreLines).toContain("dist");
    expect(ignoreLines).toContain("dist-*/");
  });
});
