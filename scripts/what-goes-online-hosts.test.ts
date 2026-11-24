import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  ONLINE_CLICK_ROWS,
  ONLINE_CLOUD_PROVIDERS,
  ONLINE_CUSTOM_HOST,
  ONLINE_OPEN_HOSTS,
} from "../src/components/settings/about/WhatGoesOnline";
import { loadAllowlist } from "./privacy-scan";

const ROOT = join(import.meta.dir, "..");

function walkFiles(dir: string, exts: readonly string[]): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    if (
      entry === "node_modules" ||
      entry === "target" ||
      entry === "dist" ||
      entry === ".git"
    ) {
      continue;
    }
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      out.push(...walkFiles(full, exts));
    } else if (exts.some((ext) => entry.endsWith(ext))) {
      out.push(full);
    }
  }
  return out;
}

function stripRustTestModules(source: string): string {
  const idx = source.indexOf("#[cfg(test)]");
  return idx === -1 ? source : source.slice(0, idx);
}

function extractHttpsHosts(text: string): string[] {
  const hosts: string[] = [];
  const matches = text.matchAll(/https:\/\/([a-zA-Z0-9.-]+)/g);
  for (const m of matches) {
    const host = m[1].toLowerCase().replace(/\.$/, "");
    if (!host || host === "localhost") continue;
    if (
      host.endsWith(".example") ||
      host === "example.com" ||
      host === "example.invalid"
    ) {
      continue;
    }
    hosts.push(host);
  }
  return hosts;
}

describe("What goes online static host & cloud provider parity (AC-D15)", () => {
  test("ONLINE_CLOUD_PROVIDERS matches every HTTPS cloud provider base_url in settings.rs", () => {
    const settingsRs = stripRustTestModules(
      readFileSync(join(ROOT, "src-tauri/src/settings.rs"), "utf8"),
    );
    const baseUrlHosts = [
      ...settingsRs.matchAll(/base_url:\s*"https:\/\/([^/"]+)/g),
    ]
      .map((m) => m[1].toLowerCase())
      .sort();

    const uiCloudHosts = ONLINE_CLOUD_PROVIDERS.map((p) =>
      p.host.toLowerCase(),
    ).sort();

    expect(uiCloudHosts).toEqual(baseUrlHosts);
    expect(ONLINE_CLOUD_PROVIDERS.length).toBe(baseUrlHosts.length);
  });

  test("every non-test HTTPS host literal in src-tauri/src and src is disclosed in WhatGoesOnline or privacy-allowlist", () => {
    const allowlistHosts = new Set(
      loadAllowlist().map((entry) => entry.host.toLowerCase()),
    );
    const disclosedRuntimeHosts = new Set<string>([
      ...ONLINE_OPEN_HOSTS.map((h) => h.toLowerCase()),
      ...ONLINE_CLICK_ROWS.map((r) => r.host.toLowerCase()),
      ...ONLINE_CLOUD_PROVIDERS.map((p) => p.host.toLowerCase()),
      ONLINE_CUSTOM_HOST.toLowerCase(),
    ]);

    const files = [
      ...walkFiles(join(ROOT, "src-tauri/src"), [".rs"]),
      ...walkFiles(join(ROOT, "src"), [".ts", ".tsx"]).filter(
        (p) => !p.includes(".test.") && !p.endsWith("bindings.ts"),
      ),
    ];

    const unknown: Array<{ file: string; host: string }> = [];
    for (const file of files) {
      let content = readFileSync(file, "utf8");
      if (file.endsWith(".rs")) {
        content = stripRustTestModules(content);
      }
      // Ignore single-line doc/source comments that cite upstream papers/repos
      const codeOnly = content
        .split("\n")
        .filter((line) => !line.trimStart().startsWith("//"))
        .join("\n");

      for (const host of extractHttpsHosts(codeOnly)) {
        if (!allowlistHosts.has(host) && !disclosedRuntimeHosts.has(host)) {
          unknown.push({
            file: file.slice(ROOT.length + 1),
            host,
          });
        }
      }
    }

    expect(unknown).toEqual([]);
  });
});
