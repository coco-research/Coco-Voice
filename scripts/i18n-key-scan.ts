import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

export const PLURAL_SUFFIXES = [
  "_zero",
  "_one",
  "_two",
  "_few",
  "_many",
  "_other",
] as const;

export const SPEAKER_SPLIT_FILES = new Set([
  "src/components/meetings/SpeakerSplitSection.tsx",
  "src/components/settings/about/AboutSettings.tsx",
]);

export interface StaticI18nCall {
  file: string;
  key: string;
  hasDefault: boolean;
}

export function hasKeyPath(
  obj: Record<string, unknown>,
  keyPath: string,
): boolean {
  let cur: unknown = obj;
  for (const part of keyPath.split(".")) {
    if (typeof cur !== "object" || cur === null || !(part in cur)) {
      return false;
    }
    cur = (cur as Record<string, unknown>)[part];
  }
  return cur !== undefined;
}

export function hasKeyOrPluralForms(
  obj: Record<string, unknown>,
  keyPath: string,
): boolean {
  if (hasKeyPath(obj, keyPath)) return true;
  const parts = keyPath.split(".");
  let parent: unknown = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const part = parts[i]!;
    if (typeof parent !== "object" || parent === null || !(part in parent)) {
      return false;
    }
    parent = (parent as Record<string, unknown>)[part];
  }
  if (typeof parent !== "object" || parent === null) return false;
  const leaf = parts[parts.length - 1]!;
  const rec = parent as Record<string, unknown>;
  return PLURAL_SUFFIXES.some((suffix) => `${leaf}${suffix}` in rec);
}

function walkFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir).sort()) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      out.push(...walkFiles(full));
    } else {
      out.push(full);
    }
  }
  return out;
}

function extractBalancedCall(text: string, openParenIndex: number): string {
  let depth = 0;
  let quote: string | null = null;
  let escaped = false;
  for (let i = openParenIndex; i < text.length; i++) {
    const ch = text[i]!;
    if (escaped) {
      escaped = false;
      continue;
    }
    if (quote !== null) {
      if (ch === "\\") {
        escaped = true;
      } else if (ch === quote) {
        quote = null;
      }
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
    } else if (ch === "(") {
      depth++;
    } else if (ch === ")") {
      depth--;
      if (depth === 0) {
        return text.slice(openParenIndex + 1, i);
      }
    }
  }
  return text.slice(openParenIndex + 1);
}

function secondArgHasDefault(argsBody: string): boolean {
  let depthParen = 0;
  let depthBrace = 0;
  let depthBracket = 0;
  let quote: string | null = null;
  let escaped = false;
  let commaIdx = -1;

  for (let i = 0; i < argsBody.length; i++) {
    const ch = argsBody[i]!;
    if (escaped) {
      escaped = false;
      continue;
    }
    if (quote !== null) {
      if (ch === "\\") {
        escaped = true;
      } else if (ch === quote) {
        quote = null;
      }
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
    } else if (ch === "(") {
      depthParen++;
    } else if (ch === ")") {
      depthParen--;
    } else if (ch === "{") {
      depthBrace++;
    } else if (ch === "}") {
      depthBrace--;
    } else if (ch === "[") {
      depthBracket++;
    } else if (ch === "]") {
      depthBracket--;
    } else if (
      ch === "," &&
      depthParen === 0 &&
      depthBrace === 0 &&
      depthBracket === 0
    ) {
      commaIdx = i;
      break;
    }
  }

  if (commaIdx === -1) return false;
  const rest = argsBody.slice(commaIdx + 1).trim();
  if (rest.length === 0) return false;
  if (rest.startsWith('"') || rest.startsWith("'") || rest.startsWith("`")) {
    return true;
  }
  if (rest.startsWith("{")) {
    return /\bdefaultValue\s*[:},]/.test(rest);
  }
  return false;
}

export function scanStaticI18nCalls(rootDir: string): StaticI18nCall[] {
  const srcDir = join(rootDir, "src");
  const calls: StaticI18nCall[] = [];

  for (const file of walkFiles(srcDir)) {
    const rel = relative(rootDir, file).replaceAll("\\", "/");
    if (rel.startsWith("src/bindings") || !/\.(ts|tsx)$/.test(rel)) continue;
    const text = readFileSync(file, "utf8");

    for (const m of text.matchAll(
      /(?<![A-Za-z0-9_])t\(\s*["']([A-Za-z0-9_.-]+)["']/g,
    )) {
      const openIdx = text.indexOf("(", m.index!);
      const argsBody = extractBalancedCall(text, openIdx);
      calls.push({
        file: rel,
        key: m[1]!,
        hasDefault: secondArgHasDefault(argsBody),
      });
    }

    for (const m of text.matchAll(/i18nKey=\s*["']([A-Za-z0-9_.-]+)["']/g)) {
      calls.push({
        file: rel,
        key: m[1]!,
        hasDefault: false,
      });
    }
  }

  return calls;
}

export function findMissingStaticI18nKeys(
  rootDir: string,
  options: { onlyWithoutDefault: boolean },
): string[] {
  const en = JSON.parse(
    readFileSync(join(rootDir, "src/i18n/locales/en/translation.json"), "utf8"),
  ) as Record<string, unknown>;
  const speakerSplitEn = JSON.parse(
    readFileSync(join(rootDir, "src/i18n/speaker-split/en.json"), "utf8"),
  ) as Record<string, unknown>;

  const missing: string[] = [];
  for (const call of scanStaticI18nCalls(rootDir)) {
    if (options.onlyWithoutDefault && call.hasDefault) continue;
    if (hasKeyOrPluralForms(en, call.key)) continue;
    if (
      SPEAKER_SPLIT_FILES.has(call.file) &&
      hasKeyOrPluralForms(speakerSplitEn, call.key)
    ) {
      continue;
    }
    missing.push(`${call.file}: ${call.key}`);
  }
  return missing;
}
