import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

const callerWorkflows = [
  ".github/workflows/main-build.yml",
  ".github/workflows/release-please.yml",
  ".github/workflows/build-test.yml",
  ".github/workflows/pr-test-build.yml",
];

const macosReleaseScripts = ["build-updater.sh", "build-dmg.sh"];

function featureLists(text: string): string[] {
  const lists: string[] = [];
  const patterns = [
    /--features(?:=|\s+)(?:"([^"]+)"|'([^']+)'|([^\s\\']+))/g,
    /(?:^|\s)-F(?:=|\s+)(?:"([^"]+)"|'([^']+)'|([^\s\\']+))/g,
  ];
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) {
      const list = match[1] ?? match[2] ?? match[3] ?? "";
      if (list) lists.push(list);
    }
  }
  return lists;
}

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

function buildWorkflowSteps(buildYml: string): string[] {
  const stepsStart = buildYml.indexOf("\n    steps:\n");
  expect(stepsStart).toBeGreaterThan(0);
  const stepsBody = buildYml.slice(stepsStart + "\n    steps:\n".length);
  return stepsBody
    .split(/\n(?= {6}- (?:name|uses): )/)
    .map((block) => block.trimEnd())
    .filter(Boolean);
}

function stepCondition(stepBlock: string): string {
  const match = stepBlock.match(/^\s*if:\s*(.+)$/m);
  return match?.[1]?.trim() ?? "";
}

test("macos_release_builds_require_meetings_and_plist", () => {
  const buildYml = readFileSync(
    join(root, ".github/workflows/build.yml"),
    "utf8",
  );
  const steps = buildWorkflowSteps(buildYml);

  const viteMeetingsSteps = steps.filter((step) =>
    step.includes("VITE_MEETINGS=1"),
  );
  expect(viteMeetingsSteps.length).toBe(1);
  expect(stepCondition(viteMeetingsSteps[0])).toBe(
    "contains(inputs.platform, 'macos')",
  );

  const tauriStep = steps.find((step) =>
    step.includes("uses: tauri-apps/tauri-action@"),
  );
  expect(tauriStep).toBeDefined();
  expect(tauriStep).toContain(
    "args: ${{ contains(inputs.platform, 'macos') && format('--features meetings --config src-tauri/tauri.meetings.conf.json {0}', inputs.build-args) || inputs.build-args }}",
  );

  for (const script of macosReleaseScripts) {
    const text = readFileSync(join(root, script), "utf8");
    expect(text, `${script} must set VITE_MEETINGS=1`).toContain(
      "VITE_MEETINGS=1",
    );
    expect(
      featureLists(text).some((list) =>
        list
          .split(",")
          .map((item) => item.trim())
          .includes("meetings"),
      ),
      `${script} must pass --features meetings`,
    ).toBe(true);
    expect(
      text,
      `${script} must pass src-tauri/tauri.meetings.conf.json`,
    ).toContain("src-tauri/tauri.meetings.conf.json");
  }

  for (const file of callerWorkflows) {
    const text = readFileSync(join(root, file), "utf8");
    expect(text, `${file} must include macOS targets`).toContain("macos-");
    expect(text, `${file} must delegate to build.yml`).toContain(
      "uses: ./.github/workflows/build.yml",
    );
  }
});

test("windows_and_linux_release_builds_forbid_meetings_and_plist", () => {
  const hits: string[] = [];

  for (const file of callerWorkflows) {
    const text = readFileSync(join(root, file), "utf8");
    for (const hit of enablesMeetings(text)) {
      hits.push(`${file}: ${hit}`);
    }
  }

  const buildYml = readFileSync(
    join(root, ".github/workflows/build.yml"),
    "utf8",
  );
  for (const step of buildWorkflowSteps(buildYml)) {
    const cond = stepCondition(step);
    const macosOnly = cond === "contains(inputs.platform, 'macos')";
    if (macosOnly) continue;

    if (step.includes("uses: tauri-apps/tauri-action@")) {
      const [envAndHeader, withPart = ""] = step.split(/\n\s*with:\n/);
      for (const hit of enablesMeetings(envAndHeader)) {
        hits.push(`.github/workflows/build.yml (tauri-action env): ${hit}`);
      }
      const argsLine = withPart.match(/^\s*args:\s*(.+)$/m)?.[1]?.trim() ?? "";
      const fallback = argsLine.split("||")[1]?.trim() ?? "";
      if (fallback !== "inputs.build-args }}") {
        hits.push(
          `.github/workflows/build.yml (non-macOS args fallback): ${fallback}`,
        );
      }
      const withoutArgs = withPart.replace(/^\s*args:\s*.+$/m, "");
      for (const hit of enablesMeetings(withoutArgs)) {
        hits.push(`.github/workflows/build.yml (tauri-action with): ${hit}`);
      }
      continue;
    }

    for (const hit of enablesMeetings(step)) {
      const name = step.match(/^\s*-\s*name:\s*(.+)$/m)?.[1] ?? "unnamed step";
      hits.push(`.github/workflows/build.yml (${name}): ${hit}`);
    }
  }

  const cargoToml = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
  if (defaultFeaturesIncludeMeetings(cargoToml)) {
    hits.push("src-tauri/Cargo.toml: meetings is in default features");
  }

  const macosTargetTable =
    cargoToml
      .split(/^\[target\.'cfg\(target_os = "macos"\)'\.dependencies\]\s*$/m)[1]
      ?.split(/^\[/m)[0] ?? "";
  expect(macosTargetTable).toContain("cidre =");
  const cargoWithoutMacos = cargoToml.replace(macosTargetTable, "");
  if (/^\s*cidre\s*=/m.test(cargoWithoutMacos)) {
    hits.push("src-tauri/Cargo.toml: cidre enabled outside macOS target");
  }

  expect(hits).toEqual([]);
});

test("macos_floor_stays_14_4", () => {
  const cargoToml = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
  expect(cargoToml).toContain('"macos_14_4"');

  const settingsRs = readFileSync(
    join(root, "src-tauri/src/settings.rs"),
    "utf8",
  );
  expect(settingsRs).toContain("macos_at_least_14_4(os, version)");
  expect(settingsRs).toMatch(
    /fn default_meetings_enabled\(\)\s*->\s*bool\s*\{\s*false\s*\}/,
  );
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

const calendarUsage =
  "Coco reads your calendar on this Mac to show upcoming meetings. It never changes your calendar or sends it anywhere.";

test("meetings_plist_has_exact_calendar_usage_strings", () => {
  const plist = readFileSync(
    join(root, "src-tauri/Info.meetings.plist"),
    "utf8",
  );
  expect(plistString(plist, "NSCalendarsFullAccessUsageDescription")).toBe(
    calendarUsage,
  );
  expect(plistString(plist, "NSCalendarsUsageDescription")).toBe(calendarUsage);
});

test("default_info_plist_has_no_calendar_keys", () => {
  const plist = readFileSync(join(root, "src-tauri/Info.plist"), "utf8");
  expect(plist).not.toContain("NSCalendars");
});
