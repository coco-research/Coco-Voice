import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../../bindings.meetings";
import Badge from "../ui/Badge";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";
import { ToggleSwitch } from "../ui/ToggleSwitch";

const DOT_FILLED = "●";
const TIMER_OPTIONS = [null, 7, 30, 90] as const;

export type DeleteAllStep = 0 | 1 | 2;

export interface MeetingsRetentionSectionProps {
  deleteAudioAfterNotes: boolean;
  transcriptDeleteAfterDays: number | null;
  updatingToggle?: boolean;
  deleteAllStep: DeleteAllStep;
  deletingAll?: boolean;
  audioLeftCount?: number;
  errorKey?: string | null;
  onToggleDeleteAudio: (enabled: boolean) => void;
  onChangeTranscriptTimer: (days: number | null) => void;
  onRequestDeleteAllStep1: () => void;
  onAdvanceDeleteAllStep2: () => void;
  onCancelDeleteAll: () => void;
  onConfirmDeleteAll: () => void;
}

export const MeetingsRetentionSection: React.FC<
  MeetingsRetentionSectionProps
> = ({
  deleteAudioAfterNotes,
  transcriptDeleteAfterDays,
  updatingToggle = false,
  deleteAllStep,
  deletingAll = false,
  audioLeftCount = 0,
  errorKey = null,
  onToggleDeleteAudio,
  onChangeTranscriptTimer,
  onRequestDeleteAllStep1,
  onAdvanceDeleteAllStep2,
  onCancelDeleteAll,
  onConfirmDeleteAll,
}) => {
  const { t } = useTranslation();

  const timerOptionNodes = TIMER_OPTIONS.map((days) => {
    const val = days === null ? "off" : String(days);
    const text =
      days === null ? t("meetings.retention.deleteTimerOff") : `${days}d`;
    return (
      <option key={val} value={val}>
        {text}
      </option>
    );
  });

  return (
    <div data-testid="frame-9-retention">
      <SettingsGroup title={t("meetings.retention.title")}>
        <ToggleSwitch
          checked={deleteAudioAfterNotes}
          onChange={onToggleDeleteAudio}
          isUpdating={updatingToggle}
          label={t("meetings.retention.deleteAudioLabel")}
          description={t("meetings.retention.deleteAudioDescription")}
          descriptionMode="inline"
          grouped={true}
        />

        <div className="flex flex-col gap-2 px-4 py-3 border-t border-mid-gray/15">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <div className="text-sm font-medium text-text">
                {t("meetings.retention.keepTranscriptsLabel")}
              </div>
              <p className="mt-0.5 text-xs text-mid-gray">
                {t("meetings.retention.keepTranscriptsDescription")}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <Badge variant="secondary">
                <span aria-hidden="true">{DOT_FILLED}</span>{" "}
                {t("meetings.retention.onThisMac")}
              </Badge>
              {deleteAllStep === 0 ? (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={deletingAll}
                  onClick={onRequestDeleteAllStep1}
                >
                  {t("meetings.retention.deleteAll")}
                </Button>
              ) : null}
            </div>
          </div>

          {deleteAllStep === 1 ? (
            <div
              data-testid="delete-all-step-1"
              role="region"
              aria-label={t("meetings.retention.confirmPrompt")}
              className="mt-1 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-amber-500/35 bg-amber-500/10 px-3 py-2 text-xs"
            >
              <span className="font-medium text-text">
                {t("meetings.retention.confirmPrompt")}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={onCancelDeleteAll}
                >
                  {t("meetings.retention.cancel")}
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  onClick={onAdvanceDeleteAllStep2}
                >
                  {t("meetings.retention.continueDeleteAll")}
                </Button>
              </div>
            </div>
          ) : null}

          {deleteAllStep === 2 ? (
            <div
              data-testid="delete-all-step-2"
              role="region"
              aria-label={t("meetings.retention.confirmStep2Prompt")}
              className="mt-1 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs"
            >
              <span className="font-semibold text-text">
                {t("meetings.retention.confirmStep2Prompt")}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={deletingAll}
                  onClick={onCancelDeleteAll}
                >
                  {t("meetings.retention.cancel")}
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  disabled={deletingAll}
                  onClick={onConfirmDeleteAll}
                >
                  {t("meetings.retention.confirmDeleteAll")}
                </Button>
              </div>
            </div>
          ) : null}

          {audioLeftCount > 0 ? (
            <p className="text-xs text-amber-600" role="status">
              {t("meetings.retention.audioLeftWarning")}
            </p>
          ) : null}

          {errorKey !== null ? (
            <p className="text-xs text-red-600" role="alert">
              {t(errorKey)}
            </p>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-4 px-4 py-3 border-t border-mid-gray/15">
          <label
            htmlFor="transcript-delete-timer"
            className="text-sm font-medium text-text"
          >
            {t("meetings.retention.deleteTimerLabel")}
          </label>
          <select
            id="transcript-delete-timer"
            value={
              transcriptDeleteAfterDays === null
                ? "off"
                : String(transcriptDeleteAfterDays)
            }
            onChange={(e) => {
              const v = e.target.value;
              onChangeTranscriptTimer(v === "off" ? null : Number(v));
            }}
            className="rounded-lg border border-mid-gray/35 bg-background px-2.5 py-1 text-xs font-medium text-text"
          >
            {timerOptionNodes}
          </select>
        </div>
      </SettingsGroup>
    </div>
  );
};

export const MeetingsRetention: React.FC = () => {
  const [deleteAudio, setDeleteAudio] = useState(true);
  const [timerDays, setTimerDays] = useState<number | null>(null);
  const [updating, setUpdating] = useState(false);
  const [step, setStep] = useState<DeleteAllStep>(0);
  const [deleting, setDeleting] = useState(false);
  const [audioLeftCount, setAudioLeftCount] = useState(0);
  const [errorKey, setErrorKey] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void commands.meetingRetentionState().then((state) => {
      if (!cancelled) {
        setDeleteAudio(state.delete_audio_after_notes);
        setTimerDays(state.transcript_delete_after_days ?? null);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <MeetingsRetentionSection
      deleteAudioAfterNotes={deleteAudio}
      transcriptDeleteAfterDays={timerDays}
      updatingToggle={updating}
      deleteAllStep={step}
      deletingAll={deleting}
      audioLeftCount={audioLeftCount}
      errorKey={errorKey}
      onToggleDeleteAudio={(enabled) => {
        setDeleteAudio(enabled);
        setUpdating(true);
        void commands
          .changeDeleteAudioAfterNotesSetting(enabled)
          .finally(() => setUpdating(false));
      }}
      onChangeTranscriptTimer={(days) => {
        setTimerDays(days);
        void commands.changeTranscriptDeleteDaysSetting(days);
      }}
      onRequestDeleteAllStep1={() => setStep(1)}
      onAdvanceDeleteAllStep2={() => setStep(2)}
      onCancelDeleteAll={() => setStep(0)}
      onConfirmDeleteAll={() => {
        setDeleting(true);
        void commands
          .deleteAllMeetings()
          .then((res) => {
            if (res.status === "ok") {
              setAudioLeftCount(res.data.audio_left_count);
              setStep(0);
            } else {
              setErrorKey("meetings.retention.actionFailed");
            }
          })
          .finally(() => setDeleting(false));
      }}
    />
  );
};
