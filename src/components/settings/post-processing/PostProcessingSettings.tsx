import React, { useCallback, useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { RefreshCcw } from "lucide-react";
import { commands, type ModelInfo, type CleanupStatusReason } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";

const API_SECTION_ID = "post-process-api-section";
const API_KEY_FIELD_ID = "post-process-api-key-field";
const PROMPTS_SECTION_ID = "post-process-prompts-section";

const FOCUS_AFTER_SCROLL_MS = 300; // Delay to allow smooth scroll before focusing

const REASON_TO_SECTION_MAP: Record<CleanupStatusReason, string> = {
  no_prompt_selected: PROMPTS_SECTION_ID,
  prompt_not_found: PROMPTS_SECTION_ID,
  prompt_empty: PROMPTS_SECTION_ID,
  no_api_key: API_KEY_FIELD_ID,
  no_provider_selected: API_SECTION_ID,
  no_model_configured: API_SECTION_ID,
  provider_unavailable_in_this_build: API_SECTION_ID,
  apple_intelligence_unavailable: API_SECTION_ID,
  local_model_not_downloaded: API_SECTION_ID,
  provider_not_found: API_SECTION_ID,
  too_long: API_SECTION_ID,
  stopped_early: API_SECTION_ID,
  model_load_failed: API_SECTION_ID,
};

import { Alert } from "../../ui/Alert";
import {
  Dropdown,
  SettingContainer,
  SettingsGroup,
  Textarea,
} from "@/components/ui";
import { Button } from "../../ui/Button";
import { ResetButton } from "../../ui/ResetButton";
import { Input } from "../../ui/Input";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { ShortcutInput } from "../ShortcutInput";
import { IterativeCorrectionToggle } from "../IterativeCorrectionToggle";
import { AppProfiles } from "../AppProfiles";
import { useSettings } from "../../../hooks/useSettings";
import { useSettingsStore } from "@/stores/settingsStore";

/** Registry id of the on-device post-process GGUF.
 *  Same string as `LOCAL_LLM_DEFAULT_MODEL_ID`: `{catalog repo id}/{filename}`.
 *  `get_model_path` looks that id up directly.
 */
const LOCAL_LLM_PROVIDER_ID = "local_llm";
const LOCAL_LLM_DEFAULT_MODEL_ID = "Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf";

const LocalLlmModelRow: React.FC<{ modelId: string }> = ({ modelId }) => {
  const { t } = useTranslation();
  const [info, setInfo] = useState<ModelInfo | null | undefined>(undefined);
  const progress = useModelStore((s) => s.downloadProgress[modelId]);
  const downloading = useModelStore((s) => modelId in s.downloadingModels);
  const verifying = useModelStore((s) => modelId in s.verifyingModels);
  const downloadModel = useModelStore((s) => s.downloadModel);
  const cancelDownload = useModelStore((s) => s.cancelDownload);

  const refresh = useCallback(async () => {
    try {
      const result = await commands.getModelInfo(modelId);
      setInfo(result.status === "ok" ? result.data : null);
    } catch {
      setInfo(null);
    }
  }, [modelId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let active = true;
    const unlisteners: Array<() => void> = [];
    void Promise.all([
      listen<string>("model-download-complete", (event) => {
        if (event.payload === modelId) void refresh();
      }),
      listen<string>("model-download-cancelled", (event) => {
        if (event.payload === modelId) void refresh();
      }),
      listen<{ model_id: string }>("model-download-failed", (event) => {
        if (event.payload.model_id === modelId) void refresh();
      }),
    ])
      .then((fns) => {
        if (!active) {
          fns.forEach((fn) => fn());
          return;
        }
        unlisteners.push(...fns);
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisteners.forEach((fn) => fn());
    };
  }, [modelId, refresh]);

  const busy = downloading || verifying || Boolean(info?.is_downloading);
  const ready = Boolean(info?.is_downloaded) && !busy;
  const percent = Math.max(
    0,
    Math.min(100, Math.round(progress?.percentage ?? 0)),
  );

  let status = "";
  if (info === null) {
    status = t("settings.postProcessing.api.localLlm.unavailable");
  } else if (info) {
    if (busy) {
      status = t("settings.postProcessing.api.localLlm.downloading", {
        percent,
      });
    } else if (ready) {
      status = t("settings.postProcessing.api.localLlm.ready");
    } else {
      status = t("settings.postProcessing.api.localLlm.notDownloaded");
    }
  }

  return (
    <SettingContainer
      title={t("settings.postProcessing.api.localLlm.title")}
      description={t("settings.postProcessing.api.localLlm.description")}
      descriptionMode="tooltip"
      layout="horizontal"
      grouped={true}
    >
      <div className="flex items-center gap-2">
        {busy && (
          <div
            className="h-1.5 w-24 overflow-hidden rounded-full bg-mid-gray/20"
            role="progressbar"
            aria-valuenow={percent}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={status}
          >
            <div
              className="h-full rounded-full bg-logo-primary motion-safe:transition-[width] motion-safe:duration-300"
              style={{ width: `${percent}%` }}
            />
          </div>
        )}
        <span
          className="text-sm text-text/70 whitespace-nowrap"
          aria-live="polite"
        >
          {status}
        </span>
        {busy ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => {
              void cancelDownload(modelId).finally(() => {
                void refresh();
              });
            }}
          >
            {t("settings.postProcessing.api.localLlm.cancel")}
          </Button>
        ) : info && !ready ? (
          <Button
            type="button"
            variant="primary"
            size="sm"
            onClick={() => {
              void downloadModel(modelId).finally(() => {
                void refresh();
              });
            }}
          >
            {t("settings.postProcessing.api.localLlm.download")}
          </Button>
        ) : null}
      </div>
    </SettingContainer>
  );
};

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();
  const isLocalLlm = state.selectedProvider?.id === LOCAL_LLM_PROVIDER_ID;
  const localLlmModelId = state.model.trim() || LOCAL_LLM_DEFAULT_MODEL_ID;

  return (
    <>
      <SettingContainer
        title={t("settings.postProcessing.api.provider.title")}
        description={t("settings.postProcessing.api.provider.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ProviderSelect
            options={state.providerOptions}
            value={state.selectedProviderId}
            onChange={state.handleProviderSelect}
          />
        </div>
      </SettingContainer>

      {state.isAppleProvider ? (
        state.appleIntelligenceUnavailable ? (
          <Alert variant="error" contained>
            {t("settings.postProcessing.api.appleIntelligence.unavailable")}
          </Alert>
        ) : null
      ) : isLocalLlm ? (
        <LocalLlmModelRow modelId={localLlmModelId} />
      ) : (
        <>
          {state.selectedProvider?.id === "custom" && (
            <SettingContainer
              title={t("settings.postProcessing.api.baseUrl.title")}
              description={t("settings.postProcessing.api.baseUrl.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <BaseUrlField
                  value={state.baseUrl}
                  onBlur={state.handleBaseUrlChange}
                  placeholder={t(
                    "settings.postProcessing.api.baseUrl.placeholder",
                  )}
                  disabled={state.isBaseUrlUpdating}
                  className="min-w-[380px]"
                />
              </div>
            </SettingContainer>
          )}

          <SettingContainer
            title={t("settings.postProcessing.api.apiKey.title")}
            description={t("settings.postProcessing.api.apiKey.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <div id={API_KEY_FIELD_ID} className="flex items-center gap-2">
              <ApiKeyField
                value={state.apiKey}
                onBlur={state.handleApiKeyChange}
                placeholder={t(
                  "settings.postProcessing.api.apiKey.placeholder",
                )}
                disabled={state.isApiKeyUpdating}
                className="min-w-[320px]"
              />
            </div>
          </SettingContainer>
        </>
      )}

      {!state.isAppleProvider && !isLocalLlm && (
        <SettingContainer
          title={t("settings.postProcessing.api.model.title")}
          description={
            state.isCustomProvider
              ? t("settings.postProcessing.api.model.descriptionCustom")
              : t("settings.postProcessing.api.model.descriptionDefault")
          }
          descriptionMode="tooltip"
          layout="stacked"
          grouped={true}
        >
          <div className="flex items-center gap-2">
            <ModelSelect
              value={state.model}
              options={state.modelOptions}
              disabled={state.isModelUpdating}
              isLoading={state.isFetchingModels}
              placeholder={
                state.modelOptions.length > 0
                  ? t(
                      "settings.postProcessing.api.model.placeholderWithOptions",
                    )
                  : t("settings.postProcessing.api.model.placeholderNoOptions")
              }
              onSelect={state.handleModelSelect}
              onCreate={state.handleModelCreate}
              onBlur={() => {}}
              className="flex-1 min-w-[380px]"
            />
            <ResetButton
              onClick={state.handleRefreshModels}
              disabled={state.isFetchingModels}
              ariaLabel={t("settings.postProcessing.api.model.refreshModels")}
              className="flex h-10 w-10 items-center justify-center"
            >
              <RefreshCcw
                className={`h-4 w-4 ${state.isFetchingModels ? "animate-spin" : ""}`}
              />
            </ResetButton>
          </div>
        </SettingContainer>
      )}
    </>
  );
};

const PostProcessingSettingsPromptsComponent: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");

  const prompts = getSetting("post_process_prompts") || [];
  const selectedPromptId = getSetting("post_process_selected_prompt_id") || "";
  const selectedPrompt =
    prompts.find((prompt) => prompt.id === selectedPromptId) || null;

  useEffect(() => {
    if (isCreating) return;

    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  }, [
    isCreating,
    selectedPromptId,
    selectedPrompt?.name,
    selectedPrompt?.prompt,
  ]);

  const handlePromptSelect = (promptId: string | null) => {
    if (!promptId) return;
    updateSetting("post_process_selected_prompt_id", promptId);
    setIsCreating(false);
  };

  const handleCreatePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      const result = await commands.addPostProcessPrompt(
        draftName.trim(),
        draftText.trim(),
      );
      if (result.status === "ok") {
        await refreshSettings();
        updateSetting("post_process_selected_prompt_id", result.data.id);
        setIsCreating(false);
      }
    } catch (error) {
      console.error("Failed to create prompt:", error);
    }
  };

  const handleUpdatePrompt = async () => {
    if (!selectedPromptId || !draftName.trim() || !draftText.trim()) return;

    try {
      await commands.updatePostProcessPrompt(
        selectedPromptId,
        draftName.trim(),
        draftText.trim(),
      );
      await refreshSettings();
    } catch (error) {
      console.error("Failed to update prompt:", error);
    }
  };

  const handleDeletePrompt = async (promptId: string) => {
    if (!promptId) return;

    try {
      await commands.deletePostProcessPrompt(promptId);
      await refreshSettings();
      setIsCreating(false);
    } catch (error) {
      console.error("Failed to delete prompt:", error);
    }
  };

  const handleCancelCreate = () => {
    setIsCreating(false);
    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  };

  const handleStartCreate = () => {
    setIsCreating(true);
    setDraftName("");
    setDraftText("");
  };

  const hasPrompts = prompts.length > 0;
  const isDirty =
    !!selectedPrompt &&
    (draftName.trim() !== selectedPrompt.name ||
      draftText.trim() !== selectedPrompt.prompt.trim());

  return (
    <SettingContainer
      title={t("settings.postProcessing.prompts.selectedPrompt.title")}
      description={t(
        "settings.postProcessing.prompts.selectedPrompt.description",
      )}
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        <div className="flex gap-2 min-w-0">
          <Dropdown
            selectedValue={selectedPromptId || null}
            options={prompts.map((p) => ({
              value: p.id,
              label: p.name,
            }))}
            onSelect={(value) => handlePromptSelect(value)}
            placeholder={
              prompts.length === 0
                ? t("settings.postProcessing.prompts.noPrompts")
                : t("settings.postProcessing.prompts.selectPrompt")
            }
            disabled={
              isUpdating("post_process_selected_prompt_id") || isCreating
            }
            className="flex-1 min-w-0"
          />
          <Button
            onClick={handleStartCreate}
            variant="primary"
            size="md"
            disabled={isCreating}
            className="shrink-0"
          >
            {t("settings.postProcessing.prompts.createNew")}
          </Button>
        </div>

        {!isCreating && hasPrompts && selectedPrompt && (
          <div className="space-y-3">
            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptLabel")}
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptLabelPlaceholder",
                )}
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptInstructions")}
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                )}
              />
              <p className="text-xs text-mid-gray/70">
                <Trans
                  i18nKey="settings.postProcessing.prompts.promptTip"
                  components={{ code: <code /> }}
                />
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleUpdatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim() || !isDirty}
              >
                {t("settings.postProcessing.prompts.updatePrompt")}
              </Button>
              <Button
                onClick={() => handleDeletePrompt(selectedPromptId)}
                variant="secondary"
                size="md"
                disabled={!selectedPromptId || prompts.length <= 1}
              >
                {t("settings.postProcessing.prompts.deletePrompt")}
              </Button>
            </div>
          </div>
        )}

        {!isCreating && !selectedPrompt && (
          <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/20">
            <p className="text-sm text-mid-gray">
              {hasPrompts
                ? t("settings.postProcessing.prompts.selectToEdit")
                : t("settings.postProcessing.prompts.createFirst")}
            </p>
          </div>
        )}

        {isCreating && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-text">
                {t("settings.postProcessing.prompts.promptLabel")}
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptLabelPlaceholder",
                )}
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptInstructions")}
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                )}
              />
              <p className="text-xs text-mid-gray/70">
                <Trans
                  i18nKey="settings.postProcessing.prompts.promptTip"
                  components={{ code: <code /> }}
                />
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleCreatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim()}
              >
                {t("settings.postProcessing.prompts.createPrompt")}
              </Button>
              <Button
                onClick={handleCancelCreate}
                variant="secondary"
                size="md"
              >
                {t("settings.postProcessing.prompts.cancel")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </SettingContainer>
  );
};

export const PostProcessingSettingsApi = React.memo(
  PostProcessingSettingsApiComponent,
);
PostProcessingSettingsApi.displayName = "PostProcessingSettingsApi";

export const PostProcessingSettingsPrompts = React.memo(
  PostProcessingSettingsPromptsComponent,
);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

const CleanupBanner: React.FC = () => {
  const { t } = useTranslation();
  const status = useSettingsStore((s) => s.lastCleanupStatus);
  const setStatus = useSettingsStore((s) => s.setLastCleanupStatus);

  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  if (!status || !status.needs_action) return null;

  const handleAction = () => {
    // scroll to the relevant section
    let idToFocus = REASON_TO_SECTION_MAP[status.reason] || API_SECTION_ID;

    let el = document.getElementById(idToFocus);
    if (!el) {
      // Fallback if target element isn't in DOM
      idToFocus = API_SECTION_ID;
      el = document.getElementById(idToFocus);
    }

    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      if (idToFocus === API_KEY_FIELD_ID) {
        const input = el.querySelector("input");
        if (input) {
          timerRef.current = setTimeout(
            () => input.focus(),
            FOCUS_AFTER_SCROLL_MS,
          );
        }
      }
    }
  };

  const actionText = t(`cleanupStatus.action.${status.reason}`, {
    defaultValue: t("cleanupStatus.action.default"),
  });

  return (
    <div className="bg-amber-500/10 border border-amber-500/20 rounded-xl p-4 flex items-center justify-between gap-4">
      <div className="flex items-center gap-3">
        <svg
          className="w-5 h-5 shrink-0 text-amber-500"
          viewBox="0 0 24 24"
          fill="none"
        >
          <path
            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
        <span className="text-[14px] text-text">
          {t(`cleanupStatus.sentence.${status.reason}`)}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <Button variant="secondary" size="sm" onClick={() => setStatus(null)}>
          {t("cleanupStatus.action.dismiss", "Dismiss")}
        </Button>
        <Button variant="primary" size="sm" onClick={handleAction}>
          {actionText}
        </Button>
      </div>
    </div>
  );
};

export const PostProcessingSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const iterativeCorrectionEnabled =
    getSetting("iterative_correction_enabled") || false;

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <CleanupBanner />

      <SettingsGroup title={t("settings.postProcessing.hotkey.title")}>
        <ShortcutInput
          shortcutId="transcribe_with_post_process"
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.correction.title")}>
        <IterativeCorrectionToggle descriptionMode="tooltip" grouped={true} />
        {iterativeCorrectionEnabled && (
          <ShortcutInput
            shortcutId="correction"
            descriptionMode="tooltip"
            grouped={true}
          />
        )}
      </SettingsGroup>

      <div id={API_SECTION_ID}>
        <SettingsGroup title={t("settings.postProcessing.api.title")}>
          <PostProcessingSettingsApi />
        </SettingsGroup>
      </div>

      <div id={PROMPTS_SECTION_ID}>
        <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
          <PostProcessingSettingsPrompts />
        </SettingsGroup>
      </div>

      <SettingsGroup title={t("settings.postProcessing.appProfiles.title")}>
        <AppProfiles descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
