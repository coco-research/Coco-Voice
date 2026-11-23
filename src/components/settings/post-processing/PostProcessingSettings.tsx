import React, { useCallback, useEffect, useRef, useState } from "react";
import type { TFunction } from "i18next";
import { Trans, useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { RefreshCcw } from "lucide-react";
import { commands, type ModelInfo, type SavedBlock } from "@/bindings";
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
import { Corrections } from "../Corrections";
import { IterativeCorrectionToggle } from "../IterativeCorrectionToggle";
import { PostProcessingToggle } from "../PostProcessingToggle";
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

      <SettingsGroup title={t("sidebar.postProcessing")}>
        <PostProcessingToggle descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

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

const WordsInlineSwitch: React.FC<{
  checked: boolean;
  label: string;
  onChange: (next: boolean) => void;
}> = ({ checked, label, onChange }) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    onClick={() => onChange(!checked)}
    className={`relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full border transition-colors cursor-pointer ${
      checked
        ? "border-logo-primary bg-logo-primary"
        : "border-mid-gray/50 bg-transparent"
    }`}
  >
    <span
      className={`inline-block h-4 w-4 rounded-full transition-transform ${
        checked ? "translate-x-[18px] bg-white" : "translate-x-[2px] bg-text"
      }`}
    />
  </button>
);

export interface FixOfferItem {
  id: string;
  old: string;
  new: string;
  app: string;
}

export function formatWhereCleanupRuns(
  settings: {
    post_process_provider_id?: string;
    post_process_providers?: Array<{ id: string; label: string }>;
    post_process_models?: Partial<Record<string, string>>;
  } | null,
  t: TFunction,
): string {
  const providerId =
    settings?.post_process_provider_id || LOCAL_LLM_PROVIDER_ID;
  if (providerId === "apple_intelligence") {
    return t(
      "v022.words.onThisMacAppleIntelligence",
      "On this Mac (Apple Intelligence)",
    );
  }
  if (providerId === LOCAL_LLM_PROVIDER_ID) {
    const rawModel = settings?.post_process_models?.[LOCAL_LLM_PROVIDER_ID] || "";
    const shortName = rawModel.toLowerCase().includes("qwen3-4b")
      ? "Qwen3-4B"
      : rawModel
          ? rawModel.split("/").pop()?.replace(/\.gguf$/i, "") || "Qwen3-4B"
          : "Qwen3-4B";
    return t("v022.words.onThisMacModelName", {
      model: shortName,
      defaultValue: `On this Mac (${shortName})`,
    });
  }
  const activeProvider = settings?.post_process_providers?.find(
    (p) => p.id === providerId,
  );
  const label = activeProvider?.label || providerId;
  const model = settings?.post_process_models?.[providerId] || "";
  return model ? `${label} (${model})` : label;
}

export function normalizeCanonicalSavedBlock(
  cue: string,
  content: string,
  existingId?: string,
): { id: string; cue: string; content: string } {
  return {
    id: existingId || `block-${Date.now()}`,
    cue: cue.trim(),
    content: content.trim(),
  };
}

export const WordsAndCleanupV022: React.FC<{
  initialFixOffer?: FixOfferItem | null;
}> = ({ initialFixOffer = null }) => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();
  const [wordDraft, setWordDraft] = useState("");
  const [offerFixEnabled, setOfferFixEnabled] = useState(() => {
    const ext = settings as Record<string, unknown> | null;
    return typeof ext?.offer_remember_fix_enabled === "boolean"
      ? ext.offer_remember_fix_enabled
      : true;
  });
  const [repoWordsEnabled, setRepoWordsEnabled] = useState(() => {
    const ext = settings as Record<string, unknown> | null;
    return typeof ext?.repo_words_enabled === "boolean"
      ? ext.repo_words_enabled
      : true;
  });
  const [removeFillerWords, setRemoveFillerWords] = useState(() => {
    const ext = settings as Record<string, unknown> | null;
    return typeof ext?.filler_word_removal_enabled === "boolean"
      ? ext.filler_word_removal_enabled
      : true;
  });
  const [saveError, setSaveError] = useState<string | null>(null);
  const [openCleanupPanel, setOpenCleanupPanel] = useState<
    null | "provider" | "style"
  >(null);
  const [fixOffer, setFixOffer] = useState<FixOfferItem | null>(
    initialFixOffer,
  );

  const [blocks, setBlocks] = useState<SavedBlock[]>(
    () => settings?.saved_blocks ?? [],
  );
  const [showBlockForm, setShowBlockForm] = useState(false);
  const [editingBlockIdx, setEditingBlockIdx] = useState<number | null>(null);
  const [cueDraft, setCueDraft] = useState("");
  const [contentDraft, setContentDraft] = useState("");

  useEffect(() => {
    if (settings?.saved_blocks) {
      setBlocks(settings.saved_blocks);
    }
    const ext = settings as Record<string, unknown> | null;
    if (typeof ext?.offer_remember_fix_enabled === "boolean") {
      setOfferFixEnabled(ext.offer_remember_fix_enabled);
    }
    if (typeof ext?.repo_words_enabled === "boolean") {
      setRepoWordsEnabled(ext.repo_words_enabled);
    }
    if (typeof ext?.filler_word_removal_enabled === "boolean") {
      setRemoveFillerWords(ext.filler_word_removal_enabled);
    }
  }, [settings]);

  useEffect(() => {
    let active = true;
    const unlistenPromise = listen<FixOfferItem | null>("fix-offer", (ev) => {
      if (!active) return;
      setFixOffer(ev.payload ?? null);
    });
    return () => {
      active = false;
      void unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const customWords = settings?.custom_words ?? [];
  const cleanEveryTake = Boolean(settings?.post_process_enabled ?? true);
  const whereCleanupRuns = formatWhereCleanupRuns(settings, t);

  const selectedPromptId = settings?.post_process_selected_prompt_id;
  const activePrompt = settings?.post_process_prompts?.find(
    (p) => p.id === selectedPromptId,
  );
  const cleanupStyleLabel =
    activePrompt?.name || t("v022.words.defaultPrompt", "Default prompt");

  const handleToggleOfferFix = async (next: boolean) => {
    const prev = offerFixEnabled;
    setOfferFixEnabled(next);
    setSaveError(null);
    try {
      await invoke("change_offer_remember_fix_setting", { enabled: next });
    } catch {
      setOfferFixEnabled(prev);
      setSaveError(t("v022.words.saveFailed", "Couldn't save. Try again."));
    }
  };

  const handleToggleRepoWords = async (next: boolean) => {
    const prev = repoWordsEnabled;
    setRepoWordsEnabled(next);
    setSaveError(null);
    try {
      await invoke("change_repo_words_setting", { enabled: next });
    } catch {
      setRepoWordsEnabled(prev);
      setSaveError(t("v022.words.saveFailed", "Couldn't save. Try again."));
    }
  };

  const handleToggleFillerWords = async (next: boolean) => {
    const prev = removeFillerWords;
    setRemoveFillerWords(next);
    setSaveError(null);
    try {
      await invoke("change_filler_word_removal_setting", { enabled: next });
    } catch {
      setRemoveFillerWords(prev);
      setSaveError(t("v022.words.saveFailed", "Couldn't save. Try again."));
    }
  };

  const handleAddWord = () => {
    const trimmed = wordDraft.trim();
    if (!trimmed) return;
    if (!customWords.some((w) => w.toLowerCase() === trimmed.toLowerCase())) {
      void updateSetting("custom_words", [...customWords, trimmed]);
    }
    setWordDraft("");
  };

  const handleRemoveWord = (word: string) => {
    void updateSetting(
      "custom_words",
      customWords.filter((w) => w !== word),
    );
  };

  const handleRememberFix = async (offer: FixOfferItem) => {
    await commands.rememberSpellingFix(offer.old, offer.new);
    if (
      !customWords.some(
        (w) => w.toLowerCase() === offer.new.trim().toLowerCase(),
      )
    ) {
      void updateSetting("custom_words", [...customWords, offer.new.trim()]);
    }
    setFixOffer(null);
  };

  const handleDismissFix = (offer: FixOfferItem) => {
    setFixOffer(null);
    void invoke("dismiss_fix_offer", { id: offer.id }).catch(() => {});
  };

  const handleSaveBlock = () => {
    const cue = cueDraft.trim();
    const body = contentDraft.trim();
    if (!cue || !body) return;
    const existing =
      editingBlockIdx !== null
        ? (blocks[editingBlockIdx] as { id?: string } | undefined)
        : undefined;
    const entry = normalizeCanonicalSavedBlock(cue, body, existing?.id);
    const next =
      editingBlockIdx !== null
        ? blocks.map((b, i) => (i === editingBlockIdx ? entry : b))
        : [...blocks, entry];
    setBlocks(next as SavedBlock[]);
    setCueDraft("");
    setContentDraft("");
    setEditingBlockIdx(null);
    setShowBlockForm(false);
    void commands.updateSavedBlocks(next as SavedBlock[]);
  };

  const handleRemoveEditingBlock = () => {
    if (editingBlockIdx === null) return;
    const next = blocks.filter((_, i) => i !== editingBlockIdx);
    setBlocks(next);
    setCueDraft("");
    setContentDraft("");
    setEditingBlockIdx(null);
    setShowBlockForm(false);
    void commands.updateSavedBlocks(next);
  };

  return (
    <div data-route-domain="voice" className="max-w-4xl w-full mx-auto">
      {/* Scene 06 (p-words): Two-column Words and clean-up */}
      <div
        data-testid="words-and-cleanup-v022"
        className="min-h-[565px] space-y-3"
      >
        <CleanupBanner />

        {/* Scene 47 (CP-5 / AC-P28): Real event-driven Remember a fix card at top */}
        {fixOffer ? (
          <section
            data-testid="remember-fix-card"
            className="rounded-xl border border-logo-primary/35 bg-surface px-4 py-3.5 space-y-2.5"
          >
            <div>
              <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
                {t("sidebar.voiceSection")}
              </p>
              <h2 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
                {t("v022.words.afterYouFixHeading", "After you fix a word")}
              </h2>
            </div>
            <p className="text-[13px] text-mid-gray">
              {t("v022.words.afterYouFixDynamic", {
                old: fixOffer.old,
                new: fixOffer.new,
                app: fixOffer.app,
                defaultValue: `You changed "${fixOffer.old}" to "${fixOffer.new}" in ${fixOffer.app}.`,
              })}
            </p>
            <div className="inline-flex flex-wrap items-center gap-2.5 rounded-xl border border-mid-gray/30 bg-background px-4 py-2 shadow-sm">
              <span className="text-[13.5px] font-medium text-text">
                {t("v022.words.alwaysWriteDynamic", {
                  word: fixOffer.new,
                  defaultValue: `Always write "${fixOffer.new}"?`,
                })}
              </span>
              <button
                type="button"
                onClick={() => void handleRememberFix(fixOffer)}
                className="rounded-lg bg-logo-primary px-3 py-1 text-xs font-semibold text-white cursor-pointer hover:opacity-90"
              >
                {t("v022.words.rememberButton", "Remember")}
              </button>
              <button
                type="button"
                onClick={() => handleDismissFix(fixOffer)}
                className="rounded-lg border border-mid-gray/40 px-3 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.words.notNowButton", "Not now")}
              </button>
            </div>
            <p className="text-xs text-mid-gray">
              {t(
                "v022.words.rememberedWordsNote",
                "Remembered words appear in Words and clean-up, where you can remove them.",
              )}
            </p>
          </section>
        ) : null}

        <div>
          <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
            {t("sidebar.voiceSection")}
          </p>
          <h1 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
            {t("sidebar.wordsAndCleanup")}
          </h1>
        </div>

        {saveError ? (
          <p role="alert" className="text-xs text-red-400">
            {saveError}
          </p>
        ) : null}

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5 items-start">
          {/* Left column: Your words */}
          <section
            aria-label={t("v022.words.yourWordsHeading", "Your words")}
            className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3.5 space-y-3"
          >
            <div>
              <h2 className="text-[14px] font-bold text-text">
                {t("v022.words.yourWordsHeading", "Your words")}
              </h2>
              <p className="mt-1 text-xs leading-[17px] text-mid-gray">
                {t(
                  "v022.words.yourWordsDesc",
                  "Names and terms spelled your way, in dictation. Meetings have their own list.",
                )}
              </p>
            </div>

            {customWords.length > 0 ? (
              <div className="flex flex-wrap gap-1.5">
                {customWords.map((word) => (
                  <span
                    key={word}
                    className="inline-flex items-center gap-1.5 rounded-full border border-mid-gray/35 bg-background px-2.5 py-0.5 text-[11.5px] font-semibold text-text"
                  >
                    <span>{word}</span>
                    <button
                      type="button"
                      aria-label={t("v022.words.removeWordAria", {
                        word,
                        defaultValue: `Remove ${word}`,
                      })}
                      onClick={() => handleRemoveWord(word)}
                      className="text-mid-gray hover:text-text cursor-pointer"
                    >
                      {"×"}
                    </button>
                  </span>
                ))}
              </div>
            ) : (
              <p className="text-xs text-mid-gray">
                {t("v022.words.noWordsYet", "No words yet.")}
              </p>
            )}

            <div className="flex items-center gap-2">
              <input
                type="text"
                value={wordDraft}
                onChange={(e) => setWordDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    handleAddWord();
                  }
                }}
                placeholder={t("v022.words.addWordPlaceholder", "Add a word")}
                aria-label={t("v022.words.addWordPlaceholder", "Add a word")}
                className="flex-1 rounded-lg border border-mid-gray/40 bg-background px-3 py-1.5 text-xs text-text placeholder:text-mid-gray focus:outline-none focus:ring-1 focus:ring-logo-primary"
              />
              <button
                type="button"
                onClick={handleAddWord}
                className="rounded-lg border border-mid-gray/45 px-3 py-1.5 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.words.addButton", "Add")}
              </button>
            </div>

            <div className="divide-y divide-mid-gray/15 border-t border-mid-gray/15 text-[13px]">
              <div className="flex items-center justify-between gap-3 py-2">
                <div className="min-w-0">
                  <div className="font-medium text-text">
                    {t(
                      "v022.words.offerToRememberFix",
                      "Offer to remember a fix",
                    )}
                  </div>
                  <div className="text-xs text-mid-gray">
                    {t(
                      "v022.words.offerToRememberFixDesc",
                      "After you correct a word, ask to add it",
                    )}
                  </div>
                </div>
                <WordsInlineSwitch
                  checked={offerFixEnabled}
                  label={t(
                    "v022.words.offerToRememberFix",
                    "Offer to remember a fix",
                  )}
                  onChange={(next) => void handleToggleOfferFix(next)}
                />
              </div>

              <div className="flex items-center justify-between gap-3 py-2">
                <div className="min-w-0">
                  <div className="font-medium text-text">
                    {t(
                      "v022.words.wordsFromOpenRepo",
                      "Words from the open repo",
                    )}
                  </div>
                  <div className="text-xs text-mid-gray">
                    {t(
                      "v022.words.wordsFromOpenRepoDesc",
                      "File and function names from the project you are in",
                    )}
                  </div>
                </div>
                <WordsInlineSwitch
                  checked={repoWordsEnabled}
                  label={t(
                    "v022.words.wordsFromOpenRepo",
                    "Words from the open repo",
                  )}
                  onChange={(next) => void handleToggleRepoWords(next)}
                />
              </div>
            </div>
          </section>

          {/* Right column: Clean up */}
          <section
            aria-label={t("v022.words.cleanUpHeading", "Clean up")}
            className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3.5 space-y-2"
          >
            <h2 className="text-[14px] font-bold text-text">
              {t("v022.words.cleanUpHeading", "Clean up")}
            </h2>

            <div className="divide-y divide-mid-gray/15 text-[13px]">
              <div className="flex items-center justify-between gap-3 py-2">
                <div className="min-w-0">
                  <div className="font-medium text-text">
                    {t("v022.words.cleanEveryTake", "Clean every take")}
                  </div>
                  <div className="text-xs text-mid-gray">
                    {t(
                      "v022.words.cleanEveryTakeDesc",
                      'Fixes punctuation, drops "um" and "scratch that"',
                    )}
                  </div>
                </div>
                <WordsInlineSwitch
                  checked={cleanEveryTake}
                  label={t("v022.words.cleanEveryTake", "Clean every take")}
                  onChange={(next) =>
                    void updateSetting("post_process_enabled", next)
                  }
                />
              </div>

              <div className="flex items-center justify-between gap-3 py-2">
                <div className="min-w-0">
                  <div className="font-medium text-text">
                    {t("v022.words.removeFillerWords", "Remove filler words")}
                  </div>
                  <div className="text-xs text-mid-gray">
                    {t(
                      "v022.words.removeFillerWordsDesc",
                      "Follows the language you speak",
                    )}
                  </div>
                </div>
                <WordsInlineSwitch
                  checked={removeFillerWords}
                  label={t(
                    "v022.words.removeFillerWords",
                    "Remove filler words",
                  )}
                  onChange={(next) => void handleToggleFillerWords(next)}
                />
              </div>

              <div className="py-2 space-y-2">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="font-medium text-text">
                      {t("v022.words.whereCleanupRuns", "Where clean-up runs")}
                    </div>
                    <div
                      data-testid="where-cleanup-runs-label"
                      className="text-xs text-mid-gray"
                    >
                      {whereCleanupRuns}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() =>
                      setOpenCleanupPanel((p) =>
                        p === "provider" ? null : "provider",
                      )
                    }
                    className="rounded-lg border border-mid-gray/45 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("v022.settings.changeButton", "Change")}
                  </button>
                </div>
                {openCleanupPanel === "provider" ? (
                  <div id={API_SECTION_ID} className="pt-1">
                    <PostProcessingSettingsApi />
                  </div>
                ) : null}
              </div>

              <div className="py-2 space-y-2">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="font-medium text-text">
                      {t("v022.words.cleanupStyle", "Clean-up style")}
                    </div>
                    <div
                      data-testid="cleanup-style-label"
                      className="text-xs text-mid-gray"
                    >
                      {cleanupStyleLabel}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() =>
                      setOpenCleanupPanel((p) =>
                        p === "style" ? null : "style",
                      )
                    }
                    className="rounded-lg border border-mid-gray/45 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("v022.words.editButton", "Edit")}
                  </button>
                </div>
                {openCleanupPanel === "style" ? (
                  <div id={PROMPTS_SECTION_ID} className="pt-1">
                    <PostProcessingSettingsPrompts />
                  </div>
                ) : null}
              </div>
            </div>
          </section>
        </div>

        {/* Scene 48 (CP-6 / AC-P30 / D-P6): Saved blocks directly below the columns */}
        <section data-testid="saved-blocks-card" className="space-y-3 pt-4">
          <div className="flex items-center justify-between gap-3">
            <h2 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
              {t("v022.words.savedBlocksHeading", "Saved blocks")}
            </h2>
            <button
              type="button"
              onClick={() => {
                setEditingBlockIdx(null);
                setCueDraft("");
                setContentDraft("");
                setShowBlockForm((v) => !v);
              }}
              className="rounded-lg border border-mid-gray/45 px-3 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("v022.words.addABlockButton", "Add a block")}
            </button>
          </div>

          <p className="text-[13px] text-mid-gray">
            {t(
              "v022.words.savedBlocksDesc",
              "Say the cue while dictating and the block is typed. Stored only on this Mac.",
            )}
          </p>

          {blocks.length > 0 ? (
            <div className="rounded-xl border border-mid-gray/25 bg-surface divide-y divide-mid-gray/20">
              {blocks.map((b, idx) => {
                const rawBlock = b as SavedBlock & {
                  cue?: string;
                  content?: string;
                  trigger?: string;
                  name?: string;
                  expansion?: string;
                };
                const cue =
                  rawBlock.cue || rawBlock.trigger || rawBlock.name || "";
                const body = rawBlock.content || rawBlock.expansion || "";
                return (
                  <div
                    key={`${cue}-${idx}`}
                    data-testid="saved-block-row"
                    className="flex items-center justify-between gap-3 px-4 py-2.5"
                  >
                    <div className="min-w-0">
                      <p className="text-[13.5px] font-semibold text-text">
                        &ldquo;{cue}&rdquo;
                      </p>
                      <p className="truncate text-xs text-mid-gray">{body}</p>
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        setEditingBlockIdx(idx);
                        setCueDraft(cue);
                        setContentDraft(body);
                        setShowBlockForm(true);
                      }}
                      className="shrink-0 rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                    >
                      {t("v022.words.editButton", "Edit")}
                    </button>
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="text-xs text-mid-gray">
              {t("v022.words.noBlocksYet", "No blocks yet.")}
            </p>
          )}

          {showBlockForm ? (
            <div
              data-testid="saved-block-form"
              className="rounded-xl border border-mid-gray/25 bg-surface p-3.5 space-y-2"
            >
              <div className="flex flex-wrap items-center gap-2">
                <input
                  type="text"
                  value={cueDraft}
                  onChange={(e) => setCueDraft(e.target.value)}
                  placeholder={t(
                    "v022.words.cuePlaceholder",
                    "Cue phrase (e.g. meeting sign-off)",
                  )}
                  className="rounded-lg border border-mid-gray/35 bg-background px-3 py-1.5 text-xs text-text"
                />
                <input
                  type="text"
                  value={contentDraft}
                  onChange={(e) => setContentDraft(e.target.value)}
                  placeholder={t(
                    "v022.words.expandedPlaceholder",
                    "Expanded text",
                  )}
                  className="flex-1 min-w-[180px] rounded-lg border border-mid-gray/35 bg-background px-3 py-1.5 text-xs text-text"
                />
                <button
                  type="button"
                  onClick={handleSaveBlock}
                  className="rounded-lg bg-logo-primary px-3 py-1.5 text-xs font-semibold text-white cursor-pointer hover:opacity-90"
                >
                  {t("settings.history.saveEdit", "Save")}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setShowBlockForm(false);
                    setEditingBlockIdx(null);
                  }}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1.5 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("tray.cancel")}
                </button>
                {editingBlockIdx !== null ? (
                  <button
                    type="button"
                    onClick={handleRemoveEditingBlock}
                    className="rounded-lg border border-red-500/40 px-2.5 py-1.5 text-xs font-semibold text-red-400 cursor-pointer hover:bg-red-500/10"
                  >
                    {t("v022.words.removeBlockButton", "Remove block")}
                  </button>
                ) : null}
              </div>
            </div>
          ) : null}
        </section>

        {/* I17: Corrections list kept below Saved blocks */}
        <section
          data-testid="words-corrections-section"
          className="rounded-xl border border-mid-gray/25 bg-surface p-3"
        >
          <Corrections grouped={true} />
        </section>

        {/* Preserved full clean-up & spoken-fix shortcut settings */}
        <section
          data-testid="words-preserved-post-processing"
          className="space-y-4 pt-2"
        >
          <PostProcessingSettings />
        </section>
      </div>
    </div>
  );
};
