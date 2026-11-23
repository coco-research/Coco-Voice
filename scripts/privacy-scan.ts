/**
 * List every http(s) and ws(s) host under the app source.
 * Comment-only text is ignored so a note cannot look like a new network call.
 * Running this file prints the host inventory and exits 0; it is not the gate.
 * The gate is scripts/privacy-scan.test.ts (run by `bun run test:static`), which
 * fails when a host is missing from scripts/privacy-allowlist.json.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

/** Roots the release check walks. Scripts and docs are out of scope. */
const SCAN_REL_PATHS = [
  "src",
  "src-tauri/src",
  "src-tauri/tauri.conf.json",
  "src-tauri/capabilities",
  "src-tauri/Cargo.toml",
];

const SKIP_DIRS = new Set(["node_modules", "target"]);

const URL_RE = /(?:https?|wss?):\/\/[^\s<>"'`\\)\]]+/gi;

const RUST_CHAR_RE =
  /'(?:[^'\\\n]|\\u\{[0-9a-fA-F]{1,6}\}|\\x[0-9a-fA-F]{2}|\\[nrt0\\'"])'/y;

export interface PrivacyHit {
  host: string;
  file: string;
  line: number;
}

export interface PrivacyAllowEntry {
  host: string;
  why: string;
  when: string;
  sends: string;
  files: string[];
}

/**
 * Drop comments, keeping every newline so later line numbers still match the file.
 * `//` and `/*` are comments only outside strings. `://` stays, so a URL is not
 * eaten. In toml, `#` starts a comment anywhere outside a string. In Rust, `#` is
 * an attribute and is kept, and `'` is a char literal only when it parses as one,
 * so a lifetime like `&'a` cannot swallow the rest of the line.
 */
export function stripComments(
  text: string,
  lang: "toml" | "rust" | "other",
): string {
  let out = "";
  let i = 0;

  while (i < text.length) {
    const raw = readRawString(text, i);
    if (raw) {
      out += raw.value;
      i = raw.next;
      continue;
    }

    if (
      lang === "toml" &&
      (text.startsWith('"""', i) || text.startsWith("'''", i))
    ) {
      const triple = text.slice(i, i + 3);
      const end = text.indexOf(triple, i + 3);
      const next = end === -1 ? text.length : end + 3;
      out += text.slice(i, next);
      i = next;
      continue;
    }

    const quote = text[i];
    if (quote === "'" && lang === "rust") {
      const end = endOfRustChar(text, i) ?? i + 1;
      out += text.slice(i, end);
      i = end;
      continue;
    }

    if (quote === '"' || quote === "'" || quote === "`") {
      const end = endOfString(text, i, quote);
      out += text.slice(i, end);
      i = end;
      continue;
    }

    if (
      text[i] === "/" &&
      text[i + 1] === "/" &&
      text[i - 1] !== ":" &&
      text[i - 1] !== "\\"
    ) {
      i += 2;
      while (i < text.length && text[i] !== "\n") i++;
      continue;
    }

    if (text[i] === "/" && text[i + 1] === "*") {
      i += 2;
      while (i < text.length && !(text[i] === "*" && text[i + 1] === "/")) {
        if (text[i] === "\n") out += "\n";
        i++;
      }
      if (i < text.length) i += 2;
      continue;
    }

    if (lang === "toml" && text[i] === "#") {
      while (i < text.length && text[i] !== "\n") i++;
      continue;
    }

    out += text[i];
    i++;
  }

  return out;
}

/** Walk the release roots and return one hit per URL, with file and line. */
export function scanPrivacyHosts(root: string): PrivacyHit[] {
  const hits: PrivacyHit[] = [];
  for (const rel of SCAN_REL_PATHS) {
    const path = join(root, rel);
    collect(root, path, hits);
  }
  hits.sort(
    (a, b) =>
      a.file.localeCompare(b.file) ||
      a.line - b.line ||
      a.host.localeCompare(b.host),
  );
  return hits;
}

/** Hosts the scan saw that the allowlist does not name. */
export function missingHosts(
  hits: PrivacyHit[],
  allowHosts: readonly string[],
): string[] {
  const allowed = new Set(allowHosts);
  return unique(
    hits.map((hit) => hit.host).filter((host) => !allowed.has(host)),
  );
}

/** Allowlist hosts the scan no longer sees. */
export function staleAllowlistHosts(
  hits: PrivacyHit[],
  allowHosts: readonly string[],
): string[] {
  const seen = new Set(hits.map((hit) => hit.host));
  return unique(allowHosts.filter((host) => !seen.has(host)));
}

/** Read the reviewed host list next to this script. */
export function loadAllowlist(
  path = join(import.meta.dir, "privacy-allowlist.json"),
): PrivacyAllowEntry[] {
  const parsed: unknown = JSON.parse(readFileSync(path, "utf8"));
  if (!Array.isArray(parsed)) {
    throw new Error("privacy allowlist must be an array");
  }
  return parsed.map((entry) => {
    if (!isAllowEntry(entry)) {
      throw new Error(
        "privacy allowlist entry is missing host, why, when, sends, or files",
      );
    }
    return entry;
  });
}

function collect(root: string, path: string, hits: PrivacyHit[]): void {
  let info: ReturnType<typeof statSync>;
  try {
    info = statSync(path);
  } catch {
    return;
  }

  if (info.isDirectory()) {
    for (const name of readdirSync(path)) {
      if (SKIP_DIRS.has(name)) continue;
      collect(root, join(path, name), hits);
    }
    return;
  }

  if (!info.isFile()) return;

  const text = readFileSync(path, "utf8");
  const stripped = stripComments(text, langOf(path));
  const file = relative(root, path).split(sep).join("/");
  const lines = stripped.split("\n");
  for (let index = 0; index < lines.length; index++) {
    for (const host of hostsOnLine(lines[index])) {
      hits.push({ host, file, line: index + 1 });
    }
  }
}

function hostsOnLine(line: string): string[] {
  const hosts: string[] = [];
  for (const match of line.matchAll(URL_RE)) {
    const raw = match[0].replace(/[.,;:!?]+$/g, "");
    try {
      const host = new URL(raw).hostname.toLowerCase();
      if (host) hosts.push(host);
    } catch {
      // Not a URL with a host. Leave it out rather than guess.
    }
  }
  return hosts;
}

function readRawString(
  text: string,
  start: number,
): { value: string; next: number } | null {
  if (text[start] !== "r") return null;
  let i = start + 1;
  let hashes = 0;
  while (text[i] === "#") {
    hashes++;
    i++;
  }
  if (text[i] !== '"') return null;
  i++;
  const close = `"${"#".repeat(hashes)}`;
  const end = text.indexOf(close, i);
  const next = end === -1 ? text.length : end + close.length;
  return { value: text.slice(start, next), next };
}

function langOf(path: string): "toml" | "rust" | "other" {
  if (path.endsWith(".toml")) return "toml";
  if (path.endsWith(".rs")) return "rust";
  return "other";
}

/** End of a Rust char literal at `start`, or null when `'` is a lifetime. */
function endOfRustChar(text: string, start: number): number | null {
  RUST_CHAR_RE.lastIndex = start;
  const match = RUST_CHAR_RE.exec(text);
  return match ? start + match[0].length : null;
}

function endOfString(text: string, start: number, quote: string): number {
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i += 2;
      continue;
    }
    if (text[i] === quote) return i + 1;
    if (text[i] === "\n" && quote !== "`") return i;
    i++;
  }
  return text.length;
}

function unique(hosts: string[]): string[] {
  return [...new Set(hosts)].sort();
}

function isAllowEntry(value: unknown): value is PrivacyAllowEntry {
  if (!value || typeof value !== "object") return false;
  const entry = value as Record<string, unknown>;
  return (
    typeof entry.host === "string" &&
    typeof entry.why === "string" &&
    typeof entry.when === "string" &&
    typeof entry.sends === "string" &&
    Array.isArray(entry.files) &&
    entry.files.every((file) => typeof file === "string")
  );
}

export interface Y7PrivacyGuardReport {
  usageOrWordCountViolations: string[];
  audioOrFeedbackUploadViolations: string[];
  unapprovedOutboundViolations: string[];
}

function listCodeFiles(dir: string, out: string[]): void {
  let info: ReturnType<typeof statSync>;
  try {
    info = statSync(dir);
  } catch {
    return;
  }
  if (!info.isDirectory()) return;
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name) || name === "dist" || name === ".git") continue;
    const child = join(dir, name);
    const st = statSync(child);
    if (st.isDirectory()) {
      listCodeFiles(child, out);
    } else if (/\.(rs|ts|tsx)$/.test(name)) {
      out.push(child);
    }
  }
}

function prodBody(path: string, raw: string): string {
  if (path.endsWith(".rs")) {
    const idx = raw.indexOf("#[cfg(test)]");
    if (idx >= 0) return raw.slice(0, idx);
  }
  return raw;
}

/**
 * Verifies the Y7 do-not-copy privacy invariants (D61, D62, D63, D72, D73):
 * 1. Zero usage telemetry or word-count collection (D63).
 * 2. Zero audio or feedback-carrying-audio upload routes (D62).
 * 3. Outbound HTTP clients strictly limited to model downloads, update check,
 *    and opt-in user-key cloud cleanup, with zero remote kill-switches or
 *    cloud transcript residency/retention copies (D61, D72, D73).
 */
export function verifyY7PrivacyGuards(root: string): Y7PrivacyGuardReport {
  const usageOrWordCountViolations: string[] = [];
  const audioOrFeedbackUploadViolations: string[] = [];
  const unapprovedOutboundViolations: string[] = [];

  for (const manifestRel of ["src-tauri/Cargo.toml", "package.json"]) {
    const body = readFileSync(join(root, manifestRel), "utf8").toLowerCase();
    for (const sdk of [
      "posthog",
      "mixpanel",
      "amplitude",
      "sentry",
      "opentelemetry",
      "google-analytics",
    ]) {
      if (body.includes(sdk)) {
        usageOrWordCountViolations.push(`${manifestRel}: ${sdk}`);
      }
    }
  }

  const files: string[] = [];
  listCodeFiles(join(root, "src-tauri/src"), files);
  listCodeFiles(join(root, "src"), files);

  for (const abs of files) {
    const rel = relative(root, abs).split(sep).join("/");
    if (rel === "src-tauri/src/y7_privacy_guards.rs") continue;
    const raw = readFileSync(abs, "utf8");
    const prod = prodBody(abs, raw);

    for (const marker of [
      "usage_metrics",
      "words_dictated",
      "analytics_events",
      "words_per_minute_telemetry",
    ]) {
      if (prod.includes(marker)) {
        usageOrWordCountViolations.push(`${rel}: ${marker}`);
      }
    }

    for (const marker of [
      "upload_audio",
      "uploadAudio",
      "audio_upload",
      "feedback_audio",
      "send_feedback_with_audio",
      "multipart::Form",
    ]) {
      if (prod.includes(marker)) {
        audioOrFeedbackUploadViolations.push(`${rel}: ${marker}`);
      }
    }

    if (
      rel.endsWith(".rs") &&
      prod.includes("reqwest::") &&
      rel !== "src-tauri/src/managers/model.rs" &&
      rel !== "src-tauri/src/llm_client.rs"
    ) {
      unapprovedOutboundViolations.push(`${rel}: reqwest::`);
    }

    for (const marker of [
      "remote_kill_switch",
      "health_agreement_gate",
      "cloud_retention_days",
      "data_residency_region",
      "sync_transcript_to_cloud",
    ]) {
      if (prod.includes(marker)) {
        unapprovedOutboundViolations.push(`${rel}: ${marker}`);
      }
    }
  }

  return {
    usageOrWordCountViolations,
    audioOrFeedbackUploadViolations,
    unapprovedOutboundViolations,
  };
}

if (import.meta.main) {
  const root = join(import.meta.dir, "..");
  const hits = scanPrivacyHosts(root);
  const filesByHost = new Map<string, Set<string>>();
  for (const hit of hits) {
    const files = filesByHost.get(hit.host) ?? new Set<string>();
    files.add(hit.file);
    filesByHost.set(hit.host, files);
  }
  const summary = [...filesByHost.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([host, files]) => ({ host, files: [...files].sort() }));
  console.log(JSON.stringify(summary, null, 2));
  console.error(`${hits.length} urls, ${summary.length} hosts`);
}
