import modelLicences from "../../../../src-tauri/src/catalog/model_licences.json";

export const CC_BY_LICENCE = "CC-BY-4.0";
export const CC_BY_DISPLAY_LICENCE = "CC BY 4.0";
export const CC_BY_LICENCE_URL = "https://creativecommons.org/licenses/by/4.0/";
export const CC_BY_DETAILS_I18N_KEY =
  "settings.about.acknowledgments.ccByModelDetails";

export interface RawLicenceEntry {
  licence: string;
  publisher: string;
  revision: string;
  url: string;
  title?: string;
  creator?: string;
  notice?: string;
  changed?: boolean;
  format?: string;
  converter?: string;
}

export interface CcByModelCredit {
  id: string;
  publisher: string;
  revision: string;
  url: string;
  title: string;
  creator: string;
  notice: string;
  changed: boolean;
  format: string;
  converter: string;
  catalogIds: string[];
}

export function getCcByModelCredits(
  models: Record<string, RawLicenceEntry> = modelLicences.models,
): CcByModelCredit[] {
  const groups = new Map<
    string,
    {
      publisher: string;
      revision: string;
      url: string;
      title: string;
      creator: string;
      notice: string;
      catalogIds: string[];
      conversions: Array<{ format: string; converter: string }>;
      changed: boolean;
    }
  >();

  for (const catalogId of Object.keys(models).sort()) {
    const entry = models[catalogId];
    if (!entry || entry.licence !== CC_BY_LICENCE) continue;

    const groupKey = `${entry.publisher}@${entry.revision}`;
    let group = groups.get(groupKey);
    if (!group) {
      group = {
        publisher: entry.publisher,
        revision: entry.revision,
        url: entry.url,
        title: entry.title ?? "",
        creator: entry.creator ?? "",
        notice: entry.notice ?? "",
        catalogIds: [],
        conversions: [],
        changed: false,
      };
      groups.set(groupKey, group);
    }

    group.catalogIds.push(catalogId);
    if (entry.changed) {
      group.changed = true;
      const format = entry.format ?? "";
      const converter = entry.converter ?? "";
      if (
        !group.conversions.some(
          (c) => c.format === format && c.converter === converter,
        )
      ) {
        group.conversions.push({ format, converter });
      }
    }
  }

  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([id, group]) => {
      // Keep GGUF before ONNX when an upstream source covers both catalog and legacy IDs.
      const orderedConversions = [...group.conversions].sort((a, b) =>
        a.format.localeCompare(b.format),
      );
      return {
        id,
        publisher: group.publisher,
        revision: group.revision,
        url: group.url,
        title: group.title,
        creator: group.creator,
        notice: group.notice,
        changed: group.changed,
        format: orderedConversions.map((c) => c.format).join(" / "),
        converter: orderedConversions.map((c) => c.converter).join(" / "),
        catalogIds: group.catalogIds,
      };
    });
}

export const CC_BY_MODEL_CREDITS: CcByModelCredit[] = getCcByModelCredits();

export function formatCcByCreditDetails(
  credit: Pick<CcByModelCredit, "changed" | "format" | "converter" | "notice">,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const base = credit.changed
    ? t(CC_BY_DETAILS_I18N_KEY, {
        context: "changed",
        format: credit.format,
        converter: credit.converter,
      })
    : t(CC_BY_DETAILS_I18N_KEY);
  const notice = credit.notice.trim();
  return notice ? `${base} ${notice}` : base;
}
