import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const mirrorDir = join(root, "scripts/mirror");
const readme = readFileSync(join(mirrorDir, "README-card.template.md"), "utf8");
const attribution = readFileSync(
  join(mirrorDir, "ATTRIBUTION.template.md"),
  "utf8",
);

// The notes name the banned words and are deleted before publishing.
// "Unofficial" is required, so the ban is the whole word.
function cardText(text: string): string {
  return text.split("## Template notes, delete before publishing")[0] ?? "";
}

const cards = `${cardText(readme)}\n${cardText(attribution)}`;

test("templates carry Kanika's required sentences", () => {
  const sentences = [
    "Unofficial mirror of {model} by {publisher}, converted to GGUF " +
      "by handy-computer. Not affiliated with or endorsed by {publisher}.",
    "Redistributed unmodified by Coco Research.",
    "Converted to GGUF by handy-computer; this is a modified format " +
      "of the original.",
    "Copyright (c) 2022 OpenAI",
    "Unofficial mirror of Qwen3-4B-GGUF by Qwen, as published by Qwen. " +
      "Not affiliated with or endorsed by Qwen.",
  ];
  for (const sentence of sentences) expect(cards).toContain(sentence);
});

test("templates do not say official, Coco models, donate, or sponsor", () => {
  const banned = [
    /\bofficial\b/i,
    /coco models/i,
    /\bdonate\b/i,
    /\bsponsor\b/i,
  ];
  for (const pattern of banned) expect(cards).not.toMatch(pattern);
});
