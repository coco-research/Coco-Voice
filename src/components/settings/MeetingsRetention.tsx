import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../../bindings.meetings";
import { Button } from "../ui/Button";
import Badge from "../ui/Badge";
import { SettingsGroup } from "../ui/SettingsGroup";
import { ToggleSwitch } from "../ui/ToggleSwitch";

interface MeetingsRetentionSectionProps {
  deleteAudioAfterNotes: boolean;
  updatingToggle: boolean;
  confirmingDeleteAll: boolean;
  deletingAll: boolean;
  audioLeftCount: number;
  errorKey: string | null;
  onToggleDeleteAudio: (enabled: boolean) => void;
  onRequestDeleteAll: () => void;
  onCancelDeleteAll: () => void;
  onConfirmDeleteAll: () => void;
}

export const MeetingsRetentionSection: React.FC<
  MeetingsRetentionSectionProps
> = ({
  deleteAudioAfterNotes,
  updatingToggle,
  confirmingDeleteAll,
  deletingAll,
  audioLeftCount,
  errorKey,
  onToggleDeleteAudio,
  onRequestDeleteAll,
  onCancelDeleteAll,
  onConfirmDeleteAll,
}) => {
  const { t } = useTranslation();

  return (
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

      <div className="flex flex-col gap-2 px-4 py-3">
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
              {t("meetings.retention.onThisMac")}
            </Badge>
            {!confirmingDeleteAll && (
              <Button
                type="button"
                variant="secondary"
                size="sm"
                disabled={deletingAll}
                onClick={onRequestDeleteAll}
              >
                {t("meetings.retention.deleteAll")}
              </Button>
            )}
          </div>
        </div>

        {confirmingDeleteAll && (
          <div
            className="mt-1 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs"
            role="region"
            aria-label={t("meetings.retention.confirmPrompt")}
          >
            <span className="font-medium text-text">
              {t("meetings.retention.confirmPrompt")}
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
        )}

        {audioLeftCount > 0 && (
          <p className="text-xs text-amber-600" role="status">
            {t("meetings.retention.audioLeftWarning")}
          </p>
        )}

        {errorKey !== null && (
          <p className="text-xs text-red-600" role="alert">
            {t(errorKey)}
          </p>
        )}
      </div>
    </SettingsGroup>
  );
};

export const MeetingsRetention: React.FC = () => {
  const [deleteAudio, setDeleteAudio] = useState(true);
  const [updating, setUpdating] = useState(false);
  const [pendingToken, setPendingToken] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [audioLeftCount, setAudioLeftCount] = useState(0);
  const [errorKey, setErrorKey] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void commands.meetingRetentionState().then((state) => {
      if (!cancelled) setDeleteAudio(state.delete_audio_after_notes);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleToggle = async (enabled: boolean) => {
    const prev = deleteAudio;
    setDeleteAudio(enabled);
    setUpdating(true);
    setErrorKey(null);
    try {
      const res = await commands.changeDeleteAudioAfterNotesSetting(enabled);
      if (res.status === "ok") {
        setDeleteAudio(res.data.delete_audio_after_notes);
      } else {
        setDeleteAudio(prev);
        setErrorKey("meetings.retention.actionFailed");
      }
    } catch {
      setDeleteAudio(prev);
      setErrorKey("meetings.retention.actionFailed");
    } finally {
      setUpdating(false);
    }
  };

  const handleRequestDeleteAll = async () => {
    setErrorKey(null);
    try {
      const res = await commands.requestDeleteAllMeetings();
      if (res.status === "ok") {
        setPendingToken(res.data);
      } else {
        setErrorKey("meetings.retention.actionFailed");
      }
    } catch {
      setErrorKey("meetings.retention.actionFailed");
    }
  };

  const handleConfirmDeleteAll = async () => {
    if (!pendingToken) return;
    setDeleting(true);
    setErrorKey(null);
    try {
      const res = await commands.deleteAllMeetings(pendingToken);
      if (res.status === "ok") {
        setAudioLeftCount(res.data.audio_left_count);
        setPendingToken(null);
      } else {
        setErrorKey("meetings.retention.actionFailed");
      }
    } catch {
      setErrorKey("meetings.retention.actionFailed");
    } finally {
      setDeleting(false);
    }
  };

  return (
    <MeetingsRetentionSection
      deleteAudioAfterNotes={deleteAudio}
      updatingToggle={updating}
      confirmingDeleteAll={pendingToken !== null}
      deletingAll={deleting}
      audioLeftCount={audioLeftCount}
      errorKey={errorKey}
      onToggleDeleteAudio={(enabled) => void handleToggle(enabled)}
      onRequestDeleteAll={() => void handleRequestDeleteAll()}
      onCancelDeleteAll={() => setPendingToken(null)}
      onConfirmDeleteAll={() => void handleConfirmDeleteAll()}
    />
  );
};
