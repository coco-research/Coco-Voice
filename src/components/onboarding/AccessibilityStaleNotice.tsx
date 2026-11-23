import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { HistoryUpdatePayload } from "@/bindings";
import {
  downloadFailureTranslationKey,
  type DownloadFailureCause,
} from "../../lib/utils/accessibilityStatus";

const SCRATCH_THAT_RE =
  /^(.*?\b(?:scratch that|i mean|no wait)\b)[,.\s]+(.*)$/i;

export function applyTryItSelfCorrection(input: string): {
  cleaned: string;
  droppedPrefix: string | null;
} {
  const trimmed = input.trim();
  const match = SCRATCH_THAT_RE.exec(trimmed);
  if (!match) {
    return { cleaned: trimmed, droppedPrefix: null };
  }
  const dropped = match[1]?.trim() ?? null;
  const remainder = (match[2] ?? "").trim();
  if (!remainder) {
    return { cleaned: trimmed, droppedPrefix: null };
  }
  const cleaned = remainder.charAt(0).toUpperCase() + remainder.slice(1);
  return { cleaned, droppedPrefix: dropped };
}

export async function evaluateTryItTakeResult(
  raw: string,
  finalText: string,
  invoker: (
    cmd: string,
    args: Record<string, unknown>,
  ) => Promise<{ dropped?: string | null }> = (cmd, args) =>
    invoke<{ dropped?: string | null }>(cmd, args),
): Promise<string | null> {
  try {
    const res = await invoker("evaluate_try_it_take", {
      raw,
      finalText,
      final_text: finalText,
    });
    if (res && typeof res.dropped === "string" && res.dropped.trim()) {
      return res.dropped.trim();
    }
    if (res && res.dropped === null) {
      return null;
    }
  } catch {
    // Fallback to client rule when command is unavailable outside Tauri runtime.
  }
  const local = applyTryItSelfCorrection(raw);
  if (
    local.droppedPrefix &&
    !finalText.toLowerCase().includes(local.droppedPrefix.toLowerCase())
  ) {
    return local.droppedPrefix;
  }
  return null;
}

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
  droppedText?: string | null;
  onDone: () => void;
}> = ({ shortcut, spokenCleanedText = null, droppedText = null, onDone }) => {
  const { t } = useTranslation();
  const initialText =
    spokenCleanedText ?? t("onboarding.tryIt.cleanedResult", "Tuesday.");
  const [boxText, setBoxText] = useState(initialText);
  const [droppedPhrase, setDroppedPhrase] = useState<string | null>(
    droppedText,
  );
  const inputRef = useRef<HTMLInputElement>(null);
  const handledHistoryTakeRef = useRef(false);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    let active = true;
    const unlistenPromise = listen<HistoryUpdatePayload>(
      "history-update-payload",
      (event) => {
        if (!active || handledHistoryTakeRef.current) return;
        const payload = event.payload;
        if (!payload || payload.action !== "added" || !payload.entry) return;
        handledHistoryTakeRef.current = true;
        const raw = payload.entry.transcription_text ?? "";
        const finalText =
          payload.entry.post_processed_text?.trim() || raw.trim();
        setBoxText(finalText);
        void evaluateTryItTakeResult(raw, finalText).then((dropped) => {
          if (active) setDroppedPhrase(dropped);
        });
      },
    ).catch(() => null);

    return () => {
      active = false;
      void unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  const handleInputChange = (nextValue: string) => {
    const result = applyTryItSelfCorrection(nextValue);
    if (result.droppedPrefix) {
      setBoxText(result.cleaned);
      setDroppedPhrase(result.droppedPrefix);
      return;
    }
    setBoxText(nextValue);
  };

  return (
    <section
      data-testid="onboarding-try-it-step"
      className="w-full min-h-screen bg-background pt-6 px-7 pb-7 text-start space-y-4"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("onboarding.tryIt.title", "Try it")}
      </p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t("onboarding.tryIt.prompt", {
          shortcut,
          defaultValue:
            'Hold {{shortcut}} and say: "Monday, scratch that, Tuesday"',
        })}
      </h2>
      <input
        ref={inputRef}
        type="text"
        value={boxText}
        onChange={(e) => handleInputChange(e.target.value)}
        aria-label={t("onboarding.tryIt.inputAria", "Try it here")}
        className="w-full rounded-[10px] border border-mid-gray/35 bg-background px-3.5 py-2.5 text-[14px] font-medium text-text focus:border-logo-primary focus:outline-none"
      />
      {droppedPhrase ? (
        <p
          data-testid="try-it-cleanup-explanation"
          className="text-[13px] leading-[19px] text-mid-gray"
        >
          {t("onboarding.tryIt.cleanupExplanationCustom", {
            dropped: droppedPhrase,
            defaultValue:
              'Clean-up dropped "{{dropped}}". That is how every take is cleaned.',
          })}
        </p>
      ) : null}
      <div className="pt-1">
        <button
          type="button"
          onClick={onDone}
          className="inline-flex min-h-[34px] items-center rounded-[10px] bg-logo-primary px-4 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
        >
          {t("onboarding.tryIt.done", "Done")}
        </button>
      </div>
    </section>
  );
};

export default AccessibilityStaleNotice;
