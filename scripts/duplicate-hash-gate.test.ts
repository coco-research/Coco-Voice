import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const HARNESS =
  process.env.CAPTURE_HARNESS_PATH ||
  path.resolve(import.meta.dir, "duplicate-hash-gate.mjs");

describe("Duplicate-hash gate (duplicate-hash-gate.mjs)", () => {
  test("exits non-zero and prints colliding pair when two files have the same sha256", () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "dup-hash-gate-"));
    try {
      const a = path.join(tmp, "shot-12-light.png");
      const b = path.join(tmp, "shot-26-light.png");
      const bytes = Buffer.from(
        "identical-png-payload-for-duplicate-gate-test",
      );
      fs.writeFileSync(a, bytes);
      fs.writeFileSync(b, bytes);

      const res = spawnSync(
        process.execPath,
        [HARNESS, "--check-duplicates", a, b],
        {
          encoding: "utf8",
        },
      );
      expect(res.status).not.toBe(0);
      expect(res.stderr).toContain("DUPLICATE_HASH_IN_CHECKPOINT");
      expect(res.stderr).toContain("shot-12-light.png == shot-26-light.png");
      expect(res.stderr).toContain("sha256=");
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  test("exits zero when all files have distinct sha256 hashes", () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "dup-hash-gate-ok-"));
    try {
      const a = path.join(tmp, "shot-12-light.png");
      const b = path.join(tmp, "shot-26-light.png");
      fs.writeFileSync(a, Buffer.from("frame-3-recording-indicator"));
      fs.writeFileSync(b, Buffer.from("frame-16a-notes-so-far"));

      const res = spawnSync(
        process.execPath,
        [HARNESS, "--check-duplicates", a, b],
        {
          encoding: "utf8",
        },
      );
      expect(res.status).toBe(0);
      expect(res.stdout).toContain("DUPLICATE_HASH_GATE_OK");
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  test("findDuplicateHashes detects both within-checkpoint and cross-checkpoint collisions", async () => {
    const mod = await import(HARNESS);
    const report = mod.findDuplicateHashes([
      { checkpoint: "B1-1", theme: "light", id: "12", sha256: "aaa" },
      { checkpoint: "B1-1", theme: "light", id: "11", sha256: "aaa" },
      { checkpoint: "B1-2", theme: "light", id: "14", sha256: "bbb" },
      { checkpoint: "X-2", theme: "light", id: "40b", sha256: "bbb" },
    ]);
    expect(report.withinCheckpoint.length).toBe(1);
    expect(report.withinCheckpoint[0].first).toBe("12");
    expect(report.withinCheckpoint[0].second).toBe("11");
    expect(report.acrossCheckpoints.length).toBe(1);
    expect(report.acrossCheckpoints[0].first).toBe("B1-2/14");
    expect(report.acrossCheckpoints[0].second).toBe("X-2/40b");
  });
});
