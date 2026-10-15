import type { CleanupStatusReason } from "@/bindings";

/** The English `cleanupStatus` strings: the keys every other locale must have. */
export type CleanupStrings =
  (typeof import("@/i18n/locales/en/translation.json"))["cleanupStatus"];

/**
 * The key of the words a reason is shown with, under `cleanupStatus.chip` and
 * `cleanupStatus.sentence`. Reasons that read alike share their words. Typed
 * from the English file, so a note without both strings does not compile.
 */
export type CleanupNote = keyof CleanupStrings["chip"] &
  keyof CleanupStrings["sentence"];

/** A Record, so a reason the backend adds cannot ship without its words. */
export const CLEANUP_NOTE: Record<CleanupStatusReason, CleanupNote> = {
  local_model_not_downloaded: "noModel",
  no_api_key: "noApiKey",
  no_model_configured: "noModelChosen",
  no_prompt_selected: "noPrompt",
  prompt_not_found: "noPrompt",
  prompt_empty: "noPrompt",
  no_provider_selected: "noProvider",
  provider_not_found: "noProvider",
  provider_unavailable_in_this_build: "appleOff",
  apple_intelligence_unavailable: "appleOff",
  too_long: "tooLong",
  stopped_early: "stoppedEarly",
  model_load_failed: "modelFailed",
};
