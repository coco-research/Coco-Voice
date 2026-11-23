import React from "react";
import { useTranslation } from "react-i18next";
import type { DownloadFailureCause, ModelInfo } from "@/bindings";
import { isLegacySource } from "./ModelCard";

export const BYTES_PER_MB = 1_000_000;
export const FIRST_RUN_DEFAULT_MODEL_ID = "parakeet-tdt-0.6b-v3";
export const ETA_WINDOW_MS = 5000;
export const ETA_HIDE_BEFORE_MS = 2000;

export interface DownloadByteSample {
  atMs: number;
  bytes: number;
}

export type FirstRunDownloadPhase =
  "idle" | "downloading" | "verifying" | "extracting" | "cancelled" | "failed";

export interface FirstRunModelDownloadViewProps {
  model: ModelInfo | null;
  downloadedMb: number;
  totalMb: number;
  percentage: number;
  secondsLeft: number | null;
  phase: FirstRunDownloadPhase;
  failureCause?: DownloadFailureCause | null;
  failedAtMb?: number | null;
  onCancel: () => void;
  onTryAgain: () => void;
  onSkipForNow: () => void;
}

export function bytesToWholeMb(bytes: number): number {
  if (!Number.isFinite(bytes) || bytes <= 0) return 0;
  return Math.round(bytes / BYTES_PER_MB);
}

export function pickDefaultFirstRunModel(
  models: ModelInfo[],
): ModelInfo | null {
  const nonLegacy = models.filter((m) => !isLegacySource(m));
  if (nonLegacy.length === 0) return models[0] ?? null;

  const exactDefault = nonLegacy.find(
    (m) => m.id === FIRST_RUN_DEFAULT_MODEL_ID,
  );
  if (exactDefault) return exactDefault;

  const measured = nonLegacy.find((m) => Boolean(m.measured_best));
  if (measured) return measured;

  const recommended = nonLegacy.find((m) => m.is_recommended);
  if (recommended) return recommended;

  return nonLegacy[0] ?? null;
}

export function computeRollingEtaSeconds(
  samples: readonly DownloadByteSample[],
  totalBytes: number,
  nowMs: number,
  startedAtMs: number,
): number | null {
  if (nowMs - startedAtMs < ETA_HIDE_BEFORE_MS) {
    return null;
  }
  const windowStart = nowMs - ETA_WINDOW_MS;
  const inWindow = samples.filter((s) => s.atMs >= windowStart);
  if (inWindow.length < 2) {
    return null;
  }
  const first = inWindow[0]!;
  const last = inWindow[inWindow.length - 1]!;
  const deltaSec = (last.atMs - first.atMs) / 1000;
  const deltaBytes = last.bytes - first.bytes;
  if (deltaSec <= 0 || deltaBytes <= 0) {
    return null;
  }
  const remainingBytes = Math.max(0, totalBytes - last.bytes);
  const bytesPerSec = deltaBytes / deltaSec;
  const rawSeconds = remainingBytes / bytesPerSec;
  if (!Number.isFinite(rawSeconds)) {
    return null;
  }
  const rounded = Math.round(rawSeconds / 5) * 5;
  return Math.max(5, rounded);
}

export function estimateDownloadSecondsLeft(
  downloadedBytes: number,
  totalBytes: number,
  speedMbPerSec: number | undefined,
): number | null {
  if (!speedMbPerSec || speedMbPerSec <= 0 || totalBytes <= downloadedBytes) {
    return null;
  }
  const remainingMb = (totalBytes - downloadedBytes) / BYTES_PER_MB;
  const rawSeconds = remainingMb / speedMbPerSec;
  if (!Number.isFinite(rawSeconds)) return null;
  return Math.max(5, Math.round(rawSeconds / 5) * 5);
}

export const FirstRunModelDownloadView: React.FC<
  FirstRunModelDownloadViewProps
> = ({
  model,
  downloadedMb,
  totalMb,
  percentage,
  secondsLeft,
  phase,
  failureCause = null,
  failedAtMb = null,
  onCancel,
  onTryAgain,
  onSkipForNow,
}) => {
  const { t } = useTranslation();
  const modelName =
    model?.name ?? t("onboarding.download.defaultModel", "Speech model");
  const effectiveTotalMb = Math.max(
    1,
    Math.round(totalMb > 0 ? totalMb : (model?.size_mb ?? 0)),
  );
  const isPostTransfer = phase === "verifying" || phase === "extracting";
  const effectiveDoneMb = isPostTransfer
    ? effectiveTotalMb
    : Math.max(0, Math.min(effectiveTotalMb, Math.round(downloadedMb)));
  const clampedPct = isPostTransfer
    ? 100
    : Math.max(0, Math.min(100, Math.round(percentage)));
  const announcedBucket = Math.floor(clampedPct / 10) * 10;
  const stoppedMb = Math.max(0, Math.round(failedAtMb ?? effectiveDoneMb));

  const isActive =
    phase === "downloading" || phase === "verifying" || phase === "extracting";

  let progressLine = t("onboarding.download.progressOf", {
    doneMb: effectiveDoneMb,
    totalMb: effectiveTotalMb,
    defaultValue: "{{doneMb}} MB of {{totalMb}} MB",
  });
  if (!isPostTransfer && secondsLeft !== null) {
    progressLine = t("onboarding.download.progressEta", {
      doneMb: effectiveDoneMb,
      totalMb: effectiveTotalMb,
      seconds: secondsLeft,
      defaultValue:
        "{{doneMb}} MB of {{totalMb}} MB · about {{seconds}} seconds left",
    });
  }

  let failureMessage = t("modelSelector.downloadFailed.other", {
    defaultValue: "Download failed. Please try again.",
  });
  if (failureCause === "network") {
    failureMessage = t("onboarding.download.failedNetwork", {
      doneMb: stoppedMb,
      defaultValue:
        "The download stopped: the connection dropped at {{doneMb}} MB. Nothing was saved.",
    });
  } else if (failureCause === "disk_space") {
    failureMessage = t(
      "modelSelector.downloadFailed.diskSpace",
      "Download failed — not enough disk space.",
    );
  } else if (failureCause === "checksum") {
    failureMessage = t(
      "modelSelector.downloadFailed.checksum",
      "Download failed — file verification did not match.",
    );
  }

  return (
    <div
      data-testid="first-run-model-download"
      className="w-full min-h-screen bg-background pt-6 px-7 pb-7 text-start space-y-4"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("onboarding.download.eyebrow", "Speech model")}
      </p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t("onboarding.download.heading", {
          model: modelName,
          defaultValue: "Downloading {{model}}",
        })}
      </h2>

      <div className="rounded-[12px] border border-mid-gray/25 bg-background p-4 space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="text-[14px] font-semibold text-text">
              {t("onboarding.download.modelSize", {
                model: modelName,
                sizeMb: effectiveTotalMb,
                defaultValue: "{{model}} · {{sizeMb}} MB",
              })}
            </div>
            {phase !== "failed" ? (
              <div className="text-[12.5px] text-mid-gray">{progressLine}</div>
            ) : null}
          </div>
          {isActive ? (
            <button
              type="button"
              onClick={onCancel}
              aria-label={t("onboarding.download.cancelAria", {
                model: modelName,
                defaultValue: "Cancel downloading {{model}}",
              })}
              className="shrink-0 rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("tray.cancel", "Cancel")}
            </button>
          ) : null}
        </div>

        {phase !== "failed" ? (
          <div
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={clampedPct}
            aria-label={t("onboarding.download.progressAria", {
              model: modelName,
              defaultValue: "Download progress for {{model}}",
            })}
            className="h-2 w-full overflow-hidden rounded-full bg-mid-gray/20"
          >
            <div
              style={{ width: `${clampedPct}%` }}
              className="h-full rounded-full bg-logo-primary transition-all duration-150"
            />
          </div>
        ) : null}
        {isActive ? (
          <span className="sr-only" aria-live="polite">
            {t("onboarding.download.progressAnnounce", {
              percent: announcedBucket,
              defaultValue: "{{percent}}% downloaded",
            })}
          </span>
        ) : null}
      </div>

      {phase === "failed" || failureCause ? (
        <div
          role="alert"
          data-testid="first-run-download-failed"
          className="rounded-[12px] border border-mid-gray/35 bg-background p-4 space-y-3"
        >
          <p className="text-[13px] leading-[19px] text-mid-gray">
            {failureMessage}
          </p>
          <div className="flex flex-wrap items-center gap-2.5">
            <button
              type="button"
              onClick={onTryAgain}
              className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-3.5 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              {t("modelSelector.downloadFailed.retry", "Try again")}
            </button>
            <button
              type="button"
              onClick={onSkipForNow}
              className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 bg-background px-3.5 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("modelSelector.downloadFailed.skip", "Skip for now")}
            </button>
          </div>
        </div>
      ) : phase === "cancelled" ? (
        <div
          role="status"
          data-testid="first-run-download-cancelled"
          className="rounded-[12px] border border-mid-gray/35 bg-background p-4"
        >
          <div className="flex flex-wrap items-center gap-2.5">
            <button
              type="button"
              onClick={onTryAgain}
              className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-3.5 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              {t("modelSelector.downloadFailed.retry", "Try again")}
            </button>
            <button
              type="button"
              onClick={onSkipForNow}
              className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 bg-background px-3.5 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("modelSelector.downloadFailed.skip", "Skip for now")}
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
};

export default FirstRunModelDownloadView;
