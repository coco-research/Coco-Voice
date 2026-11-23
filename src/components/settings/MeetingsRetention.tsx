import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { commands as meetingsCommands } from "../../bindings.meetings";

const DOT_FILLED = "●";
const DOT_EMPTY = "○";

export interface DeleteAllMeetingsOutcome {
  deleted_count?: number;
  audio_left_count: number;
}

export interface MeetingsRetentionProps {
  deleteAudioAfterNotes: boolean;
  transcriptDeleteAfterDays: number | null;
  deleteAllStep: 0 | 1 | 2;
  onToggleDeleteAudio: (next: boolean) => void;
  onChangeTranscriptTimer: (days: number | null) => void;
  onRequestDeleteAllStep1: () => void;
  onAdvanceDeleteAllStep2: () => void;
  onCancelDeleteAll: () => void;
  onConfirmDeleteAll: () => void;
  onBack?: () => void;
  showBreadcrumb?: boolean;
  deleteAllBusy?: boolean;
  deleteAllError?: string | null;
  deleteAllDoneMessage?: string | null;
}

export const MeetingsRetentionSection: React.FC<MeetingsRetentionProps> = ({
  deleteAudioAfterNotes,
  transcriptDeleteAfterDays,
  deleteAllStep,
  onToggleDeleteAudio,
  onChangeTranscriptTimer,
  onRequestDeleteAllStep1,
  onAdvanceDeleteAllStep2,
  onCancelDeleteAll,
  onConfirmDeleteAll,
  onBack,
  showBreadcrumb = false,
  deleteAllBusy = false,
  deleteAllError = null,
  deleteAllDoneMessage = null,
}) => {
  const { t } = useTranslation();

  return (
    <section
      data-testid="meetings-retention-section"
      className="space-y-4 rounded-xl border border-mid-gray/20 bg-surface p-4"
    >
      {showBreadcrumb || onBack ? (
        <div className="space-y-1">
          {onBack ? (
            <button
              type="button"
              data-testid="settings-back-link"
              onClick={onBack}
              className="inline-flex items-center gap-1 text-[13px] font-semibold text-logo-primary hover:underline cursor-pointer"
            >
              <span aria-hidden="true">←</span>
              <span>{t("sidebar.settings")}</span>
            </button>
          ) : null}
          <div>
            <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
              {t("sidebar.settings")} › {t("meetings.sidebar")} ›{" "}
              {t("v022.settings.retentionBreadcrumb", "Retention")}
            </p>
            <h2 className="text-[20px] leading-[26px] font-semibold tracking-[-0.02em] text-text">
              {t("v022.settings.retentionBreadcrumb", "Retention")}
            </h2>
          </div>
        </div>
      ) : null}

      <div className="divide-y divide-mid-gray/15">
        <div className="flex items-center justify-between gap-4 py-3 first:pt-0">
          <div className="min-w-0">
            <p className="text-sm font-medium text-text">
              {t("meetings.retention.deleteAudioLabel")}
            </p>
            <p className="text-xs text-mid-gray">
              {t("meetings.retention.deleteAudioDescription")}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2.5">
            <span className="inline-flex items-center gap-1 text-xs font-medium text-text/85">
              <span aria-hidden="true">
                {deleteAudioAfterNotes ? DOT_FILLED : DOT_EMPTY}
              </span>
              <span>
                {deleteAudioAfterNotes
                  ? t("v022.settings.stateOn", "On")
                  : t("v022.settings.stateOff", "Off")}
              </span>
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={deleteAudioAfterNotes}
              aria-label={t("meetings.retention.deleteAudioLabel")}
              onClick={() => onToggleDeleteAudio(!deleteAudioAfterNotes)}
              className={`relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full border transition-colors cursor-pointer ${
                deleteAudioAfterNotes
                  ? "border-logo-primary bg-logo-primary"
                  : "border-mid-gray/50 bg-transparent"
              }`}
            >
              <span
                className={`inline-block h-4 w-4 rounded-full transition-transform ${
                  deleteAudioAfterNotes
                    ? "translate-x-[18px] bg-white"
                    : "translate-x-[2px] bg-text"
                }`}
              />
            </button>
          </div>
        </div>

        <div className="py-3 space-y-2">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <p className="text-sm font-medium text-text">
                {t("meetings.retention.keepTranscriptsLabel")}
              </p>
              <p className="text-xs text-mid-gray">
                {t("meetings.retention.keepTranscriptsDescription")}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-2.5">
              <span className="inline-flex items-center gap-1 rounded-full bg-mid-gray/15 px-2.5 py-0.5 text-xs font-medium text-text/85">
                <span aria-hidden="true">{DOT_FILLED}</span>
                <span>{t("meetings.retention.onThisMac")}</span>
              </span>
              {deleteAllStep === 0 && (
                <button
                  type="button"
                  disabled={deleteAllBusy}
                  onClick={onRequestDeleteAllStep1}
                  className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-1 text-xs font-medium text-red-600 dark:text-red-400 hover:bg-red-500/20 cursor-pointer disabled:opacity-50"
                >
                  {t("meetings.retention.deleteAll")}
                </button>
              )}
            </div>
          </div>

          {deleteAllStep === 1 && (
            <div
              role="alert"
              aria-label={t("meetings.retention.confirmPrompt")}
              className="rounded-lg border border-red-500/30 bg-red-500/10 p-3 space-y-2"
            >
              <p className="text-xs font-medium text-text">
                {t("meetings.retention.confirmPrompt")}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  disabled={deleteAllBusy}
                  onClick={onAdvanceDeleteAllStep2}
                  className="rounded-md bg-red-600 px-3 py-1 text-xs font-medium text-white cursor-pointer disabled:opacity-50"
                >
                  {t("meetings.retention.continueDeleteAll")}
                </button>
                <button
                  type="button"
                  disabled={deleteAllBusy}
                  onClick={onCancelDeleteAll}
                  className="rounded-md border border-mid-gray/30 px-3 py-1 text-xs text-text cursor-pointer"
                >
                  {t("meetings.retention.cancel")}
                </button>
              </div>
            </div>
          )}

          {deleteAllStep === 2 && (
            <div
              role="alert"
              aria-label={t("meetings.retention.confirmStep2Prompt")}
              className="rounded-lg border border-red-500/50 bg-red-500/15 p-3 space-y-2"
            >
              <p className="text-xs font-semibold text-red-600 dark:text-red-400">
                {t("meetings.retention.confirmStep2Prompt")}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  disabled={deleteAllBusy}
                  onClick={onConfirmDeleteAll}
                  className="rounded-md bg-red-600 px-3 py-1 text-xs font-semibold text-white cursor-pointer disabled:opacity-50"
                >
                  {t("meetings.retention.confirmDeleteAll")}
                </button>
                <button
                  type="button"
                  disabled={deleteAllBusy}
                  onClick={onCancelDeleteAll}
                  className="rounded-md border border-mid-gray/30 px-3 py-1 text-xs text-text cursor-pointer"
                >
                  {t("meetings.retention.cancel")}
                </button>
              </div>
            </div>
          )}

          {deleteAllError ? (
            <p role="alert" className="text-xs text-red-600 dark:text-red-400">
              {deleteAllError}
            </p>
          ) : null}

          {deleteAllDoneMessage ? (
            <p role="status" className="text-xs text-mid-gray">
              {deleteAllDoneMessage}
            </p>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-4 pt-3">
          <div className="min-w-0">
            <p className="text-sm font-medium text-text">
              {t("meetings.retention.deleteTimerLabel")}
            </p>
          </div>
          <select
            aria-label={t("meetings.retention.deleteTimerLabel")}
            value={
              transcriptDeleteAfterDays === null
                ? "off"
                : String(transcriptDeleteAfterDays)
            }
            onChange={(e) => {
              const v = e.target.value;
              onChangeTranscriptTimer(v === "off" ? null : Number(v));
            }}
            className="rounded-lg border border-mid-gray/30 bg-background px-2.5 py-1.5 text-xs text-text"
          >
            <option value="off">{t("meetings.retention.deleteTimerOff")}</option>
            {[30, 90, 365].map((days) => (
              <option key={days} value={String(days)}>
                {`${days}d`}
              </option>
            ))}
          </select>
        </div>
      </div>
    </section>
  );
};

async function defaultRequestDeleteToken(): Promise<string> {
  return invoke<string>("request_delete_all_meetings");
}

async function defaultConfirmDeleteWithToken(
  confirmationToken: string,
): Promise<DeleteAllMeetingsOutcome> {
  return invoke<DeleteAllMeetingsOutcome>("delete_all_meetings", {
    confirmationToken,
  });
}

export const MeetingsRetention: React.FC<{
  onBack?: () => void;
  onRetentionChange?: (state: {
    delete_audio_after_notes: boolean;
    transcript_delete_after_days: number | null;
  }) => void;
  requestDeleteToken?: () => Promise<string>;
  confirmDeleteWithToken?: (
    confirmationToken: string,
  ) => Promise<DeleteAllMeetingsOutcome>;
}> = ({
  onBack,
  onRetentionChange,
  requestDeleteToken = defaultRequestDeleteToken,
  confirmDeleteWithToken = defaultConfirmDeleteWithToken,
}) => {
  const { t } = useTranslation();
  const [deleteAudioAfterNotes, setDeleteAudioAfterNotes] = useState(true);
  const [transcriptDays, setTranscriptDays] = useState<number | null>(null);
  const [deleteAllStep, setDeleteAllStep] = useState<0 | 1 | 2>(0);
  const [confirmationToken, setConfirmationToken] = useState<string | null>(
    null,
  );
  const [deleteAllBusy, setDeleteAllBusy] = useState(false);
  const [deleteAllError, setDeleteAllError] = useState<string | null>(null);
  const [deleteAllDoneMessage, setDeleteAllDoneMessage] = useState<
    string | null
  >(null);

  useEffect(() => {
    let mounted = true;
    void meetingsCommands
      .meetingRetentionState()
      .then((res) => {
        if (!mounted || !res) return;
        const nextAudio = res.delete_audio_after_notes ?? true;
        setDeleteAudioAfterNotes(nextAudio);
      })
      .catch(() => {});
    return () => {
      mounted = false;
    };
  }, []);

  const handleToggleDeleteAudio = async (next: boolean) => {
    setDeleteAudioAfterNotes(next);
    onRetentionChange?.({
      delete_audio_after_notes: next,
      transcript_delete_after_days: transcriptDays,
    });
    try {
      await meetingsCommands.changeDeleteAudioAfterNotesSetting(next);
    } catch {
      // Keep UI responsive in non-Tauri preview environments.
    }
  };

  const handleChangeTranscriptTimer = async (days: number | null) => {
    setTranscriptDays(days);
    onRetentionChange?.({
      delete_audio_after_notes: deleteAudioAfterNotes,
      transcript_delete_after_days: days,
    });
    try {
      await meetingsCommands.changeTranscriptDeleteDaysSetting(days);
    } catch {
      // Optional command when meetings feature is disabled.
    }
  };

  const handleAdvanceStep2 = async () => {
    setDeleteAllBusy(true);
    setDeleteAllError(null);
    setDeleteAllDoneMessage(null);
    try {
      const token = await requestDeleteToken();
      if (!token) {
        throw new Error(
          t(
            "meetings.retention.deleteAllTokenMissing",
            "Couldn't prepare deletion. Try again.",
          ),
        );
      }
      setConfirmationToken(token);
      setDeleteAllStep(2);
    } catch (error) {
      setDeleteAllError(
        error instanceof Error
          ? error.message
          : t(
              "meetings.retention.deleteAllFailed",
              "Couldn't delete meetings. Try again.",
            ),
      );
      setConfirmationToken(null);
      setDeleteAllStep(0);
    } finally {
      setDeleteAllBusy(false);
    }
  };

  const handleCancelDeleteAll = () => {
    setConfirmationToken(null);
    setDeleteAllError(null);
    setDeleteAllStep(0);
  };

  const handleConfirmDeleteAll = async () => {
    if (!confirmationToken) {
      setDeleteAllError(
        t(
          "meetings.retention.deleteAllTokenMissing",
          "Confirmation expired. Start again.",
        ),
      );
      setDeleteAllStep(0);
      return;
    }
    setDeleteAllBusy(true);
    setDeleteAllError(null);
    try {
      const result = await confirmDeleteWithToken(confirmationToken);
      setConfirmationToken(null);
      setDeleteAllStep(0);
      const count = result?.deleted_count;
      setDeleteAllDoneMessage(
        typeof count === "number"
          ? t("meetings.retention.deleteAllDoneCount", {
              count,
              defaultValue:
                count === 1
                  ? "Deleted 1 meeting."
                  : `Deleted ${count} meetings.`,
            })
          : t("meetings.retention.deleteAllDone", "Deleted all meetings."),
      );
    } catch (error) {
      setConfirmationToken(null);
      setDeleteAllStep(0);
      setDeleteAllError(
        error instanceof Error
          ? error.message
          : t(
              "meetings.retention.deleteAllFailed",
              "Couldn't delete meetings. Try again.",
            ),
      );
    } finally {
      setDeleteAllBusy(false);
    }
  };

  return (
    <MeetingsRetentionSection
      deleteAudioAfterNotes={deleteAudioAfterNotes}
      transcriptDeleteAfterDays={transcriptDays}
      deleteAllStep={deleteAllStep}
      onToggleDeleteAudio={(next) => void handleToggleDeleteAudio(next)}
      onChangeTranscriptTimer={(days) => void handleChangeTranscriptTimer(days)}
      onRequestDeleteAllStep1={() => {
        setDeleteAllError(null);
        setDeleteAllDoneMessage(null);
        setDeleteAllStep(1);
      }}
      onAdvanceDeleteAllStep2={() => void handleAdvanceStep2()}
      onCancelDeleteAll={handleCancelDeleteAll}
      onConfirmDeleteAll={() => void handleConfirmDeleteAll()}
      onBack={onBack}
      showBreadcrumb={Boolean(onBack)}
      deleteAllBusy={deleteAllBusy}
      deleteAllError={deleteAllError}
      deleteAllDoneMessage={deleteAllDoneMessage}
    />
  );
};
