import { createHash } from "node:crypto";
import { createReadStream, readFileSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pipeline } from "node:stream/promises";
import {
  hasTokenArg,
  parseLegalHoldIds,
  selectMirror,
  type CatalogFile,
  type CatalogRow,
  type MirrorEntry,
} from "./mirror-models.ts";
import {
  licencePin,
  needsNotice,
  renderMirrorTexts,
  type Docs,
} from "./mirror-render.ts";

type Run = { code: number | null; stdout: string; stderr: string };
export type SpawnFn = (
  command: string,
  args: readonly string[],
  env: NodeJS.ProcessEnv,
) => Promise<Run>;
export type FetchFn = (
  input: string,
  init?: { headers?: Record<string, string> },
) => Promise<{ status: number; text: () => Promise<string> }>;
type Say = (line: string) => void;
interface UploadOptions {
  spawn?: SpawnFn;
  fetch?: FetchFn;
  env?: NodeJS.ProcessEnv;
  stdout?: Say;
  stderr?: Say;
  catalogText?: string;
  licencesText?: string;
  modRsText?: string;
  templates?: Docs;
}

export async function runUpload(
  argv: readonly string[],
  options: UploadOptions = {},
): Promise<number> {
  const stdout = options.stdout ?? ((line) => console.log(line));
  const stderr = options.stderr ?? ((line) => console.error(line));
  if (hasTokenArg(argv)) {
    stderr(
      "refusing a token passed on the command line; " +
        "set HF_TOKEN in the environment",
    );
    return 1;
  }
  const env = { ...(options.env ?? process.env) };
  const token = env.HF_TOKEN?.trim() ?? "";
  if (!token) {
    stderr("HF_TOKEN is not set");
    return 1;
  }
  env.HF_TOKEN = token;
  const sayErr = (line: string) => stderr(scrub(line, token));
  const root = join(import.meta.dirname, "..");
  const read = (rel: string) => readFileSync(join(root, rel), "utf8");
  const catalog = JSON.parse(
    options.catalogText ?? read("src-tauri/src/catalog/catalog.json"),
  ) as CatalogFile;
  const holdIds = parseLegalHoldIds(
    options.modRsText ?? read("src-tauri/src/catalog/mod.rs"),
  );
  const licencesText =
    options.licencesText ?? read("src-tauri/src/catalog/model_licences.json");
  const picked = selectMirror(catalog, holdIds, argv);
  if (picked.refusals.length > 0) {
    for (const line of picked.refusals) sayErr(line);
    return 1;
  }
  const templates = options.templates ?? {
    readme: read("scripts/mirror/README-card.template.md"),
    attribution: read("scripts/mirror/ATTRIBUTION.template.md"),
  };
  const runSpawn = options.spawn ?? spawnCommand;
  const runFetch: FetchFn =
    options.fetch ?? ((input, init) => globalThis.fetch(input, init));
  const reportPath = join(root, "mirror-upload-report.json");
  const report: { models: Record<string, string>[] } = { models: [] };
  for (const entry of picked.entries) {
    const dir = await mkdtemp(join(tmpdir(), "coco-voice-mirror-"));
    let failed = false;
    try {
      const row = catalog.models.find((model) => model.id === entry.id);
      const recorded = await uploadOne(
        entry,
        row,
        dir,
        templates,
        token,
        env,
        runSpawn,
        runFetch,
        licencesText,
      );
      report.models.push(recorded);
      writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
      const line =
        `${recorded.targetRepo}@${recorded.targetRevision} ` +
        `${recorded.filename} ${recorded.sha256}`;
      stdout(scrub(line, token));
    } catch (error) {
      failed = true;
      sayErr(error instanceof Error ? error.message : String(error));
    } finally {
      await rm(dir, { recursive: true, force: true }).catch(() => {
        failed = true;
        sayErr(`could not delete the local copy at ${dir}`);
      });
    }
    if (failed) return 1;
  }
  return 0;
}

async function uploadOne(
  entry: MirrorEntry,
  row: CatalogRow | undefined,
  dir: string,
  templates: Docs,
  token: string,
  env: NodeJS.ProcessEnv,
  runSpawn: SpawnFn,
  runFetch: FetchFn,
  licencesText: string,
) {
  const sha = entry.sha256;
  if (!sha) throw new Error(`refusing ${entry.id}: no sha256`);
  licencePin(entry.id, licencesText);
  const pull = (name: string) =>
    hf(runSpawn, env, token, [
      "download",
      entry.sourceRepo,
      name,
      "--revision",
      entry.revision,
      "--local-dir",
      dir,
      "--quiet",
    ]);
  await pull(entry.filename);
  const localSha = await sha256File(join(dir, entry.filename));
  if (localSha !== sha) {
    throw new Error(
      `downloaded sha256 ${localSha} does not match catalog ${sha} ` +
        `for ${entry.filename}`,
    );
  }
  await pull("LICENSE");
  let noticeText: string | null = null;
  if (needsNotice(entry)) {
    await pull("NOTICE");
    noticeText = readFileSync(join(dir, "NOTICE"), "utf8");
  }
  await rm(join(dir, ".cache"), { recursive: true, force: true });
  const docs = renderMirrorTexts(
    templates,
    entry,
    row,
    noticeText,
    licencesText,
  );
  writeFileSync(join(dir, "README.md"), docs.readme);
  writeFileSync(join(dir, "ATTRIBUTION.md"), docs.attribution);
  await hf(runSpawn, env, token, [
    "repos",
    "create",
    entry.targetRepo,
    "--type",
    "model",
    "--exist-ok",
    "--public",
    "--quiet",
  ]);
  const uploaded = await hf(runSpawn, env, token, [
    "upload",
    entry.targetRepo,
    dir,
    ".",
    "--commit-message",
    `Mirror ${entry.filename}`,
    "--quiet",
  ]);
  const targetRevision = await targetRevisionOf(
    runFetch,
    token,
    entry.targetRepo,
    uploaded.stdout,
  );
  const remoteSha = await remoteFileSha(
    runFetch,
    token,
    entry.targetRepo,
    targetRevision,
    entry.filename,
  );
  if (remoteSha !== sha) {
    throw new Error(
      `uploaded sha256 ${remoteSha} does not match catalog ${sha} ` +
        `for ${entry.filename}`,
    );
  }
  return {
    id: entry.id,
    sourceRepo: entry.sourceRepo,
    sourceRevision: entry.revision,
    filename: entry.filename,
    sha256: sha,
    targetRepo: entry.targetRepo,
    targetRevision,
  };
}

function spawnCommand(
  command: string,
  args: readonly string[],
  env: NodeJS.ProcessEnv,
): Promise<Run> {
  const child = Bun.spawn([command, ...args], {
    env,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  const out = new Response(child.stdout).text();
  const err = new Response(child.stderr).text();
  return Promise.all([out, err, child.exited]).then(
    ([stdout, stderr, code]) => ({ code, stdout, stderr }),
  );
}

async function hf(
  runSpawn: SpawnFn,
  env: NodeJS.ProcessEnv,
  token: string,
  args: readonly string[],
): Promise<Run> {
  const bad = (arg: string) =>
    arg === "--token" || arg.startsWith("--token=") || arg.includes(token);
  if (args.some(bad)) throw new Error("refusing to pass a token to hf");
  let result: Run;
  try {
    result = await runSpawn("hf", args, env);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(scrub(`hf ${args[0] ?? ""} failed: ${message}`, token));
  }
  if (result.code === 0) return result;
  const detail = scrub(`${result.stderr}\n${result.stdout}`, token).trim();
  const suffix = detail ? `: ${detail}` : "";
  throw new Error(`hf ${args[0] ?? ""} exited ${result.code}${suffix}`);
}

async function targetRevisionOf(
  runFetch: FetchFn,
  token: string,
  repo: string,
  uploadStdout: string,
): Promise<string> {
  const fromCli = /\/commit\/([0-9a-f]{40})/i.exec(uploadStdout)?.[1];
  if (fromCli) return fromCli.toLowerCase();
  const url = `https://huggingface.co/api/models/${repo}`;
  const parsed = JSON.parse(await hubGet(runFetch, token, url)) as {
    sha?: string;
  };
  const sha = parsed.sha ?? "";
  if (!/^[0-9a-f]{40}$/i.test(sha)) {
    throw new Error(`Hugging Face API did not return a revision for ${repo}`);
  }
  return sha.toLowerCase();
}

async function remoteFileSha(
  runFetch: FetchFn,
  token: string,
  repo: string,
  revision: string,
  filename: string,
): Promise<string> {
  const base = `https://huggingface.co/api/models/${repo}`;
  const urls = [`${base}/tree/${revision}`, `${base}/revision/${revision}`];
  // Keep the last error so an auth or network failure isn't reported as a
  // missing hash.
  let lastError = "";
  for (const url of urls) {
    try {
      const body = await hubGet(runFetch, token, url);
      const sha = lfsSha(JSON.parse(body), filename);
      if (sha) return sha;
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
    }
  }
  const cause = lastError ? ` (${lastError})` : "";
  throw new Error(
    `uploaded file ${filename} has no LFS sha256 on ${repo}${cause}`,
  );
}

async function hubGet(runFetch: FetchFn, token: string, url: string) {
  const response = await runFetch(url, {
    headers: { Accept: "application/json", Authorization: `Bearer ${token}` },
  });
  if (response.status !== 200) {
    throw new Error(`Hugging Face API ${response.status}`);
  }
  return response.text();
}

function lfsSha(payload: unknown, filename: string): string | null {
  const bag = isRecord(payload) ? payload.siblings : payload;
  const rows = Array.isArray(bag) ? bag : [];
  for (const row of rows) {
    if (!isRecord(row)) continue;
    const named = typeof row.path === "string" ? row.path : row.rfilename;
    const name = typeof named === "string" ? named : "";
    if (name !== filename && !name.endsWith(`/${filename}`)) continue;
    const lfs = isRecord(row.lfs) ? row.lfs : {};
    for (const raw of [lfs.sha256, lfs.oid]) {
      if (typeof raw !== "string") continue;
      const hex = raw.replace(/^sha256:/i, "");
      if (/^[0-9a-f]{64}$/i.test(hex)) return hex.toLowerCase();
    }
  }
  return null;
}

function sha256File(path: string): Promise<string> {
  const hash = createHash("sha256");
  return pipeline(createReadStream(path), hash).then(() => hash.digest("hex"));
}

function scrub(text: string, secret: string): string {
  return secret ? text.split(secret).join("[redacted]") : text;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

if (import.meta.main) {
  const token = process.env.HF_TOKEN?.trim() ?? "";
  runUpload(process.argv.slice(2))
    .then((code) => process.exit(code))
    .catch((error: unknown) => {
      const text = error instanceof Error ? error.message : String(error);
      console.error(scrub(text, token));
      process.exit(1);
    });
}
