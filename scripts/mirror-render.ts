import type { CatalogRow, MirrorEntry } from "./mirror-models.ts";

const QWEN_SENTENCE =
  "Unofficial mirror of Qwen3-4B-GGUF by Qwen, as published by Qwen. " +
  "Not affiliated with or endorsed by Qwen.";
const QWEN_UNMODIFIED = [
  '"Unmodified" here means the GGUF bytes are identity-checked against ',
  "Qwen's file: same sha256, no re-quantizing, no metadata edits. ",
  "`LICENSE` is copied from Qwen's repository and is not rewritten. ",
  "There is no handy-computer conversion and no NOTICE for this model.",
].join("");
const NO_COPYRIGHT = "None supplied upstream";
const LICENCE: Record<string, readonly [string, string]> = {
  mit: ["MIT", "https://opensource.org/license/mit"],
  "apache-2.0": ["Apache-2.0", "https://www.apache.org/licenses/LICENSE-2.0"],
  "cc-by-4.0": ["CC-BY-4.0", "https://creativecommons.org/licenses/by/4.0/"],
};
const PUBLISHER: Record<string, string> = {
  nvidia: "NVIDIA",
  openai: "OpenAI",
  Qwen: "Qwen",
  "ibm-granite": "IBM",
  mistralai: "Mistral",
  "MediaTek-Research": "MediaTek",
  "ai-sage": "GigaChat",
  UsefulSensors: "Useful Sensors",
};

export type NoticeFacts = {
  commit: string | null;
  upstreamUrl: string | null;
};
export type Docs = { readme: string; attribution: string };
type LicenceFile = {
  models?: Record<string, { revision?: string }>;
};

export function renderMirrorDocs(
  templates: Docs,
  entry: MirrorEntry,
  row: CatalogRow | undefined,
  notice: NoticeFacts,
): Docs {
  const vars = cardVars(entry, row, notice);
  return {
    readme: renderReadme(templates.readme, entry, vars),
    attribution: renderAttribution(templates.attribution, entry, vars),
  };
}

export function needsNotice(entry: MirrorEntry): boolean {
  return !isQwen(entry);
}

export function licencePin(id: string, licencesText: string): string {
  const file = JSON.parse(licencesText) as LicenceFile;
  const hex = file.models?.[id]?.revision?.trim().toLowerCase() ?? "";
  if (/^[0-9a-f]{40}$/.test(hex)) return hex;
  throw new Error(`refusing ${id}: no 40-hex revision in model_licences.json`);
}

export function renderMirrorTexts(
  templates: Docs,
  entry: MirrorEntry,
  row: CatalogRow | undefined,
  noticeText: string | null,
  licencesText: string,
): Docs {
  const pinned = licencePin(entry.id, licencesText);
  let notice: NoticeFacts = { commit: pinned, upstreamUrl: null };
  if (needsNotice(entry)) {
    notice = withPinnedCommit(entry.id, pinned, readNotice(noticeText ?? ""));
  }
  const docs = renderMirrorDocs(templates, entry, row, notice);
  refusePlaceholders(entry.id, docs);
  return docs;
}

// NOTICE states a short commit. The full hash is the licence revision.
function readNotice(text: string): NoticeFacts {
  const found = [...text.matchAll(/at commit ([0-9a-f]{7,40})\b/gi)].map(
    (match) => match[1].toLowerCase(),
  );
  const unique = [...new Set(found)];
  const upstream = text.match(
    /https:\/\/huggingface\.co\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+/,
  );
  return {
    commit: unique.length === 1 ? unique[0] : null,
    upstreamUrl: upstream?.[0] ?? null,
  };
}
function withPinnedCommit(
  id: string,
  pinned: string,
  notice: NoticeFacts,
): NoticeFacts {
  const short = notice.commit;
  if (short && pinned.startsWith(short)) {
    return { commit: pinned, upstreamUrl: notice.upstreamUrl };
  }
  const seen = short ?? "missing";
  throw new Error(
    `refusing ${id}: NOTICE commit ${seen} does not match ` +
      `model_licences revision ${pinned}`,
  );
}
function refusePlaceholders(id: string, docs: Docs): void {
  const text = `${docs.readme}\n${docs.attribution}`;
  const open = /\{\s*[A-Za-z0-9_]+\s*\}/;
  if (text.includes("{UNKNOWN:") || open.test(text)) {
    throw new Error(
      `refusing ${id}: README.md or ATTRIBUTION.md has a placeholder`,
    );
  }
}
function cardVars(
  entry: MirrorEntry,
  row: CatalogRow | undefined,
  notice: NoticeFacts,
): Record<string, string> {
  const qwen = isQwen(entry);
  const upstream =
    row?.base_model?.trim() ||
    (qwen ? entry.sourceRepo : "{UNKNOWN: upstream repository}");
  const model =
    tail(row?.base_model) || row?.slug?.trim() || tail(entry.sourceRepo);
  const org = upstream.split("/")[0] ?? "";
  const publisher = upstream.startsWith("{UNKNOWN:")
    ? "{UNKNOWN: publisher plain name}"
    : (PUBLISHER[org] ?? "{UNKNOWN: publisher plain name}");
  const commit = notice.commit || (qwen ? entry.revision : "");
  if (!commit) {
    throw new Error(`refusing ${entry.id}: upstream commit is missing`);
  }
  const upstreamUrl =
    notice.upstreamUrl ||
    (upstream.startsWith("{UNKNOWN:")
      ? upstream
      : `https://huggingface.co/${upstream}`);
  const copyright = isWhisper(entry, model)
    ? "Copyright (c) 2022 OpenAI"
    : qwen
      ? "Copyright 2025 Alibaba Cloud"
      : NO_COPYRIGHT;
  const [licenceName, licenceUrl] = LICENCE[entry.licence] ?? [
    entry.licence,
    "{UNKNOWN: licence url}",
  ];
  return {
    spdx_id: entry.licence,
    upstream_repo: upstream,
    model,
    publisher,
    mirrored_filename: entry.filename,
    quant: entry.quant,
    sha256: entry.sha256 ?? `{UNKNOWN: sha256 of ${entry.filename}}`,
    upstream_repo_url: upstreamUrl,
    upstream_commit: commit,
    converted_at_commit: commit,
    sha: commit,
    licence_name: licenceName,
    licence_url: licenceUrl,
    copyright_notice: copyright,
  };
}
function renderReadme(
  template: string,
  entry: MirrorEntry,
  vars: Record<string, string>,
): string {
  let text = fill(template.split("\n## Variant:")[0]?.trim() ?? "", vars);
  if (!isQwen(entry)) return `${text}\n`;
  text = text.replace(/^Unofficial mirror of .+$/m, QWEN_SENTENCE);
  text = dropLines(text, "- Converter:");
  text = text.replace(
    "`NOTICE` records the conversion and states that the weights " +
      "were not retrained or fine-tuned. ",
    "",
  );
  return `${swapQwen(text).trim()}\n`;
}
function renderAttribution(
  template: string,
  entry: MirrorEntry,
  vars: Record<string, string>,
): string {
  let copy = fill(between(template, "## This copy", "## Licence blocks"), vars);
  if (isQwen(entry)) {
    copy = dropLines(copy, "- Converter:");
    copy = copy.replace(/^"Unmodified" here means .+$/m, QWEN_UNMODIFIED);
  }
  const heading =
    entry.licence === "mit"
      ? "### MIT"
      : entry.licence === "cc-by-4.0"
        ? "### CC-BY-4.0"
        : "### Apache-2.0";
  let licenceBody = stripNotes(fill(section(template, heading), vars));
  if (isQwen(entry)) {
    licenceBody = swapQwen(licenceBody).replace(
      "`NOTICE` is handy-computer's file.",
      "There is no NOTICE for this model.",
    );
    licenceBody += "\n\nCopyright: Copyright 2025 Alibaba Cloud";
  }
  const parts = [copy, `## ${heading.slice(4)}`, licenceBody];
  if (isWhisper(entry, vars.model ?? "")) {
    const whisper = section(template, "### Whisper");
    parts.push("## Whisper", stripNotes(fill(whisper, vars)));
  }
  return `${parts.join("\n\n").trim()}\n`;
}
function swapQwen(text: string): string {
  return text
    .replaceAll("handy-computer's repository", "Qwen's repository")
    .replaceAll("the file handy-computer published", "the file Qwen published")
    .replaceAll(
      "The other quants handy-computer publishes",
      "The other quants Qwen publishes",
    )
    .replaceAll("the model this file was converted from.", "the model.");
}
function between(template: string, start: string, end: string): string {
  const from = template.indexOf(start);
  const to = template.indexOf(end, from + start.length);
  if (from < 0 || to < 0) throw new Error(`template missing ${start}`);
  return template.slice(from, to).trim();
}
function section(template: string, heading: string): string {
  const lines = template.split("\n");
  const start = lines.findIndex((line) => line.startsWith(heading));
  if (start < 0) throw new Error(`template missing ${heading}`);
  const rest = lines.slice(start + 1);
  const end = rest.findIndex(
    (line) => line.startsWith("### ") || line.startsWith("## "),
  );
  return (end < 0 ? rest : rest.slice(0, end)).join("\n").trim();
}
function stripNotes(body: string): string {
  const skip = (line: string) =>
    line.startsWith("Use this paragraph") ||
    line.startsWith("Keep the model block") ||
    line.startsWith("Drop the converter") ||
    line.startsWith("Ship this notice") ||
    line.startsWith("Card sentence for this model");
  return body
    .split("\n")
    .filter((line) => !skip(line.trim()))
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}
function dropLines(text: string, prefix: string): string {
  const keep = (line: string) => !line.trim().startsWith(prefix);
  return text.split("\n").filter(keep).join("\n");
}
function fill(template: string, vars: Record<string, string>): string {
  return template.replace(/\{\s*([a-z0-9_]+)\s*\}/gi, (whole, key: string) =>
    Object.prototype.hasOwnProperty.call(vars, key) ? vars[key] : whole,
  );
}
function isQwen(entry: { id: string; sourceRepo: string }): boolean {
  return (
    entry.id === "Qwen/Qwen3-4B-GGUF" ||
    entry.sourceRepo === "Qwen/Qwen3-4B-GGUF"
  );
}
function isWhisper(entry: MirrorEntry, model: string): boolean {
  return model.startsWith("whisper-") || entry.id.includes("/whisper-");
}
function tail(repo: string | undefined): string {
  return repo?.trim().split("/").filter(Boolean).pop() ?? "";
}
