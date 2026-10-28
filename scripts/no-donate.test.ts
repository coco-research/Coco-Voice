import { expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const forbidden = ["donate", "cocovoice.ai", "supportdevelopment"];

function filesToScan(): string[] {
  const locales = join(root, "src/i18n/locales");
  const translations = readdirSync(locales)
    .filter((name) => statSync(join(locales, name)).isDirectory())
    .map((name) => join(locales, name, "translation.json"));

  return [
    join(root, "src/components/settings/about/AboutSettings.tsx"),
    join(root, "CONTRIBUTING.md"),
    join(root, "README.md"),
    ...translations,
  ];
}

test("no donate link or unowned cocovoice.ai domain", () => {
  const hits: string[] = [];

  for (const file of filesToScan()) {
    const text = readFileSync(file, "utf8").toLowerCase();
    for (const needle of forbidden) {
      if (text.includes(needle)) {
        hits.push(`${file} contains ${needle}`);
      }
    }
  }

  expect(hits).toEqual([]);
});
