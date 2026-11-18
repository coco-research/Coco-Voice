import React from "react";
import { useTranslation } from "react-i18next";
import {
  downloadFailureTranslationKey,
  type DownloadFailureCause,
} from "../../lib/utils/accessibilityStatus";

const STEPS = [
  "onboarding.permissions.accessibility.stale.step1",
  "onboarding.permissions.accessibility.stale.step2",
  "onboarding.permissions.accessibility.stale.step3",
] as const;

export interface AccessibilityStaleNoticeProps {
  onCheckAgain: () => void;
  onContinueWithoutTyping?: () => void;
}

export const AccessibilityStaleNotice: React.FC<
  AccessibilityStaleNoticeProps
> = ({ onCheckAgain, onContinueWithoutTyping }) => {
  const { t } = useTranslation();

  return (
    <div
      data-testid="stale-accessibility-notice"
      className="flex flex-col items-start gap-2.5 rounded-[12px] border border-amber-500/35 bg-amber-500/10 p-3.5"
    >
      <p className="text-sm font-medium text-text">
        {t("onboarding.permissions.accessibility.stale.description")}
      </p>
      <ol className="list-decimal ps-5 text-sm text-text/85 space-y-1">
        {STEPS.map((stepKey) => (
          <li key={stepKey}>{t(stepKey)}</li>
        ))}
      </ol>
      <div className="flex flex-wrap items-center gap-2 pt-0.5">
        <button
          type="button"
          onClick={onCheckAgain}
          className="px-3.5 py-1.5 rounded-lg bg-logo-primary hover:bg-logo-primary/90 text-white text-sm font-semibold cursor-pointer"
        >
          {t("onboarding.permissions.accessibility.stale.checkAgain")}
        </button>
        {onContinueWithoutTyping ? (
          <button
            type="button"
            onClick={onContinueWithoutTyping}
            className="px-3.5 py-1.5 rounded-lg border border-mid-gray/45 text-text text-sm font-medium hover:bg-mid-gray/10 cursor-pointer"
          >
            {t(
              "onboarding.permissions.accessibility.stale.continueWithoutTyping",
            )}
          </button>
        ) : null}
      </div>
    </div>
  );
};

export const ModelDownloadFailedBanner: React.FC<{
  cause: DownloadFailureCause;
  disabled?: boolean;
  onRetry?: () => void;
  onSkip?: () => void;
}> = ({ cause, disabled = false, onRetry, onSkip }) => {
  const { t } = useTranslation();
  const msgKey = downloadFailureTranslationKey(cause);

  return (
    <div
      data-testid="model-download-failed-banner"
      className="w-full mt-2 flex items-center gap-2 flex-wrap rounded-[9px] border border-red-500/30 bg-red-500/[0.08] px-3 py-2"
    >
      <p role="alert" className="text-xs font-medium text-red-600 flex-1">
        {t(msgKey)}
      </p>
      {onRetry ? (
        <button
          type="button"
          disabled={disabled}
          onClick={onRetry}
          className="rounded-lg border border-mid-gray/45 bg-background px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10 disabled:opacity-50"
        >
          {t("modelSelector.downloadFailed.retry")}
        </button>
      ) : null}
      {onSkip ? (
        <button
          type="button"
          onClick={onSkip}
          className="rounded-lg px-2.5 py-1 text-xs font-medium text-text/80 cursor-pointer hover:text-text hover:bg-mid-gray/10"
        >
          {t("modelSelector.downloadFailed.skip")}
        </button>
      ) : null}
    </div>
  );
};

export const TryItStepCard: React.FC<{
  shortcut: string;
  spokenCleanedText?: string | null;
  onDone: () => void;
}> = ({ shortcut, spokenCleanedText = null, onDone }) => {
  const { t } = useTranslation();

  return (
    <section
      data-testid="onboarding-try-it-step"
      className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-3"
    >
      <h3 className="text-[16px] font-semibold text-text">
        {t("onboarding.tryIt.title")}
      </h3>
      <p className="text-[13.5px] text-text">
        {t("onboarding.tryIt.prompt", { shortcut })}
      </p>
      <div className="rounded-[10px] border border-mid-gray/25 bg-mid-gray/[0.05] px-3 py-2 font-mono text-[13px] font-semibold text-text">
        {spokenCleanedText ?? t("onboarding.tryIt.cleanedResult")}
      </div>
      <div className="flex justify-end">
        <button
          type="button"
          onClick={onDone}
          className="rounded-[9px] bg-logo-primary px-4 py-1.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
        >
          {t("onboarding.tryIt.done")}
        </button>
      </div>
    </section>
  );
};

export default AccessibilityStaleNotice;
