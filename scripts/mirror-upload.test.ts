import { expect, test } from "bun:test";
import { runUpload, type SpawnFn } from "./mirror-upload.ts";

test("token is never printed, passed, or left in errors", async () => {
  const secret = "hf_super_secret_token_value";
  const calls: string[][] = [];
  const stdout: string[] = [];
  const stderr: string[] = [];
  const spawn: SpawnFn = async (command, args) => {
    calls.push([command, ...args]);
    return { code: 1, stdout: "", stderr: `auth failed ${secret}` };
  };
  const code = await runUpload(["Qwen/Qwen3-4B-GGUF"], {
    env: { HF_TOKEN: secret },
    stdout: (line) => stdout.push(line),
    stderr: (line) => stderr.push(line),
    spawn,
    fetch: async () => {
      throw new Error("fetch should not run");
    },
  });
  const printed = `${stdout.join("\n")}\n${stderr.join("\n")}`;
  const argv = JSON.stringify(calls);
  expect(code).not.toBe(0);
  expect(argv).not.toContain(secret);
  expect(argv).not.toContain("--token");
  expect(printed).not.toContain(secret);
  expect(printed).toContain("[redacted]");
});

test("an entry with no sha256 is refused before any spawn", async () => {
  const calls: string[] = [];
  const spawn: SpawnFn = async (_command, args) => {
    calls.push(args[0] ?? "");
    return { code: 0, stdout: "", stderr: "" };
  };
  const id = "handy-computer/example-gguf";
  const stderr: string[] = [];
  const code = await runUpload([id], {
    env: { HF_TOKEN: "hf_test_token_value" },
    stdout: () => {},
    stderr: (line) => stderr.push(line),
    spawn,
    fetch: async () => {
      throw new Error("fetch should not run");
    },
    catalogText: JSON.stringify({
      models: [
        {
          id,
          license: "cc-by-4.0",
          source_repo: id,
          revision: "c".repeat(40),
          default_quant: "Q8_0",
          files: [{ filename: "model.gguf", quant: "Q8_0" }],
        },
      ],
    }),
    licencesText: "{}",
    modRsText: "pub const LEGAL_HOLD_IDS: &[&str] = &[];\n",
  });
  expect(code).not.toBe(0);
  expect(calls).toEqual([]);
  expect(stderr.join("\n")).toContain("no sha256");
});
