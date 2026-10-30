import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const sentinel = "coco-meetings-ui-v1";

function filesIn(dir: string): string[] {
  const found: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      found.push(...filesIn(path));
    } else {
      found.push(path);
    }
  }
  return found;
}

test("production build omits the meetings ui sentinel", () => {
  const outDir = mkdtempSync(join(tmpdir(), "coco-meetings-gate-"));
  const env = { ...process.env };
  delete env.VITE_MEETINGS;
  try {
    const result = spawnSync(
      "bun",
      ["vite", "build", "--outDir", outDir, "--emptyOutDir"],
      { cwd: root, env, encoding: "utf8" },
    );
    expect(result.status, result.stderr || result.stdout).toBe(0);
    const hits = filesIn(outDir).filter((file) =>
      readFileSync(file).includes(Buffer.from(sentinel)),
    );
    expect(hits).toEqual([]);
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
}, 180_000);
