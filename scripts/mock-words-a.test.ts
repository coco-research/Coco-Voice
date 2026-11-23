import { describe, expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";
import en from "../src/i18n/locales/en/translation.json";

function collectStringValues(obj: unknown, out: string[] = []): string[] {
  if (typeof obj === "string") {
    out.push(obj);
  } else if (obj && typeof obj === "object") {
    for (const val of Object.values(obj as Record<string, unknown>)) {
      collectStringValues(val, out);
    }
  }
  return out;
}

function loadMergedEnglishValues(): string[] {
  const values = collectStringValues(en);
  // Also include pending Plan A shard delta JSON if present before atomic locale merge.
  const deltaPath = path.resolve(
    import.meta.dir,
    "..",
    "..",
    "i18n-delta-ckpta-2.json",
  );
  if (fs.existsSync(deltaPath)) {
    const delta = JSON.parse(fs.readFileSync(deltaPath, "utf8")) as Record<
      string,
      string
    >;
    for (const val of Object.values(delta)) {
      if (typeof val === "string") values.push(val);
    }
  }
  return values;
}

function walkTsFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walkTsFiles(full, acc);
    } else if (/\.(ts|tsx)$/.test(entry.name)) {
      acc.push(full);
    }
  }
  return acc;
}

const APPENDIX_PATTERNS: Array<{
  section: string;
  label: string;
  pattern: RegExp;
}> = [
  // A1
  { section: "A1", label: "Welcome", pattern: /^Welcome$/ },
  {
    section: "A1",
    label: "Heading",
    pattern: /^Coco Voice turns your voice into text, on this Mac$/,
  },
  {
    section: "A1",
    label: "Subtitle",
    pattern: /^Two permissions, then one model download\. About a minute\.$/,
  },
  { section: "A1", label: "Microphone", pattern: /^Microphone$/ },
  { section: "A1", label: "To hear you", pattern: /^To hear you$/ },
  { section: "A1", label: "Allowed", pattern: /^Allowed$/ },
  { section: "A1", label: "Accessibility", pattern: /^Accessibility$/ },
  {
    section: "A1",
    label: "Accessibility desc",
    pattern: /^To type your words into other apps$/,
  },
  {
    section: "A1",
    label: "Open System Settings",
    pattern: /^Open System Settings$/,
  },
  {
    section: "A1",
    label: "Continue without typing",
    pattern: /^Continue without typing$/,
  },
  {
    section: "A1",
    label: "Clipboard hint",
    pattern:
      /^Your words go to the clipboard instead\. You can turn typing on later\.$/,
  },
  // A2
  {
    section: "A2",
    label: "Stale heading",
    pattern: /^Coco Voice is switched on, but macOS still blocks it$/,
  },
  {
    section: "A2",
    label: "Stale subtitle",
    pattern:
      /^This happens after an update: macOS keeps the old entry\. Three steps fix it:$/,
  },
  {
    section: "A2",
    label: "Step 1",
    pattern:
      /^1\. In System Settings › Privacy & Security, open the list of apps allowed to control your Mac, select Coco Voice and click minus \(−\)\.$/,
  },
  {
    section: "A2",
    label: "Step 2",
    pattern:
      /^2\. Click plus \(\+\), choose Applications → Coco Voice, switch it on\.$/,
  },
  {
    section: "A2",
    label: "Step 3",
    pattern: /^3\. Come back here\. We check again by ourselves\.$/,
  },
  // A3
  { section: "A3", label: "Speech model", pattern: /^Speech model$/ },
  {
    section: "A3",
    label: "Downloading <model>",
    pattern: /^Downloading \{\{.+\}\}$/,
  },
  {
    section: "A3",
    label: "<done> MB of <total> MB",
    pattern: /^\{\{.+\}\} MB of \{\{.+\}\} MB/,
  },
  {
    section: "A3",
    label: "about <N> seconds left",
    pattern: /about \{\{.+\}\} seconds left$/,
  },
  { section: "A3", label: "Cancel", pattern: /^Cancel$/ },
  {
    section: "A3",
    label: "Connection dropped",
    pattern:
      /^The download stopped: the connection dropped at \{\{.+\}\} MB\. Nothing was saved\.$/,
  },
  { section: "A3", label: "Try again", pattern: /^Try again$/ },
  { section: "A3", label: "Skip for now", pattern: /^Skip for now$/ },
  // A4
  { section: "A4", label: "Try it", pattern: /^Try it$/ },
  {
    section: "A4",
    label: "Hold <key> and say",
    pattern: /^Hold \{\{.+\}\} and say: "Monday, scratch that, Tuesday"$/,
  },
  {
    section: "A4",
    label: "Clean-up dropped",
    pattern:
      /^Clean-up dropped "\{\{.+\}\}"\. That is how every take is cleaned\.$/,
  },
  { section: "A4", label: "Done", pattern: /^Done$/ },
  // A5
  { section: "A5", label: "Home", pattern: /^Home$/ },
  { section: "A5", label: "Good morning", pattern: /^Good morning$/ },
  { section: "A5", label: "Meetings", pattern: /^Meetings$/ },
  { section: "A5", label: "Record a meeting", pattern: /^Record a meeting$/ },
  {
    section: "A5",
    label: "Meetings body",
    pattern:
      /^Notes and a transcript, made on this Mac\. Recording starts only when you press Start\.$/,
  },
  { section: "A5", label: "Start recording", pattern: /^Start recording$/ },
  { section: "A5", label: "Recent meetings", pattern: /^Recent meetings$/ },
  { section: "A5", label: "Notes ready", pattern: /^Notes ready$/ },
  { section: "A5", label: "All meetings arrow", pattern: /^All meetings →$/ },
  { section: "A5", label: "Voice", pattern: /^Voice$/ },
  { section: "A5", label: "Dictate anywhere", pattern: /^Dictate anywhere$/ },
  {
    section: "A5",
    label: "Dictate body",
    pattern:
      /^Hold your shortcut, speak, let go\. The words are typed where your cursor is\.$/,
  },
  { section: "A5", label: "hold to talk", pattern: /^hold to talk$/ },
  { section: "A5", label: "Change shortcut", pattern: /^Change shortcut$/ },
  { section: "A5", label: "Recent dictations", pattern: /^Recent dictations$/ },
  {
    section: "A5",
    label: "All dictations arrow",
    pattern: /^All dictations →$/,
  },
  // A6
  {
    section: "A6",
    label: "Stale AX notice title",
    pattern: /^After the update, turn on Coco Voice again in System Settings$/,
  },
  {
    section: "A6",
    label: "Stale AX notice body",
    pattern:
      /^macOS kept the old entry\. Until it is fixed, your words go to the clipboard\.$/,
  },
  { section: "A6", label: "Fix it (3 steps)", pattern: /^Fix it \(3 steps\)$/ },
  {
    section: "A6",
    label: "Shortcut unset title",
    pattern: /^Your shortcut is not set up$/,
  },
  {
    section: "A6",
    label: "Another app uses <key>",
    pattern: /^Another app uses \{\{.+\}\}\.$/,
  },
  { section: "A6", label: "Choose a shortcut", pattern: /^Choose a shortcut$/ },
  { section: "A6", label: "Typing is off", pattern: /^Typing is off$/ },
  {
    section: "A6",
    label: "Your words go to the clipboard",
    pattern: /^Your words go to the clipboard\.$/,
  },
  { section: "A6", label: "Turn typing on", pattern: /^Turn typing on$/ },
  {
    section: "A6",
    label: "Update ready: <version>",
    pattern: /^Update ready: \{\{.+\}\}$/,
  },
  {
    section: "A6",
    label: "Installs when you restart",
    pattern: /^Installs when you restart\.$/,
  },
  { section: "A6", label: "Restart now", pattern: /^Restart now$/ },
  // A7
  {
    section: "A7",
    label: "What's new in <version>",
    pattern: /^What's new in \{\{.+\}\}$/,
  },
  {
    section: "A7",
    label: "Meetings, and a new layout",
    pattern: /^Meetings, and a new layout$/,
  },
  {
    section: "A7",
    label: "What's new body",
    pattern:
      /^Record a meeting from Home\. Meetings and dictation now have their own sections\.$/,
  },
  { section: "A7", label: "Close", pattern: /^Close$/ },
  {
    section: "A7",
    label: "Rollback message",
    pattern:
      /^\{\{.+\}\} did not start, so Coco Voice went back to \{\{.+\}\}\. Your settings and history are unchanged\.$/,
  },
  { section: "A7", label: "Report it", pattern: /^Report it$/ },
];

describe("Plan A Appendix mock-words & AC-A16 literal version guard", () => {
  test("every drawn string in Plan A Appendix A1-A7 exists in English translations", () => {
    const values = loadMergedEnglishValues();
    const missing: string[] = [];

    for (const item of APPENDIX_PATTERNS) {
      const found = values.some((v) => item.pattern.test(v));
      if (!found) {
        missing.push(`${item.section}: ${item.label}`);
      }
    }

    expect(missing).toEqual([]);
  });

  test("AC-A16: zero literal '0.20.0' or '0.1.2' strings exist anywhere in src/ and Footer hides version when getVersion fails", async () => {
    const srcRoot = path.resolve(import.meta.dir, "..", "src");
    const files = walkTsFiles(srcRoot);
    const hits: string[] = [];

    for (const file of files) {
      const content = fs.readFileSync(file, "utf8");
      if (content.includes("0.20.0") || content.includes("0.1.2")) {
        hits.push(path.relative(srcRoot, file));
      }
    }

    expect(hits).toEqual([]);

    const { resolveFooterVersion } =
      await import("../src/components/footer/Footer");
    expect(
      await resolveFooterVersion(async () => {
        throw new Error("tauri unavailable");
      }),
    ).toBe("");
    expect(await resolveFooterVersion(async () => "0.20.0")).toBe("0.20.0");
  });
});
