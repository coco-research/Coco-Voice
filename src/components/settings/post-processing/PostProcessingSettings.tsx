import React, { useCallback, useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { RefreshCcw } from "lucide-react";
import { commands, type ModelInfo } from "@/bindings";
import { CleanupMark } from "@/components/shared/CleanupMark";
import {
  CLEANUP_NOTE,
  type CleanupNote,
  type CleanupStrings,
} from "@/lib/utils/cleanupStatus";
import { useModelStore } from "@/stores/modelStore";
import { useSettingsStore } from "@/stores/settingsStore";

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
import { ProgressBar } from "../../shared";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { ShortcutInput } from "../ShortcutInput";
import { IterativeCorrectionToggle } from "../IterativeCorrectionToggle";
import { useSettings } from "../../../hooks/useSettings";

/** Same id as `LOCAL_LLM_PROVIDER_ID` in src-tauri/src/settings.rs. */
const LOCAL_LLM_PROVIDER_ID = "local_llm";

const API_SECTION_ID = "post-process-api-section";
const API_KEY_FIELD_ID = "post-process-api-key-field";
const PROMPTS_SECTION_ID = "post-process-prompts-section";

const FOCUS_AFTER_SCROLL_MS = 300; // Delay to allow smooth scroll before focusing

// What the banner button does, and where it takes the user. Typed from the
// English file, so an action without its label does not compile.
type BannerAction = keyof CleanupStrings["action"];

const ACTION_TARGET: Record<BannerAction, string> = {
  download: API_SECTION_ID,
  addKey: API_KEY_FIELD_ID,
  chooseModel: API_SECTION_ID,
  choosePrompt: PROMPTS_SECTION_ID,
  chooseProvider: API_SECTION_ID,
  useLocal: API_SECTION_ID,
};

// The button for a note, or null for the ones that concern one take only (no
// banner). `local` is the on-device model: its id, and whether it is downloaded
// (null until known). A switch, so a new note cannot be added without a choice.
const bannerAction = (
  note: CleanupNote,
  local: { id: string; ready: boolean | null },
): BannerAction | null => {
  switch (note) {
    case "noModel":
      return "download";
    case "noApiKey":
      return "addKey";
    case "noModelChosen":
      return "chooseModel";
    case "noPrompt":
      return "choosePrompt";
    case "noProvider":
      return "chooseProvider";
    case "appleOff":
      // Apple Intelligence cannot be fixed from here: the way on is the
      // on-device model, never a hosted provider.
      if (!local.id) return "chooseProvider";
      return local.ready === null
        ? null
        : local.ready
          ? "useLocal"
          : "download";
    case "tooLong":
    case "stoppedEarly":
    case "modelFailed":
      return null;
  }
};

const isDownloaded = async (modelId: string) => {
  try {
    const info = await commands.getModelInfo(modelId);
    return info.status === "ok" && Boolean(info.data?.is_downloaded);
  } catch {
    return false;
  }
};

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
  // Bounded for the status string. The bar clamp lives in ProgressBar.
  const percent = Math.max(
    0,
    Math.min(100, Math.round(progress?.percentage ?? 0)),
  );

  let status = t("settings.postProcessing.api.localLlm.checking");
  if (busy) {
    status = t("settings.postProcessing.api.localLlm.downloading", {
      percent,
    });
  } else if (info === null) {
    status = t("settings.postProcessing.api.localLlm.unavailable");
  } else if (ready) {
    status = t("settings.postProcessing.api.localLlm.ready");
  } else if (info) {
    status = t("settings.postProcessing.api.localLlm.notDownloaded");
  }

  // The percentage ticks on every progress event. Keep it out of the live
  // region so a screen reader announces the phrase once, not every tick.
  // Visible text stays the full translated status, including the percent.
  const percentTail = `${percent}%`;
  const hasPercent = busy && status.endsWith(percentTail);
  const livePhrase = hasPercent
    ? status.slice(0, -percentTail.length).trimEnd()
    : status;
  const percentSuffix = hasPercent ? status.slice(livePhrase.length) : "";

  let modelAction: React.ReactNode = null;
  if (busy) {
    modelAction = (
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
    );
  } else if (info && !ready) {
    modelAction = (
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
    );
  }

  let progressBar: React.ReactNode = null;
  if (busy) {
    progressBar = (
      <ProgressBar
        progress={[{ id: modelId, percentage: percent }]}
        size="large"
        ariaLabel={status}
      />
    );
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
        {progressBar}
        <span className="text-sm text-text/70 whitespace-nowrap">
          <span aria-live="polite">{livePhrase}</span>
          {percentSuffix !== "" && (
            <span aria-hidden="true">{percentSuffix}</span>
          )}
        </span>
        {modelAction}
      </div>
    </SettingContainer>
  );
};

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();
  const isLocalLlm = state.selectedProvider?.id === LOCAL_LLM_PROVIDER_ID;
  // settings.rs ensure_post_process_defaults copies LOCAL_LLM_DEFAULT_MODEL_ID
  // into post_process_models before this value reaches the client.
  const localLlmModelId = state.model.trim();

  let providerFields: React.ReactNode = null;
  if (state.isAppleProvider && state.appleIntelligenceUnavailable) {
    providerFields = (
      <Alert variant="error" contained>
        {t("settings.postProcessing.api.appleIntelligence.unavailable")}
      </Alert>
    );
  } else if (isLocalLlm && localLlmModelId) {
    providerFields = <LocalLlmModelRow modelId={localLlmModelId} />;
  } else if (!state.isAppleProvider && !isLocalLlm) {
    providerFields = (
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
              placeholder={t("settings.postProcessing.api.apiKey.placeholder")}
              disabled={state.isApiKeyUpdating}
              className="min-w-[320px]"
            />
          </div>
        </SettingContainer>
      </>
    );
  }

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

      {providerFields}

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

// The last "cleanup did not run" status that the user can fix: a mark, one line
// and one button that does the fix. No close button: only fixing the cause clears
// it. App.tsx stores the status and clears it when a take cleans.
const CleanupBanner: React.FC = () => {
  const { t } = useTranslation();
  const { settings, setPostProcessProvider } = useSettings();
  const status = useSettingsStore((s) => s.lastCleanupStatus);
  const downloadModel = useModelStore((s) => s.downloadModel);
  const [localReady, setLocalReady] = useState<boolean | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const note = status?.needs_action ? CLEANUP_NOTE[status.reason] : null;
  const localId =
    settings?.post_process_models?.[LOCAL_LLM_PROVIDER_ID]?.trim() ?? "";

  // Asked again for each new status: the model may have been downloaded since.
  useEffect(() => {
    if (note !== "appleOff" || !localId) return;
    let active = true;
    setLocalReady(null);
    void isDownloaded(localId).then((ready) => {
      if (active) setLocalReady(ready);
    });
    return () => {
      active = false;
    };
  }, [status, note, localId]);

  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  if (!note) return null;
  const action = bannerAction(note, { id: localId, ready: localReady });

  const handleAction = async () => {
    if (!action) return;
    if (action === "download" || action === "useLocal") {
      if (settings?.post_process_provider_id !== LOCAL_LLM_PROVIDER_ID) {
        await setPostProcessProvider(LOCAL_LLM_PROVIDER_ID);
      }
      // Checked again here: the model may have been downloaded since the label.
      if (action === "download" && localId && !(await isDownloaded(localId))) {
        void downloadModel(localId);
      }
    }

    let idToFocus = ACTION_TARGET[action];
    let el = document.getElementById(idToFocus);
    if (!el) {
      // The target is not mounted (the API key field only exists for a hosted
      // provider): the API section is always there.
      idToFocus = API_SECTION_ID;
      el = document.getElementById(idToFocus);
    }

    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      if (idToFocus === API_KEY_FIELD_ID) {
        const input = el.querySelector("input");
        if (input) {
          if (timerRef.current) {
            clearTimeout(timerRef.current);
          }
          timerRef.current = setTimeout(
            () => input.focus(),
            FOCUS_AFTER_SCROLL_MS,
          );
        }
      }
    }
  };

  return (
    <div
      role="status"
      className="flex items-center justify-between gap-4 px-4 py-2 text-sm"
    >
      <div className="flex min-w-0 items-center gap-2">
        <CleanupMark needsAction />
        <span>{t(`cleanupStatus.sentence.${note}`)}</span>
      </div>
      {action && (
        <Button
          variant="primary"
          size="sm"
          className="shrink-0"
          onClick={handleAction}
        >
          {t(`cleanupStatus.action.${action}`)}
        </Button>
      )}
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
    </div>
  );
};
