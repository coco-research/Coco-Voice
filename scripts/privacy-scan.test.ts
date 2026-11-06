import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  loadAllowlist,
  missingHosts,
  scanPrivacyHosts,
  staleAllowlistHosts,
  stripComments,
} from "./privacy-scan.ts";

function withTree(
  files: Record<string, string>,
  run: (root: string) => void,
): void {
  const root = mkdtempSync(join(tmpdir(), "privacy-scan-"));
  try {
    for (const [rel, body] of Object.entries(files)) {
      const path = join(root, rel);
      mkdirSync(join(path, ".."), { recursive: true });
      writeFileSync(path, body);
    }
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("a file with a new host fails the check", () => {
  withTree(
    {
      "src/app.ts": 'export const u = "https://new-host.example/x";\n',
    },
    (root) => {
      const hits = scanPrivacyHosts(root);
      expect(hits).toEqual([
        { host: "new-host.example", file: "src/app.ts", line: 1 },
      ]);
      expect(missingHosts(hits, [])).toEqual(["new-host.example"]);
    },
  );
});

test("an allowlisted host passes", () => {
  withTree(
    {
      "src/app.ts": 'export const u = "https://known.example/x";\n',
    },
    (root) => {
      const hits = scanPrivacyHosts(root);
      expect(missingHosts(hits, ["known.example"])).toEqual([]);
      expect(staleAllowlistHosts(hits, ["known.example"])).toEqual([]);
    },
  );
});

test("a comment-only line is ignored", () => {
  withTree(
    {
      "src/app.ts": [
        "// https://comment-line.example/no",
        "/// https://doc-comment.example/no",
        "//! https://inner-comment.example/no",
        "/* https://block.example/no */",
        'const keep = "https://kept.example/yes";',
        "const n = 1; // https://trailing.example/no",
        "/*",
        "https://multiline.example/no",
        "*/",
        "",
      ].join("\n"),
      "src/node_modules/pkg/index.js":
        'export const u = "https://from-modules.example/no";\n',
      "src/target/skip.rs":
        'const U: &str = "https://from-target.example/no";\n',
      "src-tauri/Cargo.toml":
        '# See https://doc.rust-lang.org/cargo/reference/manifest.html\ngit = "https://github.com/kept/crate"\n',
    },
    (root) => {
      const hits = scanPrivacyHosts(root);
      expect(hits.map((hit) => hit.host)).toEqual([
        "github.com",
        "kept.example",
      ]);
    },
  );
});

test("a rust lifetime does not hide a trailing comment", () => {
  const out = stripComments(
    `fn f() -> &'static str { "" } // https://comment.example`,
    "rust",
  );
  expect(out).not.toContain("comment.example");
});

test("a rust char literal is not a comment and the next string is kept", () => {
  const out = stripComments(
    `let c = '"'; let u = "https://kept.example";`,
    "rust",
  );
  expect(out).toContain("kept.example");
});

test("a rust lifetime keeps the real url and drops the trailing comment", () => {
  const out = stripComments(
    `let s: &'a str = "https://kept.example"; // https://comment.example`,
    "rust",
  );
  expect(out).toContain("kept.example");
  expect(out).not.toContain("comment.example");
});

test("a toml inline comment is dropped and a # inside a string is kept", () => {
  const out = stripComments('serde = "1" # https://comment.example', "toml");
  expect(out).not.toContain("comment.example");
  const kept = stripComments('url = "https://kept.example#frag"', "toml");
  expect(kept).toContain("kept.example");
});

test("a toml multi-line string keeps its url", () => {
  const basic = stripComments('a = """\n# https://kept.example\n"""', "toml");
  expect(basic).toContain("kept.example");
  const literal = stripComments("a = '''\n# https://kept.example\n'''", "toml");
  expect(literal).toContain("kept.example");
});

test("a ts trailing comment is dropped and the string is kept", () => {
  const out = stripComments(
    `const a = 'https://kept.example' // https://comment.example`,
    "other",
  );
  expect(out).toContain("kept.example");
  expect(out).not.toContain("comment.example");
});

test("this repo matches the allowlist", () => {
  const root = join(import.meta.dir, "..");
  const hits = scanPrivacyHosts(root);
  const allow = loadAllowlist();
  const hosts = allow.map((entry) => entry.host);
  expect(new Set(hosts).size).toBe(hosts.length);
  expect(missingHosts(hits, hosts)).toEqual([]);
  expect(staleAllowlistHosts(hits, hosts)).toEqual([]);
});
