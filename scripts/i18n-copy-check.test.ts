import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = join(import.meta.dir, "..");
const LOCALES_DIR = join(ROOT, "src", "i18n", "locales");
const PENDING_PATH = join(ROOT, "scripts", "i18n-pending.json");

const NON_LATIN_LOCALES = new Set([
  "ar",
  "bg",
  "he",
  "ja",
  "ko",
  "ne",
  "ru",
  "uk",
  "zh",
  "zh-TW",
]);

/**
 * Fixed allowlist per Plan map v1.1: product/model names, key names,
 * file extensions, URLs, commands, and pure placeholders.
 */
const ALLOWED_EQUAL_TO_ENGLISH = new Set([
  "Coco Voice",
  "Coco Voice {{version}}",
  "Qwen3-4B",
  "Breeze ASR",
  "Canary 180M Flash",
  "Canary 1B v2",
  "Cohere",
  "GigaAM v3",
  "Whisper Large",
  "Whisper Medium",
  "Whisper Small",
  "Whisper Turbo",
  "Whisper Base",
  "Whisper Tiny",
  "Moonshine Base",
  "Parakeet V2",
  "Parakeet V3",
  "SenseVoice",
  "ggml",
  "Apple Intelligence",
  "GitHub",
  "Dropbox",
  "iCloud Drive",
  "Google Drive",
  "OneDrive",
  "Claude Code",
  "Cursor",
  "Codex",
  "Mail",
  "Terminal",
  "1Password",
  "Safari",
  "fn",
  "Esc",
  "⌘",
  "Cmd+Enter",
  "Ctrl+Enter",
  "Super+Enter",
  "Enter",
  "Shift+Insert",
  "Ctrl+Shift+V",
  "sk-...",
  "https://api.openai.com/v1",
  "coco-voice mcp",
  "{{ms}} ms",
  "{{model}} · {{sizeMb}} MB",
  "MB/s",
  "ms",
  "OK",
  "Audio",
  "Auto",
  "URL",
  "API",
  "MCP",
  "JSON",
  "Markdown",
  "WAV",
]);

export function isAllowedEqualToEnglish(value: string): boolean {
  const s = value.trim();
  if (!s || ALLOWED_EQUAL_TO_ENGLISH.has(s)) return true;
  if (
    /^(\{\{[^}]+\}\}|[\d.,:%/·+\-×()[\] ]|ms|s|MB|GB|KB|Hz|kHz|dB|B\/s|MB\/s)+$/.test(
      s,
    )
  ) {
    return true;
  }
  if (/^https?:\/\//.test(s) || /^\.[a-z0-9]{1,5}$/.test(s)) return true;
  return false;
}

function flatten(
  obj: Record<string, unknown>,
  prefix = "",
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(obj)) {
    const nextKey = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "object" && v !== null && !Array.isArray(v)) {
      Object.assign(out, flatten(v as Record<string, unknown>, nextKey));
    } else if (typeof v === "string") {
      out[nextKey] = v;
    }
  }
  return out;
}

export function findUntranslatedEnglishCopies(
  enFlat: Record<string, string>,
  localesFlat: Record<string, Record<string, string>>,
  pendingKeys: Set<string>,
): string[] {
  const langs = Object.keys(localesFlat).sort();
  const offenders: string[] = [];
  for (const [key, enVal] of Object.entries(enFlat)) {
    if (isAllowedEqualToEnglish(enVal) || pendingKeys.has(key)) continue;
    let nonLatinSame = false;
    let totalSame = 0;
    for (const lang of langs) {
      if (localesFlat[lang]?.[key] === enVal) {
        totalSame += 1;
        if (NON_LATIN_LOCALES.has(lang)) nonLatinSame = true;
      }
    }
    if (nonLatinSame || totalSame >= 6) {
      offenders.push(`${key} (${totalSame}/${langs.length} locales equal EN)`);
    }
  }
  return offenders.sort();
}

describe("W18 · Plan map v1.1 translation copy-check", () => {
  const enFlat = flatten(
    JSON.parse(
      readFileSync(join(LOCALES_DIR, "en", "translation.json"), "utf8"),
    ),
  );
  const langs = readdirSync(LOCALES_DIR, { withFileTypes: true })
    .filter((d) => d.isDirectory() && d.name !== "en")
    .map((d) => d.name)
    .sort();
  const localesFlat: Record<string, Record<string, string>> = {};
  for (const lang of langs) {
    localesFlat[lang] = flatten(
      JSON.parse(
        readFileSync(join(LOCALES_DIR, lang, "translation.json"), "utf8"),
      ),
    );
  }
  const pendingDoc = JSON.parse(readFileSync(PENDING_PATH, "utf8")) as {
    plans: Record<string, string[]>;
  };
  const allPendingKeys = new Set(Object.values(pendingDoc.plans).flat());

  test("Plan D has emptied its i18n-pending group", () => {
    expect(langs.length).toBe(21);
    expect(pendingDoc.plans.planD).toEqual([]);
  });

  test("no non-English locale copies English verbatim outside allowlist and i18n-pending.json", () => {
    const offenders = findUntranslatedEnglishCopies(
      enFlat,
      localesFlat,
      allPendingKeys,
    );
    expect(offenders).toEqual([]);
  });

  test("every key in scripts/i18n-pending.json exists in en and is still untranslated", () => {
    const missingFromEn: string[] = [];
    const staleTranslated: string[] = [];
    for (const key of allPendingKeys) {
      const enVal = enFlat[key];
      if (enVal === undefined) {
        missingFromEn.push(key);
        continue;
      }
      const stillCopied = findUntranslatedEnglishCopies(
        { [key]: enVal },
        localesFlat,
        new Set(),
      );
      if (stillCopied.length === 0) {
        staleTranslated.push(key);
      }
    }
    expect(missingFromEn).toEqual([]);
    expect(staleTranslated).toEqual([]);
  });

  test("detects an unlisted verbatim English copy in a non-English locale", () => {
    const syntheticEn = { "test.sample": "Hold fn to talk" };
    const syntheticLocales = {
      de: { "test.sample": "Zum Sprechen fn halten" },
      ja: { "test.sample": "Hold fn to talk" },
    };
    const caught = findUntranslatedEnglishCopies(
      syntheticEn,
      syntheticLocales,
      new Set(),
    );
    expect(caught.length).toBe(1);
    expect(caught[0]).toContain("test.sample");
  });
});
