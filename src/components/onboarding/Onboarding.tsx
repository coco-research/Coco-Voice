import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands, type ModelInfo } from "@/bindings";
import { TryItStepCard } from "./AccessibilityStaleNotice";
import {
  bytesToWholeMb,
  computeRollingEtaSeconds,
  ETA_WINDOW_MS,
  FirstRunModelDownloadView,
  pickDefaultFirstRunModel,
  type DownloadByteSample,
  type FirstRunDownloadPhase,
} from "./FirstRunModelDownloadView";
import { useModelStore } from "../../stores/modelStore";
import { useSettingsStore } from "../../stores/settingsStore";

interface OnboardingProps {
  onModelSelected: () => void;
  onDownloadComplete?: () => void;
  onSkip?: () => void;
}

export function resolveOnboardingShortcut(
  binding: string | null | undefined,
  fallback: string,
): string {
  const trimmed = binding?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : fallback;
}

export async function cleanupPartialOnDownloadFailure(
  modelId: string,
  deleteModelFn: (id: string) => Promise<unknown> = (id) =>
    commands.deleteModel(id),
): Promise<void> {
  try {
    await deleteModelFn(modelId);
  } catch {
    // Ignore "No model files found" when no partial existed yet.
  }
}

const Onboarding: React.FC<OnboardingProps> = ({
  onModelSelected,
  onDownloadComplete,
  onSkip,
}) => {
  const { t } = useTranslation();
  const {
    models,
    downloadModel,
    cancelDownload,
    selectModel,
    downloadingModels,
    verifyingModels,
    extractingModels,
    downloadProgress,
    downloadFailures,
    setSuppressFailureToast,
  } = useModelStore();
  const transcribeBinding = useSettingsStore(
    (state) => state.settings?.bindings?.transcribe?.current_binding,
  );
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const [showTryIt, setShowTryIt] = useState(false);
  const [secondsLeft, setSecondsLeft] = useState<number | null>(null);
  const [cleanedFailureKey, setCleanedFailureKey] = useState<string | null>(
    null,
  );
  const hasStartedSelection = useRef(false);
  const autoStartedRef = useRef(false);
  const lastDownloadedMbRef = useRef<number>(0);
  const downloadStartedAtRef = useRef<number>(Date.now());
  const samplesRef = useRef<DownloadByteSample[]>([]);
  const cleanedFailureRef = useRef<string | null>(null);

  useEffect(() => {
    setSuppressFailureToast(true);
    return () => setSuppressFailureToast(false);
  }, [setSuppressFailureToast]);

  const defaultModel: ModelInfo | null = useMemo(
    () => pickDefaultFirstRunModel(models),
    [models],
  );
  const activeModelId = selectedModelId ?? defaultModel?.id ?? null;
  const activeModel = useMemo(
    () =>
      (activeModelId
        ? models.find((m) => m.id === activeModelId)
        : undefined) ?? defaultModel,
    [activeModelId, defaultModel, models],
  );

  const failureCause = activeModel
    ? (downloadFailures[activeModel.id] ?? null)
    : null;
  const progress = activeModel ? downloadProgress[activeModel.id] : undefined;

  useEffect(() => {
    if (!progress || progress.downloaded <= 0) return;
    const now = Date.now();
    lastDownloadedMbRef.current = bytesToWholeMb(progress.downloaded);
    const nextSamples = [
      ...samplesRef.current.filter((s) => now - s.atMs <= ETA_WINDOW_MS),
      { atMs: now, bytes: progress.downloaded },
    ];
    samplesRef.current = nextSamples;
    setSecondsLeft(
      computeRollingEtaSeconds(
        nextSamples,
        progress.total,
        now,
        downloadStartedAtRef.current,
      ),
    );
  }, [progress]);

  // AC-A7 / Plan A v1.1: remove any .partial file left by resumable downloader on
  // first-run failure BEFORE showing the failure state so "Nothing was saved." is true.
  const activeFailureKey =
    activeModel && failureCause ? `${activeModel.id}:${failureCause}` : null;

  useEffect(() => {
    if (!activeModel || !failureCause || !activeFailureKey) {
      cleanedFailureRef.current = null;
      setCleanedFailureKey(null);
      return;
    }
    if (cleanedFailureRef.current === activeFailureKey) return;
    cleanedFailureRef.current = activeFailureKey;
    let active = true;
    void cleanupPartialOnDownloadFailure(activeModel.id).finally(() => {
      if (active) setCleanedFailureKey(activeFailureKey);
    });
    return () => {
      active = false;
    };
  }, [activeFailureKey, activeModel, failureCause]);

  // Automatically start the single default speech model download on First Run,
  // or advance straight to "Try it" if the model is already on disk.
  useEffect(() => {
    if (!activeModel || showTryIt || autoStartedRef.current) return;
    if (failureCause || cancelled) return;

    if (activeModel.is_downloaded) {
      autoStartedRef.current = true;
      setSelectedModelId(activeModel.id);
      return;
    }

    autoStartedRef.current = true;
    downloadStartedAtRef.current = Date.now();
    samplesRef.current = [];
    setSecondsLeft(null);
    setSelectedModelId(activeModel.id);
    void downloadModel(activeModel.id).then((ok) => {
      if (!ok) {
        setSelectedModelId(null);
      }
    });
  }, [activeModel, cancelled, downloadModel, failureCause, showTryIt]);

  // Watch for the selected model to finish downloading + verifying + extracting
  useEffect(() => {
    if (!selectedModelId || cancelled) {
      hasStartedSelection.current = false;
      return;
    }

    const model = models.find((m) => m.id === selectedModelId);
    const stillDownloading = selectedModelId in downloadingModels;
    const stillVerifying = selectedModelId in verifyingModels;
    const stillExtracting = selectedModelId in extractingModels;

    if (
      model?.is_downloaded &&
      !stillDownloading &&
      !stillVerifying &&
      !stillExtracting &&
      !hasStartedSelection.current
    ) {
      hasStartedSelection.current = true;

      void selectModel(selectedModelId).then((success) => {
        if (success) {
          if (onDownloadComplete) {
            onDownloadComplete();
          } else {
            setShowTryIt(true);
          }
        } else {
          toast.error(t("onboarding.errors.selectModel"));
          hasStartedSelection.current = false;
          setSelectedModelId(null);
        }
      });
    }
  }, [
    cancelled,
    selectedModelId,
    models,
    downloadingModels,
    verifyingModels,
    extractingModels,
    selectModel,
    onDownloadComplete,
    t,
  ]);

  const handleCancel = async () => {
    if (!activeModel) return;
    setCancelled(true);
    hasStartedSelection.current = false;
    setSelectedModelId(null);
    setSecondsLeft(null);
    await cancelDownload(activeModel.id);
  };

  const handleTryAgain = async () => {
    if (!activeModel) return;
    setCancelled(false);
    hasStartedSelection.current = false;
    lastDownloadedMbRef.current = 0;
    downloadStartedAtRef.current = Date.now();
    samplesRef.current = [];
    setSecondsLeft(null);
    setSelectedModelId(activeModel.id);
    const ok = await downloadModel(activeModel.id);
    if (!ok) {
      setSelectedModelId(null);
    }
  };

  const shortcut = resolveOnboardingShortcut(
    transcribeBinding,
    t("sidebar.shortcutNotSet", "Shortcut not set"),
  );

  if (showTryIt) {
    return (
      <div className="min-h-screen w-full bg-background">
        <TryItStepCard shortcut={shortcut} onDone={onModelSelected} />
      </div>
    );
  }

  const failureReady =
    Boolean(failureCause) && cleanedFailureKey === activeFailureKey;

  let phase: FirstRunDownloadPhase = "downloading";
  if (failureReady) {
    phase = "failed";
  } else if (cancelled) {
    phase = "cancelled";
  } else if (activeModel && activeModel.id in extractingModels) {
    phase = "extracting";
  } else if (activeModel && activeModel.id in verifyingModels) {
    phase = "verifying";
  } else if (activeModel && activeModel.id in downloadingModels) {
    phase = "downloading";
  } else if (!activeModel) {
    phase = "idle";
  }

  const totalMb =
    progress && progress.total > 0
      ? bytesToWholeMb(progress.total)
      : Math.round(activeModel?.size_mb ?? 0);
  const downloadedMb =
    progress && progress.downloaded > 0
      ? bytesToWholeMb(progress.downloaded)
      : lastDownloadedMbRef.current;
  const percentage = progress?.percentage ?? 0;

  return (
    <div className="min-h-screen w-full bg-background">
      <FirstRunModelDownloadView
        model={activeModel}
        downloadedMb={downloadedMb}
        totalMb={totalMb}
        percentage={percentage}
        secondsLeft={secondsLeft}
        phase={phase}
        failureCause={failureReady ? failureCause : null}
        failedAtMb={lastDownloadedMbRef.current}
        onCancel={() => {
          void handleCancel();
        }}
        onTryAgain={() => {
          void handleTryAgain();
        }}
        onSkipForNow={onSkip ?? onModelSelected}
      />
    </div>
  );
};

export default Onboarding;
