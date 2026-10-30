import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

const OPENING =
  "Coco Voice doesn't own these models. Each one is made by the publisher listed and is provided under the licence shown. Where Coco hosts a copy, it is unmodified.";

const FORBIDDEN = [
  "cleared",
  "approved by legal",
  "safe for commercial use",
  "compliant",
];

type CatalogModel = { id: string; license: string };

function page(): string {
  return readFileSync(join(root, "sbom/model-licences.md"), "utf8");
}

function catalogModels(): CatalogModel[] {
  const catalog = JSON.parse(
    readFileSync(join(root, "src-tauri/src/catalog/catalog.json"), "utf8"),
  ) as { models: CatalogModel[] };
  return catalog.models;
}

function holdIds(): string[] {
  const src = readFileSync(join(root, "src-tauri/src/catalog/mod.rs"), "utf8");
  const block = src.match(/pub const LEGAL_HOLD_IDS[\s\S]*?\];/)?.[0] ?? "";
  return [...block.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

function tableRows(text: string): string[][] {
  return text
    .split("\n")
    .filter((line) => line.startsWith("|"))
    .filter((line) => !line.includes("---"))
    .slice(1)
    .map((line) =>
      line
        .split("|")
        .slice(1, -1)
        .map((cell) => cell.trim()),
    );
}

function modelId(cell: string): string {
  return cell.replaceAll("`", "");
}

test("opening line is present exactly", () => {
  const text = page();
  expect(text.startsWith(OPENING)).toBe(true);
  expect(text.split("\n")[0]).toBe(OPENING);
});

test("every catalog id has a row and no other model does", () => {
  const ids = catalogModels()
    .map((model) => model.id)
    .sort();
  const rowIds = tableRows(page())
    .map((cells) => modelId(cells[0]))
    .sort();
  expect(rowIds).toEqual(ids);
});

test("no held id appears", () => {
  const held = holdIds();
  expect(held.length).toBe(20);
  const text = page();
  expect(held.filter((id) => text.includes(id))).toEqual([]);
});

test("none of the four forbidden phrases appears", () => {
  const lower = page().toLowerCase();
  expect(FORBIDDEN.filter((phrase) => lower.includes(phrase))).toEqual([]);
});

// The page shows the SPDX ID; the catalog stores it lowercase.
const SPDX: Record<string, string> = {
  mit: "MIT",
  "apache-2.0": "Apache-2.0",
  "cc-by-4.0": "CC-BY-4.0",
};

test("each row's licence is the SPDX ID of the catalog's licence", () => {
  const byId = new Map(
    catalogModels().map((model) => [model.id, SPDX[model.license]]),
  );
  const mismatches = tableRows(page()).flatMap((cells) => {
    const id = modelId(cells[0]);
    const licence = cells[2].match(/\[([^\]]+)\]/)?.[1] ?? cells[2];
    return licence === byId.get(id) ? [] : [`${id}: ${licence}`];
  });
  expect(mismatches).toEqual([]);
});
