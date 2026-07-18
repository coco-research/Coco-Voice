import type { CorrectionPair } from "@/bindings";

/**
 * Auto-learn: derive deterministic `{from, to}` correction pairs from a manual
 * edit of a transcription result.
 *
 * This is intentionally conservative. It only learns from edits that look like a
 * small number of in-place, single-word substitutions — the case where the user
 * fixed a single misheard word (e.g. "cocoa" -> "CoCo"). Anything that changes
 * the number of words (insertions, deletions, reordering) or touches more than a
 * couple of words is treated as a rewrite and ignored, so the correction map is
 * never polluted with phrase-level or structural edits.
 *
 * The pairs returned here are fed straight into the existing correction map via
 * the `update_corrections` command, so they follow the same semantics as
 * manually added corrections: a case-insensitive, whole-word `from` -> `to`
 * replacement applied as the final output pass.
 */

// Ignore edits that change more than this many words: past this it is a rewrite,
// not a targeted word fix.
const MAX_WORD_CHANGES = 2;
// Mirror the length guard the Corrections settings UI enforces on manual entry.
const MAX_WORD_LEN = 100;

// Strip leading/trailing punctuation and quotes so "world." -> "world"; we learn
// the bare word rather than the surrounding punctuation. Unicode letter/number
// classes keep non-Latin scripts intact.
const EDGE_PUNCT = /^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu;
const HAS_ALNUM = /[\p{L}\p{N}]/u;

function coreWord(token: string): string {
  return token.replace(EDGE_PUNCT, "");
}

/**
 * Compare the original transcription with the user's edited version and return
 * the correction pairs worth learning. Returns an empty array when the edit is
 * not a clean set of single-word substitutions, or when every candidate pair is
 * already present in `existing`.
 */
export function learnCorrectionsFromEdit(
  original: string,
  edited: string,
  existing: CorrectionPair[] = [],
): CorrectionPair[] {
  const before = original.trim().split(/\s+/).filter(Boolean);
  const after = edited.trim().split(/\s+/).filter(Boolean);

  // A different word count means words were added/removed/reordered — that is a
  // rewrite, not an in-place swap, so we learn nothing.
  if (before.length === 0 || before.length !== after.length) {
    return [];
  }

  const changedIndexes: number[] = [];
  for (let i = 0; i < before.length; i++) {
    if (before[i] !== after[i]) {
      changedIndexes.push(i);
    }
  }

  // No change at all, or too many changes (a rewrite) → learn nothing.
  if (
    changedIndexes.length === 0 ||
    changedIndexes.length > MAX_WORD_CHANGES
  ) {
    return [];
  }

  const existingFroms = new Set(existing.map((p) => p.from.toLowerCase()));
  const seen = new Set<string>();
  const learned: CorrectionPair[] = [];

  for (const i of changedIndexes) {
    const from = coreWord(before[i]);
    const to = coreWord(after[i]);

    // Skip edits that only touched surrounding punctuation, empty cores, or
    // over-long tokens.
    if (!from || !to) continue;
    if (from.length > MAX_WORD_LEN || to.length > MAX_WORD_LEN) continue;
    if (!HAS_ALNUM.test(from) || !HAS_ALNUM.test(to)) continue;
    // Core word unchanged (only the surrounding punctuation moved).
    if (from === to) continue;

    const key = from.toLowerCase();
    if (existingFroms.has(key) || seen.has(key)) continue;

    seen.add(key);
    learned.push({ from, to });
  }

  return learned;
}
