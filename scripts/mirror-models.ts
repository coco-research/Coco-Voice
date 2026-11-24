/** Dry-run allowlist. One line per model, no network. Upload is mirror-upload.ts. */
import { readFileSync } from "node:fs";
import { join } from "node:path";

export const MIRROR_TARGET_ORG = "coco-research";

export const STAY_IDS = [
  "handy-computer/canary-qwen-2.5b-gguf",
  "handy-computer/cohere-transcribe-03-2026-gguf",
] as const;

const ALLOWED_LICENCES = new Set(["mit", "apache-2.0", "cc-by-4.0"]);

export interface CatalogRow {
  id: string;
  source_repo?: string;
  license?: string | null;
  revision?: string;
  sha256?: string;
  slug?: string;
  base_model?: string;
  default_quant?: string;
  files?: Array<{ filename?: string; quant?: string; sha256?: string }>;
}

export interface CatalogFile {
  models: CatalogRow[];
}

export interface MirrorEntry {
  id: string;
  sourceRepo: string;
  revision: string;
  filename: string;
  quant: string;
  sha256: string | null;
  licence: string;
  targetRepo: string;
}

export interface MirrorRunOptions {
  stdout?: (line: string) => void;
  stderr?: (line: string) => void;
  catalogText?: string;
  modRsText?: string;
  root?: string;
}

// Reads the const array from src-tauri/src/catalog/mod.rs so this script
// can't drift from the app's list. The test pins the count at 20 so a format change fails loudly.
export function parseLegalHoldIds(modRs: string): string[] {
  const marker = modRs.indexOf("LEGAL_HOLD_IDS");
  if (marker < 0) throw new Error("LEGAL_HOLD_IDS not found");
  const slice = modRs.slice(marker);
  const begin = slice.indexOf("[");
  const end = slice.indexOf("];");
  if (begin < 0 || end < 0 || end < begin) {
    throw new Error("LEGAL_HOLD_IDS array not found");
  }
  return [...slice.slice(begin, end).matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

export function refusalReason(
  row: CatalogRow,
  holdIds: readonly string[],
): string | null {
  if (holdIds.some((id) => sameId(id, row.id))) {
    return `refusing ${row.id}: legal hold`;
  }
  if (STAY_IDS.some((id) => sameId(id, row.id))) {
    return `refusing ${row.id}: stays on the current host`;
  }
  const licence = normalizeLicence(row.license);
  if (!licence || !ALLOWED_LICENCES.has(licence)) {
    return `refusing ${row.id}: licence ${licence ?? "(none)"} is not allowlisted`;
  }
  if (!/^[0-9a-f]{40}$/i.test((row.revision ?? "").trim())) {
    return `refusing ${row.id}: no pinned 40-hex revision`;
  }
  const quant = row.default_quant ?? "";
  const hasFile =
    quant.length > 0 &&
    (row.files ?? []).some(
      (item) => item.quant === quant && (item.filename?.trim().length ?? 0) > 0,
    );
  if (!hasFile) return `refusing ${row.id}: no default-quant file`;
  return null;
}

export function mirrorSet(
  catalog: CatalogFile,
  holdIds: readonly string[],
): MirrorEntry[] {
  return catalog.models.flatMap((row) =>
    refusalReason(row, holdIds) ? [] : [toEntry(row)],
  );
}

export function selectMirror(
  catalog: CatalogFile,
  holdIds: readonly string[],
  requested: readonly string[],
): { entries: MirrorEntry[]; refusals: string[] } {
  if (requested.length === 0) {
    return { entries: mirrorSet(catalog, holdIds), refusals: [] };
  }
  const entries: MirrorEntry[] = [];
  const refusals: string[] = [];
  for (const id of requested) {
    const row = catalog.models.find((model) => sameId(model.id, id));
    if (!row) {
      refusals.push(`refusing ${id}: not in the catalog`);
      continue;
    }
    const reason = refusalReason(row, holdIds);
    if (reason) refusals.push(reason);
    else entries.push(toEntry(row));
  }
  return { entries, refusals };
}

export async function runMirror(
  argv: readonly string[],
  options: MirrorRunOptions = {},
): Promise<number> {
  const stdout = options.stdout ?? ((line: string) => console.log(line));
  const stderr = options.stderr ?? ((line: string) => console.error(line));
  if (hasTokenArg(argv)) {
    stderr(
      "refusing a token passed on the command line; set HF_TOKEN in the environment",
    );
    return 1;
  }
  const ids: string[] = [];
  for (const arg of argv) {
    if (arg.startsWith("-")) {
      stderr(`unknown argument ${arg}`);
      return 1;
    }
    ids.push(arg);
  }
  const root = options.root ?? join(import.meta.dirname, "..");
  const text =
    options.catalogText ??
    readFileSync(join(root, "src-tauri/src/catalog/catalog.json"), "utf8");
  const catalog = JSON.parse(text) as CatalogFile;
  const holdIds = parseLegalHoldIds(
    options.modRsText ??
      readFileSync(join(root, "src-tauri/src/catalog/mod.rs"), "utf8"),
  );
  const picked = selectMirror(catalog, holdIds, ids);
  if (picked.refusals.length > 0) {
    for (const line of picked.refusals) stderr(line);
    return 1;
  }
  for (const entry of picked.entries) {
    const sha = entry.sha256 ?? "missing";
    stdout(
      `${entry.sourceRepo}@${entry.revision} ${entry.filename} ${sha} ${entry.targetRepo}`,
    );
  }
  return 0;
}

export function hasTokenArg(argv: readonly string[]): boolean {
  return argv.some((arg) =>
    /^(?:--(?:token|hf-token|hf_token)(?:=.*)?|hf_[A-Za-z0-9]{8,})$/.test(arg),
  );
}

function toEntry(row: CatalogRow): MirrorEntry {
  const files = row.files ?? [];
  const file =
    files.find((item) => item.quant === row.default_quant) ?? files[0];
  const sourceRepo = row.source_repo?.trim() || row.id;
  const name = sourceRepo
    .split("/")
    .filter((part) => part.length > 0)
    .pop();
  return {
    id: row.id,
    sourceRepo,
    revision: (row.revision ?? "").trim(),
    filename: file?.filename ?? "",
    quant: file?.quant ?? "",
    sha256: parseSha(file?.sha256) ?? parseSha(row.sha256),
    licence: normalizeLicence(row.license) ?? "",
    targetRepo: `${MIRROR_TARGET_ORG}/${name ?? row.id}`,
  };
}

function parseSha(raw: string | undefined): string | null {
  const hex = raw?.trim().toLowerCase() ?? "";
  return /^[0-9a-f]{64}$/.test(hex) ? hex : null;
}

function normalizeLicence(raw: string | null | undefined): string | null {
  const licence = raw?.trim().toLowerCase() ?? "";
  return licence.length > 0 ? licence : null;
}

function sameId(left: string, right: string): boolean {
  return left.toLowerCase() === right.toLowerCase();
}

function main(): void {
  runMirror(process.argv.slice(2))
    .then((code) => process.exit(code))
    .catch((error: unknown) => {
      console.error(error instanceof Error ? error.message : String(error));
      process.exit(1);
    });
}

if (import.meta.main) main();
