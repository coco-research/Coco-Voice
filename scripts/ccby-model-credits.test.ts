import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  CC_BY_DETAILS_I18N_KEY,
  CC_BY_LICENCE,
  CC_BY_LICENCE_URL,
  formatCcByCreditDetails,
  getCcByModelCredits,
  type RawLicenceEntry,
} from "../src/components/settings/about/ccByModelCredits";

const root = join(import.meta.dir, "..");

const EN_UNCHANGED =
  "Licensed under CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/).";
const EN_CHANGED_TEMPLATE =
  "Licensed under CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Changed: converted to {{format}} by {{converter}}.";

function loadLicenceModels(): Record<string, RawLicenceEntry> {
  const parsed = JSON.parse(
    readFileSync(
      join(root, "src-tauri/src/catalog/model_licences.json"),
      "utf8",
    ),
  ) as { models: Record<string, RawLicenceEntry> };
  return parsed.models;
}

function loadLocaleJson(lang: string): {
  settings: { about: { acknowledgments: Record<string, unknown> } };
} {
  return JSON.parse(
    readFileSync(
      join(root, "src/i18n/locales", lang, "translation.json"),
      "utf8",
    ),
  ) as { settings: { about: { acknowledgments: Record<string, unknown> } } };
}

function englishTranslator(
  key: string,
  options?: Record<string, unknown>,
): string {
  const ack = loadLocaleJson("en").settings.about.acknowledgments;
  const leaf = key.split(".").pop()!;
  const contextKey =
    typeof options?.context === "string" ? `${leaf}_${options.context}` : leaf;
  let value = String(ack[contextKey] ?? ack[leaf] ?? key);
  if (options) {
    for (const [k, v] of Object.entries(options)) {
      if (k === "context") continue;
      value = value.replaceAll(`{{${k}}}`, String(v));
    }
  }
  return value;
}

function readmeSpeechModelLines(): string[] {
  const readme = readFileSync(join(root, "README.md"), "utf8");
  const creditsIdx = readme.indexOf("## Credits");
  expect(creditsIdx).toBeGreaterThan(-1);
  const creditsSection = readme.slice(creditsIdx);
  const heading = "### Speech models (CC BY 4.0)";
  const start = creditsSection.indexOf(heading);
  expect(start).toBeGreaterThan(-1);
  const afterHeading = creditsSection.slice(start + heading.length);
  const nextSection = afterHeading.search(/\n## /);
  const block =
    nextSection === -1 ? afterHeading : afterHeading.slice(0, nextSection);
  return block
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("- "));
}

test("every CC-BY-4.0 entry has title, creator, notice, url, and change metadata", () => {
  const models = loadLicenceModels();
  const ccByEntries = Object.entries(models).filter(
    ([, entry]) => entry.licence === CC_BY_LICENCE,
  );
  expect(ccByEntries.length).toBeGreaterThan(0);

  for (const [id, entry] of ccByEntries) {
    expect(typeof entry.title, `${id}.title`).toBe("string");
    expect(
      entry.title?.trim().length ?? 0,
      `${id}.title non-empty`,
    ).toBeGreaterThan(0);
    expect(typeof entry.creator, `${id}.creator`).toBe("string");
    expect(
      entry.creator?.trim().length ?? 0,
      `${id}.creator non-empty`,
    ).toBeGreaterThan(0);
    expect(typeof entry.notice, `${id}.notice`).toBe("string");
    expect(entry.url.startsWith("https://"), `${id}.url https`).toBe(true);
    expect(
      entry.url.includes(entry.revision),
      `${id}.url pinned revision`,
    ).toBe(true);
    expect(typeof entry.changed, `${id}.changed boolean`).toBe("boolean");

    if (entry.changed) {
      expect(typeof entry.format, `${id}.format`).toBe("string");
      expect(
        entry.format?.trim().length ?? 0,
        `${id}.format non-empty`,
      ).toBeGreaterThan(0);
      expect(typeof entry.converter, `${id}.converter`).toBe("string");
      expect(
        entry.converter?.trim().length ?? 0,
        `${id}.converter non-empty`,
      ).toBeGreaterThan(0);
    }
  }
});

test("K1 i18n strings exist in every locale with exact URL and placeholders", () => {
  const enAck = loadLocaleJson("en").settings.about.acknowledgments;
  expect(enAck.ccByModelDetails).toBe(EN_UNCHANGED);
  expect(enAck.ccByModelDetails_changed).toBe(EN_CHANGED_TEMPLATE);

  const localesDir = join(root, "src/i18n/locales");
  const langs = readdirSync(localesDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort();

  for (const lang of langs) {
    const ack = loadLocaleJson(lang).settings.about.acknowledgments;
    const unchanged = String(ack.ccByModelDetails ?? "");
    const changed = String(ack.ccByModelDetails_changed ?? "");
    expect(unchanged, `${lang} unchanged URL`).toContain(CC_BY_LICENCE_URL);
    expect(changed, `${lang} changed URL`).toContain(CC_BY_LICENCE_URL);
    expect(changed, `${lang} {{format}}`).toContain("{{format}}");
    expect(changed, `${lang} {{converter}}`).toContain("{{converter}}");
  }
});

test("formatCcByCreditDetails selects the exact K1 variant for changed vs unchanged", () => {
  const changedText = formatCcByCreditDetails(
    {
      changed: true,
      format: "GGUF",
      converter: "handy-computer",
      notice: "",
    },
    englishTranslator,
  );
  expect(changedText).toBe(
    `Licensed under CC BY 4.0 (${CC_BY_LICENCE_URL}). Changed: converted to GGUF by handy-computer.`,
  );

  const unchangedText = formatCcByCreditDetails(
    {
      changed: false,
      format: "",
      converter: "",
      notice: "",
    },
    englishTranslator,
  );
  expect(unchangedText).toBe(EN_UNCHANGED);
  expect(unchangedText).not.toContain("Changed:");

  const withNotice = formatCcByCreditDetails(
    {
      changed: true,
      format: "GGUF",
      converter: "handy-computer",
      notice: "Copyright Example.",
    },
    englishTranslator,
  );
  expect(withNotice).toBe(
    `Licensed under CC BY 4.0 (${CC_BY_LICENCE_URL}). Changed: converted to GGUF by handy-computer. Copyright Example.`,
  );
});

test("every CC-BY-4.0 catalog entry has a generated Acknowledgments entry using its exact K1 variant", () => {
  const models = loadLicenceModels();
  const ccByIds = Object.entries(models)
    .filter(([, entry]) => entry.licence === CC_BY_LICENCE)
    .map(([id]) => id)
    .sort();

  const credits = getCcByModelCredits(models);
  const coveredIds = credits.flatMap((c) => c.catalogIds).sort();
  expect(coveredIds).toEqual(ccByIds);

  for (const credit of credits) {
    const details = formatCcByCreditDetails(credit, englishTranslator);
    expect(details).toContain(CC_BY_LICENCE_URL);
    if (credit.changed) {
      expect(details).toContain(
        `Changed: converted to ${credit.format} by ${credit.converter}.`,
      );
    } else {
      expect(details).not.toContain("Changed:");
    }
    if (credit.notice.trim()) {
      expect(details).toContain(credit.notice.trim());
    }
  }

  // Confirm AboutSettings renders CC_BY_MODEL_CREDITS unconditionally (outside SPEAKER_SPLIT_ENABLED).
  const aboutSrc = readFileSync(
    join(root, "src/components/settings/about/AboutSettings.tsx"),
    "utf8",
  );
  expect(aboutSrc).toContain("CC_BY_MODEL_CREDITS.map");
  expect(aboutSrc).toContain("formatCcByCreditDetails");
  const speakerSplitIdx = aboutSrc.indexOf("SPEAKER_SPLIT_ENABLED");
  const ccByIdx = aboutSrc.indexOf("CC_BY_MODEL_CREDITS.map");
  expect(ccByIdx).toBeGreaterThan(speakerSplitIdx);
  expect(CC_BY_DETAILS_I18N_KEY).toBe(
    "settings.about.acknowledgments.ccByModelDetails",
  );
});

test("README Credits lists every CC-BY-4.0 upstream source with its pinned link, creator, change note, and notice", () => {
  const credits = getCcByModelCredits(loadLicenceModels());
  const lines = readmeSpeechModelLines();
  expect(lines.length).toBe(credits.length);

  for (const credit of credits) {
    const match = lines.find((line) => line.includes(credit.url));
    expect(
      match,
      `README line for ${credit.publisher} (${credit.url})`,
    ).toBeDefined();
    expect(match!).toContain(credit.title);
    expect(match!).toContain(credit.creator);
    expect(match!).toContain(CC_BY_LICENCE_URL);
    if (credit.changed) {
      expect(match!).toContain(
        `Changed: converted to ${credit.format} by ${credit.converter}.`,
      );
    } else {
      expect(match!).not.toContain("Changed:");
    }
    if (credit.notice.trim()) {
      expect(match!).toContain(credit.notice.trim());
    }
  }
});
