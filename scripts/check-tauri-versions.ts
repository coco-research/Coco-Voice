/**
 * Fail when a Tauri npm package and its Rust crate differ in major.minor.
 * `tauri build` rejects that skew, and PR CI does not run a Tauri build.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

export interface TauriVersionMismatch {
  npmPackage: string;
  npmVersion: string;
  crate: string;
  crateVersion: string;
}

export interface TauriVersionReport {
  pairCount: number;
  mismatches: TauriVersionMismatch[];
}

/** Drop JSONC trailing commas so JSON.parse can read bun.lock. */
export function stripJsoncTrailingCommas(text: string): string {
  return text.replace(/,(\s*[}\]])/g, "$1");
}

/**
 * Resolved version of each top-level bun.lock `packages` entry.
 * The id is `name@version` in the first array slot.
 */
export function parseBunLock(text: string): Map<string, string> {
  const parsed: unknown = JSON.parse(stripJsoncTrailingCommas(text));
  if (!isRecord(parsed) || !isRecord(parsed.packages)) {
    throw new Error("bun.lock has no packages map");
  }

  const versions = new Map<string, string>();
  for (const [name, entry] of Object.entries(parsed.packages)) {
    if (!Array.isArray(entry) || typeof entry[0] !== "string") continue;
    const version = versionFromResolvedId(name, entry[0]);
    if (version) versions.set(name, version);
  }
  return versions;
}

/**
 * Every `[[package]]` version in a Cargo.lock, keyed by crate name.
 * A name can appear more than once.
 */
export function parseCargoLock(text: string): Map<string, string[]> {
  const versions = new Map<string, string[]>();
  for (const block of text.split(/^\[\[package\]\]\r?$/m)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    if (!name || !version) continue;
    const existing = versions.get(name);
    if (existing) existing.push(version);
    else versions.set(name, [version]);
  }
  return versions;
}

/**
 * Compare package.json dependency names with Cargo.lock.
 * `@tauri-apps/cli` is skipped. A package with no crate is skipped.
 * Patch may differ; major.minor may not.
 */
export function compareTauriVersions(
  dependencyNames: readonly string[],
  bunLockText: string,
  cargoLockText: string,
): TauriVersionReport {
  const npmVersions = parseBunLock(bunLockText);
  const crateVersions = parseCargoLock(cargoLockText);
  const mismatches: TauriVersionMismatch[] = [];
  let pairCount = 0;

  for (const npmPackage of dependencyNames) {
    const crate = crateForNpmPackage(npmPackage);
    if (!crate) continue;
    const crates = crateVersions.get(crate);
    if (!crates || crates.length === 0) continue;

    const npmVersion = npmVersions.get(npmPackage);
    if (!npmVersion) {
      mismatches.push({
        npmPackage,
        npmVersion: "(missing from bun.lock)",
        crate,
        crateVersion: crates.join(", "),
      });
      continue;
    }

    for (const crateVersion of crates) {
      pairCount += 1;
      if (majorMinor(npmVersion) !== majorMinor(crateVersion)) {
        mismatches.push({ npmPackage, npmVersion, crate, crateVersion });
      }
    }
  }

  return { pairCount, mismatches };
}

function crateForNpmPackage(npmPackage: string): string | null {
  if (npmPackage === "@tauri-apps/cli") return null;
  if (npmPackage === "@tauri-apps/api") return "tauri";
  const plugin = /^@tauri-apps\/plugin-(.+)$/.exec(npmPackage);
  if (!plugin) return null;
  return `tauri-plugin-${plugin[1]}`;
}

function majorMinor(version: string): string {
  const [major, minor] = version.split(".");
  return `${major}.${minor ?? ""}`;
}

function versionFromResolvedId(name: string, id: string): string | null {
  const prefix = `${name}@`;
  if (!id.startsWith(prefix)) return null;
  const version = id.slice(prefix.length);
  return version.length > 0 ? version : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function formatTable(rows: TauriVersionMismatch[]): string {
  const header = ["npm package", "npm version", "crate", "crate version"];
  const body = rows.map((row) => [
    row.npmPackage,
    row.npmVersion,
    row.crate,
    row.crateVersion,
  ]);
  const widths = header.map((cell, index) =>
    Math.max(cell.length, ...body.map((row) => row[index].length)),
  );
  const line = (cells: string[]) =>
    cells
      .map((cell, index) => cell.padEnd(widths[index]))
      .join("  ")
      .trimEnd();
  return [line(header), ...body.map(line)].join("\n");
}

function main(): void {
  const root = join(import.meta.dirname, "..");
  const packageJson = JSON.parse(
    readFileSync(join(root, "package.json"), "utf8"),
  ) as { dependencies?: Record<string, string> };
  const report = compareTauriVersions(
    Object.keys(packageJson.dependencies ?? {}),
    readFileSync(join(root, "bun.lock"), "utf8"),
    readFileSync(join(root, "src-tauri", "Cargo.lock"), "utf8"),
  );

  if (report.mismatches.length > 0) {
    console.error("Tauri npm and crate versions differ in major.minor:");
    console.error(formatTable(report.mismatches));
    process.exit(1);
  }
  console.log(`tauri versions match: ${report.pairCount} pairs`);
}

if (import.meta.main) main();
