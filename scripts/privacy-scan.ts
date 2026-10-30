/**
 * List every http(s) and ws(s) host under the app source.
 * Comment-only text is ignored so a note cannot look like a new network call.
 * A host that is not in scripts/privacy-allowlist.json fails the static check.
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
 * eaten. Cargo `#` comments are dropped only in toml, where that is the comment
 * syntax. A `#` line in Rust is an attribute and is kept.
 */
export function stripComments(text: string, toml: boolean): string {
  let out = "";
  let i = 0;

  while (i < text.length) {
    const raw = readRawString(text, i);
    if (raw) {
      out += raw.value;
      i = raw.next;
      continue;
    }

    const quote = text[i];
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

    if (toml && text[i] === "#" && lineSoFarIsBlank(out)) {
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
  const stripped = stripComments(text, path.endsWith(".toml"));
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

function lineSoFarIsBlank(out: string): boolean {
  for (let i = out.length - 1; i >= 0 && out[i] !== "\n"; i--) {
    if (out[i] !== " " && out[i] !== "\t") return false;
  }
  return true;
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
