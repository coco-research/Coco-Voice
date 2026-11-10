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

// The system-audio usage key lives only in the meetings plist; release builds must not pull it in.
const meetingsPlistRefs = [
  "tauri.meetings.conf",
  "Info.meetings.plist",
  "infoPlist",
];

function enablesMeetings(text: string): string[] {
  const hits: string[] = [];
  if (text.includes("VITE_MEETINGS")) hits.push("sets VITE_MEETINGS");
  for (const ref of meetingsPlistRefs) {
    if (text.includes(ref)) hits.push(`references ${ref}`);
  }
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

const systemAudioKey = "NSAudioCaptureUsageDescription";

function plistString(plist: string, key: string): string | undefined {
  const match = plist.match(
    new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`),
  );
  return match?.[1];
}

test("default_info_plist_has_no_system_audio_key", () => {
  const plist = readFileSync(join(root, "src-tauri/Info.plist"), "utf8");
  expect(plist).not.toContain(systemAudioKey);
  expect(plistString(plist, "NSMicrophoneUsageDescription")).toBeTruthy();

  const config = JSON.parse(
    readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
  );
  expect(config.bundle?.macOS?.infoPlist).toBeUndefined();
});

test("meetings_config_adds_system_audio_key", () => {
  const config = JSON.parse(
    readFileSync(join(root, "src-tauri/tauri.meetings.conf.json"), "utf8"),
  );
  expect(config.bundle.macOS.infoPlist).toBe("Info.meetings.plist");

  const plist = readFileSync(
    join(root, "src-tauri/Info.meetings.plist"),
    "utf8",
  );
  expect(plistString(plist, systemAudioKey)?.trim()).toBeTruthy();
  expect(
    plistString(plist, "NSMicrophoneUsageDescription")?.trim(),
  ).toBeTruthy();
});
