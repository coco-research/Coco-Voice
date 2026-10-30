import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

// PR6a: the consent copy is checked in the file the dialog actually reads, byte
// for byte, so a "helpful" rewording fails here instead of shipping.
const localesDir = join(import.meta.dir, "..", "src", "i18n", "locales");
const englishFile = join(localesDir, "en", "translation.json");

// Exact wording from mock 2 / lab-0394.
const REMINDER =
  "In many places, including California, Florida, Illinois, Maryland, Massachusetts, Pennsylvania and Washington, everyone in the conversation must agree to being recorded. Tell participants before you start.";
const ANNOUNCEMENT =
  "I'm taking notes with an app that records and transcribes on my computer. Tell me if you'd rather I didn't.";
const ORG_RULES =
  "Check your organisation's rules before using this in work meetings.";

// Claims the dialog must never make.
const BANNED = [
  "consent-compliant",
  "BIPA-compliant",
  "legal in all states",
  "private by law",
];

test("consent copy matches the approved wording", () => {
  const english = readFileSync(englishFile, "utf8");
  for (const line of [REMINDER, ANNOUNCEMENT, ORG_RULES]) {
    // JSON.stringify adds the surrounding quotes and escapes exactly what the
    // translation file holds, so this is a byte comparison, not a substring fuzz.
    expect(english).toContain(JSON.stringify(line));
  }
});

test("no locale claims the recording is legal or private", () => {
  for (const entry of readdirSync(localesDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const file = join(localesDir, entry.name, "translation.json");
    const text = readFileSync(file, "utf8").toLowerCase();
    for (const phrase of BANNED) {
      expect(text).not.toContain(phrase.toLowerCase());
    }
  }
});
