import React, { useEffect, useMemo, useRef, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Globe, RefreshCw } from "lucide-react";
import type { ModelCardStatus } from "@/components/onboarding";
import RecommendedBadge from "@/components/onboarding/RecommendedBadge";
import { useModelStore } from "@/stores/modelStore";
import { useSettings } from "@/hooks/useSettings";
import {
  getLanguageLabel,
  MODEL_CAPABILITY_LANGUAGES,
  supportsLanguageCode,
} from "@/lib/constants/languages.ts";
import { formatModelSize } from "@/lib/utils/format";
import type { DownloadFailureCause, ModelInfo } from "@/bindings";

const DOT_FILLED = "●";

const modelSupportsLanguage = (model: ModelInfo, langCode: string): boolean => {
  return supportsLanguageCode(model.supported_languages, langCode);
};

const LOCAL_LLM_DEFAULT_MODEL_ID = "Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf";

const isCleanupLlmModel = (model: ModelInfo): boolean => {
  return model.engine_type === "LlamaCpp";
};

export function formatCleanModelSize(sizeMb: number): string {
  return formatModelSize(sizeMb).replace(/([.,])0(?=\s*(?:MB|GB)\b)/, "");
}

export function formatSpeechModelSubtitle(
  model: ModelInfo,
  t: TFunction,
): string {
  const idLower = model.id.toLowerCase();
  const sizeText = formatCleanModelSize(model.size_mb);
  const desc = (model.description ?? "").trim();

  if (idLower.includes("parakeet") && idLower.includes("v3")) {
    return [
      t("v022.models.parakeetV3Languages", "English and 24 European languages"),
      t("v022.models.noteFast", "fast"),
      sizeText,
    ]
      .filter(Boolean)
      .join(" · ");
  }

  if (idLower.includes("whisper") && !idLower.includes(".en")) {
    return [t("v022.models.whisper99Languages", "99 languages"), sizeText]
      .filter(Boolean)
      .join(" · ");
  }

  if (idLower.includes("moonshine") && idLower.includes("base")) {
    return [
      t("v022.models.englishOnly", "English"),
      t("v022.models.noteSmallest", "smallest"),
      sizeText,
    ]
      .filter(Boolean)
      .join(" · ");
  }

  const descMentionsLang =
    /\b(\d+[- ]languages?|english|russian|mandarin|european)\b/i.test(desc);
  if (descMentionsLang) {
    return [desc, sizeText].filter(Boolean).join(" · ");
  }

  const langCount = model.supported_languages?.length ?? 0;
  const langText =
    langCount > 1
      ? t("v022.models.languageCount", {
          count: langCount,
          defaultValue: `${langCount} languages`,
        })
      : langCount === 1
        ? getLanguageLabel(model.supported_languages[0]) ||
          model.supported_languages[0]
        : "";
  return [langText, desc, sizeText]
    .filter((s) => Boolean(s && s.trim()))
    .join(" · ");
}

function formatDownloadFailure(
  cause: DownloadFailureCause,
  t: TFunction,
): string {
  switch (cause) {
    case "network":
      return t(
        "v022.models.downloadFailedNetwork",
        "Download failed: check your connection.",
      );
    case "disk_space":
      return t(
        "v022.models.downloadFailedDiskSpace",
        "Download failed: not enough free disk space.",
      );
    case "checksum":
      return t(
        "v022.models.downloadFailedChecksum",
        "Download failed: file check did not match.",
      );
    default:
      return t("v022.models.downloadFailedOther", "Download failed.");
  }
}

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, setPostProcessProvider, updateSetting } = useSettings();
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [languageFilter, setLanguageFilter] = useState("all");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const languageSearchInputRef = useRef<HTMLInputElement>(null);
  const {
    models,
    currentModel,
    downloadingModels,
    downloadProgress,
    downloadFailures,
    verifyingModels,
    extractingModels,
    loading,
    isRescanning,
    downloadModel,
    cancelDownload,
    selectModel,
    deleteModel,
    rescanLocalModels,
  } = useModelStore();

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        languageDropdownRef.current &&
        !languageDropdownRef.current.contains(event.target as Node)
      ) {
        setLanguageDropdownOpen(false);
        setLanguageSearch("");
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  useEffect(() => {
    if (languageDropdownOpen && languageSearchInputRef.current) {
      languageSearchInputRef.current.focus();
    }
  }, [languageDropdownOpen]);

  const filteredLanguages = useMemo(() => {
    return MODEL_CAPABILITY_LANGUAGES.filter((lang) =>
      lang.label.toLowerCase().includes(languageSearch.toLowerCase()),
    );
  }, [languageSearch]);

  const selectedLanguageLabel = useMemo(() => {
    if (languageFilter === "all") {
      return t("settings.models.filters.allLanguages");
    }
    return getLanguageLabel(languageFilter) || "";
  }, [languageFilter, t]);

  const getModelStatus = (modelId: string): ModelCardStatus => {
    if (modelId in extractingModels) return "extracting";
    if (modelId in verifyingModels) return "verifying";
    if (modelId in downloadingModels) return "downloading";
    if (switchingModelId === modelId) return "switching";
    if (modelId === currentModel) return "active";
    const model = models.find((m: ModelInfo) => m.id === modelId);
    if (model?.is_downloaded) return "available";
    return "downloadable";
  };

  const handleModelSelect = async (modelId: string) => {
    setSwitchingModelId(modelId);
    try {
      await selectModel(modelId);
    } finally {
      setSwitchingModelId(null);
    }
  };

  const handleModelDownload = async (modelId: string) => {
    await downloadModel(modelId);
  };

  const handleModelDelete = async (modelId: string) => {
    const model = models.find((m: ModelInfo) => m.id === modelId);
    const modelName = model?.name || modelId;
    const isActive = modelId === currentModel;

    const confirmed = await ask(
      isActive
        ? t("settings.models.deleteActiveConfirm", { modelName })
        : t("settings.models.deleteConfirm", { modelName }),
      {
        title: t("settings.models.deleteTitle"),
        kind: "warning",
      },
    );

    if (confirmed) {
      try {
        await deleteModel(modelId);
      } catch (err) {
        console.error(`Failed to delete model ${modelId}:`, err);
      }
    }
  };

  const handleModelCancel = async (modelId: string) => {
    try {
      await cancelDownload(modelId);
    } catch (err) {
      console.error(`Failed to cancel download for ${modelId}:`, err);
    }
  };

  const { speechModels, localCleanupModel } = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const cleanupModels = models.filter(isCleanupLlmModel);
    const cleanupCandidate =
      cleanupModels.find((m) => m.id === LOCAL_LLM_DEFAULT_MODEL_ID) ??
      cleanupModels[0] ??
      null;

    const speech = models.filter((model: ModelInfo) => {
      if (isCleanupLlmModel(model)) return false;
      if (languageFilter !== "all") {
        if (!modelSupportsLanguage(model, languageFilter)) return false;
      }
      if (q) {
        const haystack = `${model.name} ${model.description}`.toLowerCase();
        if (!haystack.includes(q)) return false;
      }
      return true;
    });

    speech.sort((a, b) => {
      if (a.id === currentModel) return -1;
      if (b.id === currentModel) return 1;
      const aMeasured =
        a.measured_best !== null && a.measured_best !== undefined;
      const bMeasured =
        b.measured_best !== null && b.measured_best !== undefined;
      if (aMeasured !== bMeasured) return aMeasured ? -1 : 1;
      if (a.is_downloaded !== b.is_downloaded) return a.is_downloaded ? -1 : 1;
      if (a.is_custom !== b.is_custom) return a.is_custom ? 1 : -1;
      return 0;
    });

    return { speechModels: speech, localCleanupModel: cleanupCandidate };
  }, [models, languageFilter, searchQuery, currentModel]);

  // AC-D13: Local clean-up model (Qwen3-4B) is "In use" ONLY when post-processing
  // is enabled, provider is specifically "local_llm" (never Apple Intelligence or cloud),
  // AND the local model is actually downloaded on disk.
  const cleanupProviderId = settings?.post_process_provider_id || "";
  const selectedLocalModel = settings?.post_process_models?.local_llm || "";
  const isLocalCleanupInUse = Boolean(
    settings?.post_process_enabled &&
    cleanupProviderId === "local_llm" &&
    localCleanupModel?.is_downloaded &&
    (!selectedLocalModel || selectedLocalModel === localCleanupModel.id),
  );

  const cleanupStatus = localCleanupModel
    ? getModelStatus(localCleanupModel.id)
    : "downloadable";
  const cleanupFailure = localCleanupModel
    ? downloadFailures[localCleanupModel.id]
    : undefined;
  const cleanupPct = localCleanupModel
    ? Math.round(downloadProgress[localCleanupModel.id]?.percentage ?? 0)
    : 0;

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto">
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
        </div>
      </div>
    );
  }

  return (
    <div
      data-route-domain="shared"
      className="max-w-3xl w-full mx-auto space-y-3"
    >
      <div>
        <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
          {t("sidebar.models")}
        </p>
        <div className="mt-0.5 flex flex-wrap items-center justify-between gap-3">
          <h1 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
            {t("sidebar.models")}
          </h1>

          <div className="flex flex-wrap items-center gap-2">
            <input
              type="search"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={t("v022.models.searchModels", "Search models")}
              aria-label={t("v022.models.searchModels", "Search models")}
              className="w-48 rounded-lg border border-mid-gray/40 bg-background px-3 py-1.5 text-xs text-text placeholder:text-mid-gray focus:outline-none focus:ring-1 focus:ring-logo-primary"
            />
            <button
              type="button"
              onClick={() => void rescanLocalModels()}
              disabled={isRescanning}
              title={t("settings.models.rescan.tooltip", "Rescan local models")}
              className="flex items-center gap-1.5 rounded-lg border border-mid-gray/35 bg-surface px-2.5 py-1.5 text-xs text-text/80 hover:text-text hover:bg-mid-gray/10 transition-colors cursor-pointer disabled:opacity-50"
            >
              <RefreshCw
                className={`w-3.5 h-3.5 ${isRescanning ? "animate-spin" : ""}`}
              />
              <span>
                {isRescanning
                  ? t("settings.models.rescanning", "Rescanning…")
                  : t("settings.models.rescan.label", "Rescan")}
              </span>
            </button>

            <div className="relative" ref={languageDropdownRef}>
              <button
                type="button"
                onClick={() => setLanguageDropdownOpen(!languageDropdownOpen)}
                className={`flex items-center gap-1.5 px-2.5 py-1.5 text-xs rounded-lg border transition-colors cursor-pointer ${
                  languageFilter !== "all"
                    ? "border-logo-primary/40 bg-logo-primary/15 text-logo-primary font-semibold"
                    : "border-mid-gray/35 bg-surface text-text/80 hover:bg-mid-gray/10"
                }`}
              >
                <Globe className="w-3.5 h-3.5" />
                <span className="max-w-[110px] truncate">
                  {selectedLanguageLabel}
                </span>
                <ChevronDown
                  className={`w-3.5 h-3.5 transition-transform ${
                    languageDropdownOpen ? "rotate-180" : ""
                  }`}
                />
              </button>

              {languageDropdownOpen && (
                <div className="absolute end-0 mt-1 w-56 bg-background border border-mid-gray/30 rounded-lg shadow-lg z-50 overflow-hidden">
                  <div className="p-2 border-b border-mid-gray/20">
                    <input
                      ref={languageSearchInputRef}
                      type="text"
                      value={languageSearch}
                      onChange={(e) => setLanguageSearch(e.target.value)}
                      placeholder={t(
                        "settings.general.language.searchPlaceholder",
                      )}
                      className="w-full px-2.5 py-1.5 text-xs bg-mid-gray/10 border border-mid-gray/20 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
                    />
                  </div>
                  <div className="max-h-56 overflow-y-auto py-1">
                    <button
                      type="button"
                      onClick={() => {
                        setLanguageFilter("all");
                        setLanguageDropdownOpen(false);
                        setLanguageSearch("");
                      }}
                      className={`w-full px-3 py-1.5 text-start text-xs transition-colors cursor-pointer ${
                        languageFilter === "all"
                          ? "bg-logo-primary/20 text-logo-primary font-semibold"
                          : "hover:bg-mid-gray/10"
                      }`}
                    >
                      {t("settings.models.filters.allLanguages")}
                    </button>
                    {filteredLanguages.map((lang) => (
                      <button
                        key={lang.value}
                        type="button"
                        onClick={() => {
                          setLanguageFilter(lang.value);
                          setLanguageDropdownOpen(false);
                          setLanguageSearch("");
                        }}
                        className={`w-full px-3 py-1.5 text-start text-xs transition-colors cursor-pointer ${
                          languageFilter === lang.value
                            ? "bg-logo-primary/20 text-logo-primary font-semibold"
                            : "hover:bg-mid-gray/10"
                        }`}
                      >
                        {lang.label}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Section 1: Speech: used for dictation and meetings */}
      <section data-testid="speech-models-section" className="space-y-2">
        <p className="text-xs text-mid-gray">
          {t(
            "v022.models.speechHeading",
            "Speech: used for dictation and meetings",
          )}
        </p>

        {speechModels.length > 0 ? (
          <div className="rounded-xl border border-mid-gray/25 bg-surface divide-y divide-mid-gray/20">
            {speechModels.map((model: ModelInfo) => {
              const status = getModelStatus(model.id);
              const failure = downloadFailures[model.id];
              const pct = Math.round(
                downloadProgress[model.id]?.percentage ?? 0,
              );
              const subtitleParts = formatSpeechModelSubtitle(model, t);

              return (
                <div
                  key={model.id}
                  data-model-row-id={model.id}
                  className="flex flex-wrap items-center justify-between gap-3 px-4 py-2.5"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-[14px] font-semibold text-text">
                        {model.name}
                      </span>
                      {model.measured_best ? (
                        <RecommendedBadge reason={model.measured_best} />
                      ) : null}
                    </div>
                    <p className="mt-0.5 text-xs text-mid-gray truncate">
                      {subtitleParts}
                    </p>
                    {failure &&
                    status !== "downloading" &&
                    status !== "verifying" &&
                    status !== "extracting" ? (
                      <p role="alert" className="mt-1 text-xs text-red-400">
                        {formatDownloadFailure(failure, t)}
                      </p>
                    ) : null}
                  </div>

                  <div className="flex shrink-0 items-center gap-2">
                    {status === "active" ? (
                      <span
                        data-testid={`model-in-use-${model.id}`}
                        className="inline-flex items-center gap-1.5 text-xs font-semibold text-emerald-600 dark:text-emerald-400"
                      >
                        <span aria-hidden="true">{DOT_FILLED}</span>
                        <span>{t("v022.models.inUse", "In use")}</span>
                      </span>
                    ) : status === "available" ? (
                      <>
                        <button
                          type="button"
                          onClick={() => void handleModelSelect(model.id)}
                          className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                        >
                          {t("v022.models.useButton", "Use")}
                        </button>
                        <button
                          type="button"
                          onClick={() => void handleModelDelete(model.id)}
                          className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                        >
                          {t("v022.models.deleteButton", "Delete")}
                        </button>
                      </>
                    ) : status === "downloading" ||
                      status === "verifying" ||
                      status === "extracting" ? (
                      <>
                        <span className="font-mono text-xs text-mid-gray">
                          {pct}%
                        </span>
                        <button
                          type="button"
                          onClick={() => void handleModelCancel(model.id)}
                          className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                        >
                          {t("settings.postProcessing.api.localLlm.cancel")}
                        </button>
                      </>
                    ) : failure ? (
                      <button
                        type="button"
                        onClick={() => void handleModelDownload(model.id)}
                        className="rounded-lg border border-red-500/40 px-2.5 py-1 text-xs font-semibold text-red-400 cursor-pointer hover:bg-red-500/10"
                      >
                        {t("v022.models.tryAgainButton", "Try again")}
                      </button>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void handleModelDownload(model.id)}
                        className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                      >
                        {t("v022.models.downloadButton", "Download")}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="rounded-xl border border-mid-gray/20 bg-surface py-6 text-center text-xs text-mid-gray">
            {t("settings.models.noModelsMatch")}
          </div>
        )}
      </section>

      {/* Section 2: Clean-up and notes: runs on this Mac */}
      <section
        data-testid="local-cleanup-models-section"
        className="space-y-2 pt-1"
      >
        <p className="text-xs text-mid-gray">
          {t(
            "v022.models.cleanupNotesHeading",
            "Clean-up and notes: runs on this Mac",
          )}
        </p>

        <div className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-2.5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0 flex-1">
              <div className="text-[14px] font-semibold text-text">
                {localCleanupModel?.name ||
                  t("v022.models.qwenTitle", "Qwen3-4B")}
              </div>
              <p className="mt-0.5 text-xs text-mid-gray">
                {t(
                  "v022.models.qwenDescription",
                  "Clean-up, meeting notes and Ask · 2.5 GB · Apache-2.0",
                )}
              </p>
              {cleanupFailure &&
              cleanupStatus !== "downloading" &&
              cleanupStatus !== "verifying" &&
              cleanupStatus !== "extracting" ? (
                <p role="alert" className="mt-1 text-xs text-red-400">
                  {formatDownloadFailure(cleanupFailure, t)}
                </p>
              ) : null}
            </div>

            <div className="flex shrink-0 items-center gap-2">
              {isLocalCleanupInUse ? (
                <span
                  data-testid="cleanup-model-in-use"
                  className="inline-flex items-center gap-1.5 text-xs font-semibold text-emerald-600 dark:text-emerald-400"
                >
                  <span aria-hidden="true">{DOT_FILLED}</span>
                  <span>{t("v022.models.inUse", "In use")}</span>
                </span>
              ) : localCleanupModel &&
                (cleanupStatus === "downloading" ||
                  cleanupStatus === "verifying" ||
                  cleanupStatus === "extracting") ? (
                <>
                  <span className="font-mono text-xs text-mid-gray">
                    {cleanupPct}%
                  </span>
                  <button
                    type="button"
                    onClick={() => void handleModelCancel(localCleanupModel.id)}
                    className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("settings.postProcessing.api.localLlm.cancel")}
                  </button>
                </>
              ) : localCleanupModel && cleanupFailure ? (
                <button
                  type="button"
                  onClick={() => void handleModelDownload(localCleanupModel.id)}
                  className="rounded-lg border border-red-500/40 px-2.5 py-1 text-xs font-semibold text-red-400 cursor-pointer hover:bg-red-500/10"
                >
                  {t("v022.models.tryAgainButton", "Try again")}
                </button>
              ) : !localCleanupModel || !localCleanupModel.is_downloaded ? (
                <button
                  type="button"
                  disabled={!localCleanupModel}
                  onClick={() =>
                    localCleanupModel &&
                    void handleModelDownload(localCleanupModel.id)
                  }
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10 disabled:opacity-50"
                >
                  {t("v022.models.downloadButton", "Download")}
                </button>
              ) : (
                <>
                  <button
                    type="button"
                    onClick={() => {
                      void updateSetting("post_process_enabled", true);
                      void setPostProcessProvider("local_llm");
                    }}
                    className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("v022.models.useButton", "Use")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void handleModelDelete(localCleanupModel.id)}
                    className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("v022.models.deleteButton", "Delete")}
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
};
