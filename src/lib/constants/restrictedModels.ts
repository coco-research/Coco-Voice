import type { ModelInfo } from "@/bindings";
import restricted from "./restricted-models.json";

const QUANT = /^(?:IQ\d+\w*|Q\d[\w_]*|F16|BF16|F32)$/i;

function matchesSlug(
  model: Pick<ModelInfo, "id" | "filename">,
  slug: string,
): boolean {
  if (model.id.includes(`handy-computer/${slug}-gguf`)) return true;
  const file = (model.filename || model.id).split("/").pop() ?? "";
  const stem = file.replace(/\.gguf$/i, "");
  if (stem === slug) return true;
  return (
    stem.startsWith(`${slug}-`) && QUANT.test(stem.slice(slug.length + 1))
  );
}

/** Label for a model we no longer offer for download. Null if we still ship it. */
export function licenseNotice(
  model: Pick<ModelInfo, "id" | "filename">,
): string | null {
  for (const slug of restricted.review) {
    if (matchesSlug(model, slug)) return restricted.notices.review;
  }
  for (const slug of restricted.noncommercial) {
    if (matchesSlug(model, slug)) return restricted.notices.noncommercial;
  }
  return null;
}
