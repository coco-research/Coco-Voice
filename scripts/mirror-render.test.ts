import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  mirrorSet,
  parseLegalHoldIds,
  type CatalogFile,
  type CatalogRow,
} from "./mirror-models.ts";
import {
  renderMirrorDocs,
  renderMirrorTexts,
  type NoticeFacts,
} from "./mirror-render.ts";

const root = join(import.meta.dirname, "..");
const mirrorDir = join(root, "scripts/mirror");
const readme = readFileSync(join(mirrorDir, "README-card.template.md"), "utf8");
const attribution = readFileSync(
  join(mirrorDir, "ATTRIBUTION.template.md"),
  "utf8",
);

const catalog = JSON.parse(
  readFileSync(join(root, "src-tauri/src/catalog/catalog.json"), "utf8"),
) as CatalogFile;
const holds = parseLegalHoldIds(
  readFileSync(join(root, "src-tauri/src/catalog/mod.rs"), "utf8"),
);
const templates = { readme, attribution };
const none: NoticeFacts = { commit: null, upstreamUrl: null };

function row(id: string): CatalogRow {
  const found = catalog.models.find((model) => model.id === id);
  if (!found) throw new Error(`${id} is not in the catalog`);
  return found;
}

function docs(id: string, notice: NoticeFacts = none) {
  const entry = mirrorSet(catalog, holds).find((item) => item.id === id);
  if (!entry) throw new Error(`${id} is not in the mirror set`);
  return renderMirrorDocs(
    templates,
    { ...entry, sha256: entry.sha256 ?? "ab".repeat(32) },
    row(id),
    notice,
  );
}

test("Whisper, CC-BY and Qwen renderings carry their required lines", () => {
  const whisper = docs("handy-computer/whisper-large-v3-turbo-gguf", {
    commit: "cd".repeat(20),
    upstreamUrl: null,
  });
  expect(whisper.readme).toContain(
    "Unofficial mirror of whisper-large-v3-turbo by OpenAI, " +
      "converted to GGUF by handy-computer. Not affiliated " +
      "with or endorsed by OpenAI.",
  );
  expect(whisper.attribution).toContain(
    "Redistributed unmodified by Coco Research.",
  );
  expect(whisper.attribution).toContain("Copyright (c) 2022 OpenAI");
  expect(whisper.attribution).toContain(
    "Whisper's code and model weights are released under the MIT License.",
  );

  const canary = docs("handy-computer/canary-180m-flash-gguf", {
    commit: "cd".repeat(20),
    upstreamUrl: null,
  });
  expect(canary.readme).toContain(
    "Unofficial mirror of canary-180m-flash by NVIDIA, converted " +
      "to GGUF by handy-computer. Not affiliated with or endorsed by NVIDIA.",
  );
  expect(canary.attribution).toContain(
    "Redistributed unmodified by Coco Research.",
  );
  expect(canary.attribution).toContain("- Creator: NVIDIA");
  expect(canary.attribution).toContain(
    "- Licence: CC-BY-4.0, https://creativecommons.org/licenses/by/4.0/",
  );
  expect(canary.attribution).toContain(
    "Converted to GGUF by handy-computer; this is a modified format " +
      "of the original.",
  );
  expect(canary.attribution).toContain(
    "The Creative Commons licence's disclaimer of warranties applies " +
      "to this model as it does to the original.",
  );
  expect(canary.attribution).toContain(
    "This repository is not gated and carries no DRM.",
  );

  const qwen = docs("Qwen/Qwen3-4B-GGUF");
  expect(qwen.readme).toContain(
    "Unofficial mirror of Qwen3-4B-GGUF by Qwen, as published by Qwen. " +
      "Not affiliated with or endorsed by Qwen.",
  );
  expect(qwen.readme).not.toContain("converted to GGUF by handy-computer");
  expect(qwen.readme).not.toContain("records the conversion");
  expect(qwen.attribution).toContain(
    "There is no handy-computer conversion and no NOTICE for this model.",
  );
  expect(qwen.attribution).toContain(
    "Redistributed unmodified by Coco Research.",
  );
});

const ID = "handy-computer/canary-180m-flash-gguf";
const FULL = "a".repeat(40);

function canary() {
  const found = mirrorSet(catalog, holds).find((item) => item.id === ID);
  if (!found) throw new Error(ID);
  const entry = { ...found, sha256: found.sha256 ?? "ab".repeat(32) };
  const model = row(ID);
  const licences = JSON.stringify({
    models: { [ID]: { revision: FULL } },
  });
  const at = (short: string) =>
    `https://huggingface.co/nvidia/canary at commit ${short}`;
  const texts = (notice: string, card = templates, base?: string) =>
    renderMirrorTexts(
      card,
      entry,
      base === undefined ? model : { ...model, base_model: base },
      notice,
      licences,
    );
  return { at, texts };
}

test("a NOTICE short commit that disagrees is refused", () => {
  const got = canary();
  const run = () => got.texts(got.at("bbbbbbb"));
  expect(run).toThrow(/bbbbbbb does not match/);
});

test("a rendered card with a leftover placeholder is refused", () => {
  const got = canary();
  const open = "{still_open}";
  const bad = {
    readme: `${open}\n`,
    attribution:
      "## This copy\nkept\n## Licence blocks\n### CC-BY-4.0\n" + open,
  };
  const hit = () => got.texts(got.at("aaaaaaa"), bad);
  expect(hit).toThrow(/placeholder/);
  const unknown = () =>
    got.texts(got.at("aaaaaaa"), templates, "mystery/example");
  expect(unknown).toThrow(/placeholder/);
});

test("no stated copyright renders None supplied upstream", () => {
  const got = canary();
  const card = got.texts(got.at("aaaaaaa"));
  const text = `${card.readme}\n${card.attribution}`;
  expect(text).toContain("None supplied upstream");
  expect(text).toContain(FULL);
  expect(text).not.toContain("{UNKNOWN:");
});
