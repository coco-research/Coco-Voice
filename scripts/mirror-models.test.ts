import { expect, spyOn, test } from "bun:test";
import * as childProcess from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  MIRROR_TARGET_ORG,
  mirrorSet,
  parseLegalHoldIds,
  refusalReason,
  runMirror,
  STAY_IDS,
  type CatalogFile,
  type MirrorRunOptions,
} from "./mirror-models.ts";

const root = join(import.meta.dirname, "..");
const catalog = JSON.parse(
  readFileSync(join(root, "src-tauri/src/catalog/catalog.json"), "utf8"),
) as CatalogFile;
const holds = parseLegalHoldIds(
  readFileSync(join(root, "src-tauri/src/catalog/mod.rs"), "utf8"),
);
const ALLOWED = new Set(["mit", "apache-2.0", "cc-by-4.0"]);

function probe() {
  const stdout: string[] = [];
  const stderr: string[] = [];
  const io: MirrorRunOptions = {
    stdout: (line: string) => stdout.push(line),
    stderr: (line: string) => stderr.push(line),
  };
  return { io, stdout, stderr };
}

test("today's catalog gives 44 models", () => {
  const set = mirrorSet(catalog, holds);
  expect(set).toHaveLength(44);
  expect(new Set(set.map((entry) => entry.targetRepo)).size).toBe(44);
  for (const entry of set) {
    expect(entry.targetRepo).toBe(
      `${MIRROR_TARGET_ORG}/${entry.sourceRepo.split("/").pop()}`,
    );
    expect(ALLOWED.has(entry.licence)).toBe(true);
    expect(entry.revision).toMatch(/^[0-9a-f]{40}$/);
    expect(entry.filename.endsWith(".gguf")).toBe(true);
  }
  for (const id of STAY_IDS) {
    expect(catalog.models.some((row) => row.id === id)).toBe(true);
    expect(set.some((entry) => entry.id === id)).toBe(false);
  }
  const canary = set.find(
    (entry) => entry.id === "handy-computer/canary-180m-flash-gguf",
  );
  expect(canary?.filename).toBe("canary-180m-flash-Q8_0.gguf");
  expect(canary?.sha256).toBeNull();
  const qwen = set.find((entry) => entry.id === "Qwen/Qwen3-4B-GGUF");
  expect(qwen?.sha256).toBe(
    "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5",
  );
  expect(qwen?.targetRepo).toBe(`${MIRROR_TARGET_ORG}/Qwen3-4B-GGUF`);
});

test("stay ids are refused", async () => {
  for (const id of STAY_IDS) {
    expect(refusalReason({ id, license: "apache-2.0" }, holds)).not.toBeNull();
    expect(mirrorSet(catalog, holds).some((entry) => entry.id === id)).toBe(
      false,
    );
  }
  const run = probe();
  expect(await runMirror([STAY_IDS[0]], run.io)).not.toBe(0);
});

test("a held id is refused", async () => {
  const held = "handy-computer/medasr-gguf";
  expect(holds).toContain(held);
  expect(refusalReason({ id: held, license: "mit" }, holds)).toBe(
    `refusing ${held}: legal hold`,
  );
  const run = probe();
  const code = await runMirror([held], {
    ...run.io,
    catalogText: JSON.stringify({
      models: [
        {
          id: held,
          license: "mit",
          source_repo: held,
          revision: "a".repeat(40),
          default_quant: "Q8_0",
          files: [{ filename: "medasr-Q8_0.gguf", quant: "Q8_0" }],
        },
      ],
    }),
  });
  expect(code).not.toBe(0);
  expect(run.stderr.join("\n")).toContain("legal hold");
});

test("a non-allowlisted licence is refused", async () => {
  const row = {
    id: "example/custom-model",
    license: "cc-by-nc-4.0",
    revision: "a".repeat(40),
    default_quant: "Q8_0",
    files: [{ filename: "custom-Q8_0.gguf", quant: "Q8_0" }],
  };
  expect(refusalReason(row, [])).toBe(
    "refusing example/custom-model: licence cc-by-nc-4.0 is not allowlisted",
  );
  expect(mirrorSet({ models: [row] }, [])).toEqual([]);
  const run = probe();
  const code = await runMirror(["example/custom-model"], {
    ...run.io,
    catalogText: JSON.stringify({ models: [row] }),
    modRsText: "pub const LEGAL_HOLD_IDS: &[&str] = &[];\n",
  });
  expect(code).not.toBe(0);
});

test("a row with no pinned revision is refused", async () => {
  const row = {
    id: "example/unpinned",
    license: "mit",
    default_quant: "Q8_0",
    files: [{ filename: "unpinned-Q8_0.gguf", quant: "Q8_0" }],
  };
  expect(refusalReason(row, [])).toBe(
    "refusing example/unpinned: no pinned 40-hex revision",
  );
  expect(mirrorSet({ models: [row] }, [])).toEqual([]);
  const run = probe();
  const code = await runMirror(["example/unpinned"], {
    ...run.io,
    catalogText: JSON.stringify({ models: [row] }),
    modRsText: "pub const LEGAL_HOLD_IDS: &[&str] = &[];\n",
  });
  expect(code).not.toBe(0);
  expect(run.stdout).toEqual([]);
  expect(run.stderr.join("\n")).toBe(
    "refusing example/unpinned: no pinned 40-hex revision",
  );
});

test("a row with no default-quant file is refused", async () => {
  const row = {
    id: "example/no-file",
    license: "apache-2.0",
    revision: "b".repeat(40),
    default_quant: "Q8_0",
    files: [{ filename: "other-Q4_0.gguf", quant: "Q4_0" }],
  };
  expect(refusalReason(row, [])).toBe(
    "refusing example/no-file: no default-quant file",
  );
  expect(mirrorSet({ models: [row] }, [])).toEqual([]);
  const run = probe();
  const code = await runMirror(["example/no-file"], {
    ...run.io,
    catalogText: JSON.stringify({ models: [row] }),
    modRsText: "pub const LEGAL_HOLD_IDS: &[&str] = &[];\n",
  });
  expect(code).not.toBe(0);
  expect(run.stdout).toEqual([]);
  expect(run.stderr.join("\n")).toBe(
    "refusing example/no-file: no default-quant file",
  );
});

test("LEGAL_HOLD_IDS parses to 20 ids", () => {
  expect(holds).toHaveLength(20);
  expect(new Set(holds).size).toBe(20);
  expect(holds).toContain("handy-computer/Fun-ASR-Nano-2512-gguf");
  expect(holds).toContain("handy-computer/medasr-gguf");
});

test("an id missing from the catalog is refused without a hold check", async () => {
  const held = "handy-computer/medasr-gguf";
  const run = probe();
  const code = await runMirror([held], {
    ...run.io,
    catalogText: JSON.stringify({ models: [] }),
    modRsText: `pub const LEGAL_HOLD_IDS: &[&str] = &["${held}"];\n`,
  });
  expect(code).not.toBe(0);
  expect(run.stderr.join("\n")).toBe(`refusing ${held}: not in the catalog`);
});

test("dry run calls no network or child process", async () => {
  const run = probe();
  const realFetch = globalThis.fetch;
  const spawn = spyOn(childProcess, "spawn").mockImplementation(() => {
    throw new Error("dry run spawned a child process");
  });
  globalThis.fetch = () => {
    throw new Error("dry run touched the network");
  };
  try {
    expect(await runMirror([], run.io)).toBe(0);
    expect(run.stdout).toHaveLength(44);
    expect(run.stdout[0]).toMatch(
      new RegExp(
        `^[^ ]+@[0-9a-f]{40} \\S+ (missing|[0-9a-f]{64}) ${MIRROR_TARGET_ORG}/\\S+$`,
      ),
    );
  } finally {
    globalThis.fetch = realFetch;
    spawn.mockRestore();
  }
});

test("--token on the command line is refused", async () => {
  const secret = "hf_command_line_secret";
  const run = probe();
  expect(await runMirror(["--token", secret], run.io)).not.toBe(0);
  expect(run.stderr.join("\n")).not.toContain(secret);
});
