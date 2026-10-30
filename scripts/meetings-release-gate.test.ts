import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

const releaseFiles = [
  ".github/workflows/build.yml",
  ".github/workflows/main-build.yml",
  ".github/workflows/release-please.yml",
  ".github/workflows/build-test.yml",
  ".github/workflows/pr-test-build.yml",
  "build-updater.sh",
  "build-dmg.sh",
];

function featureLists(text: string): string[] {
  const lists: string[] = [];
  const patterns = [
    /--features(?:=|\s+)(?:"([^"]+)"|'([^']+)'|([^\s\\]+))/g,
    /(?:^|\s)-F(?:=|\s+)(?:"([^"]+)"|'([^']+)'|([^\s\\]+))/g,
  ];
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) {
      const list = match[1] ?? match[2] ?? match[3] ?? "";
      if (list) lists.push(list);
    }
  }
  return lists;
}

function enablesMeetings(text: string): string[] {
  const hits: string[] = [];
  if (text.includes("VITE_MEETINGS")) hits.push("sets VITE_MEETINGS");
  for (const list of featureLists(text)) {
    const names = list.split(",").map((name) => name.trim());
    if (names.includes("meetings")) hits.push(list);
  }
  return hits;
}

function defaultFeaturesIncludeMeetings(cargoToml: string): boolean {
  const after = cargoToml.split(/^\[features\]\s*$/m)[1];
  if (!after) return false;
  const body = after.split(/^\[/m)[0] ?? "";
  for (const match of body.matchAll(/^\s*default\s*=\s*\[([\s\S]*?)\]/gm)) {
    const names = [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
    if (names.includes("meetings")) return true;
  }
  return false;
}

test("release_workflows_do_not_enable_meetings", () => {
  const hits: string[] = [];
  for (const file of releaseFiles) {
    const text = readFileSync(join(root, file), "utf8");
    for (const hit of enablesMeetings(text)) {
      hits.push(`${file}: ${hit}`);
    }
  }

  const cargoToml = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
  if (defaultFeaturesIncludeMeetings(cargoToml)) {
    hits.push("src-tauri/Cargo.toml: meetings is in default features");
  }

  expect(hits).toEqual([]);
});
