import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  commands,
  type MeetingNoteLineView,
  type MeetingNotesSnapshot,
  type MeetingOffer,
  type MeetingRefusal,
  type MeetingSegmentView,
  type MeetingStatus,
} from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";
import { formatMeetingParticipantsLine } from "./channelLabels";
import { LiveTranscriptSection } from "./LiveTranscriptSection";
import { UpcomingMeetings, upcomingEventKey } from "./UpcomingMeetings";
import type { UpcomingMeeting } from "./upcoming";
import {
  ConsentDialog,
  EditableMeetingTitle,
  formatMeetingClock,
  MeetingFloatingPill,
  OfferPanel,
} from "./MeetingEntryPanels";
import { AddAudioControl } from "./MeetingHistoryExtras";
import {
  CatchUpCard,
  type CatchUpMinutes,
  MeetingNotesSection,
} from "./MeetingNotesPanel";
import {
  MEETING_EXTRAS_READY_EVENT,
  MEETING_NOTES_FAILED_EVENT,
  MEETING_NOTES_PROGRESS_EVENT,
  MEETING_NOTES_READY_EVENT,
  MEETING_NOTES_SO_FAR_EVENT,
  MEETING_SEGMENT_EVENT,
  MEETING_SPEAKERS_READY_EVENT,
  MEETING_STATE_EVENT,
  MEETINGS_OFFER_EVENT,
  MEETINGS_OPEN_CONSENT_EVENT,
  consumePendingMeetingStartRefusal,
  onMeetingStartRefusalHandOff,
  peekPendingMeetingStartRefusal,
  type ExtrasReadyEvent,
  type NotesFailedEvent,
  type NotesProgressEvent,
  type NotesReadyEvent,
  type NotesSoFarEvent,
  type SpeakersReadyEvent,
} from "./meetingEvents";

const DOT_FILLED = "●";
const DOT_OPEN = "○";
const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
export const MEETING_WORDS_STORAGE_KEY = "coco_voice.meeting_custom_words";
export const NOTEPAD_SAVE_DEBOUNCE_MS = 500;

export function formatDictationShortcutLabel(
  raw: string | null | undefined,
): string | null {
  const trimmed = raw?.trim();
  if (!trimmed) return null;
  return trimmed
    .split("+")
    .map((part) => {
      const p = part.trim().toLowerCase();
      if (p === "option" || p === "alt") return "Option";
      if (p === "right_option" || p === "rightoption") return "Right Option";
      if (p === "left_option" || p === "leftoption") return "Left Option";
      if (p === "command" || p === "cmd" || p === "super") return "Command";
      if (p === "control" || p === "ctrl") return "Control";
      if (p === "shift") return "Shift";
      if (p === "space") return "Space";
      if (p === "fn") return "fn";
      return part.trim();
    })
    .join("+");
}

export function normalizeMeetingWordList(
  existing: string[],
  addition: string,
): string[] {
  const trimmed = addition.trim();
  if (!trimmed) return existing;
  if (existing.some((w) => w.toLowerCase() === trimmed.toLowerCase())) {
    return existing;
  }
  return [...existing, trimmed];
}

type ClassifiedStartRefusal =
  { kind: "permission" } | { kind: "device"; code: string } | { kind: "other" };

function isActiveMeetingState(state: MeetingStatus["state"]): boolean {
  return state === "recording" || state === "paused" || state === "stopping";
}

function formatDeviceErrorCode(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "err";
  const parenIdx = trimmed.indexOf(" (");
  if (parenIdx > 0) {
    return trimmed.slice(0, parenIdx).trim();
  }
  return trimmed;
}

function classifyStartRefusal(
  refusal: MeetingRefusal | string | null | undefined,
): ClassifiedStartRefusal | null {
  if (refusal === null || refusal === undefined) return null;
  if (refusal === "system_audio_permission_denied") {
    return { kind: "permission" };
  }
  if (typeof refusal === "object" && "system_audio_tap_failed" in refusal) {
    const detail = refusal.system_audio_tap_failed;
    if (detail?.cause === "permission") {
      return { kind: "permission" };
    }
    return {
      kind: "device",
      code: formatDeviceErrorCode(detail?.code ?? ""),
    };
  }
  return { kind: "other" };
}

export interface RecordMeetingSectionProps {
  offerEnabled: boolean;
  offer: MeetingOffer | null;
  consentOpen: boolean;
  renderInlineConsent?: boolean;
  status: MeetingStatus;
  segments: MeetingSegmentView[];
  meetingWords: string[];
  meetingTitle?: string;
  onRenameTitle?: (nextTitle: string) => void | Promise<void>;
  onToggleOffer: (enabled: boolean) => void;
  onOpenConsent: () => void;
  onConfirmStart: () => void;
  onCancelConsent: () => void;
  onDismissOffer: (bundleId: string) => void;
  onStopMeeting: () => void;
  onAddMeetingWord: (word: string) => void;
  onRemoveMeetingWord: (word: string) => void;
  onPauseMeeting?: () => void;
  onResumeMeeting?: () => void;
  onAppendAudio?: () => void;
  startRefusal?: MeetingRefusal | string | null;
  onConfirmStartMicOnly?: () => void;
  onOpenSystemSettings?: () => void;
  onDismissStartRefusal?: () => void;
  catchUpLines?: MeetingNoteLineView[];
  catchUpOpen?: boolean;
  catchUpMinutes?: CatchUpMinutes;
  catchUpNotEnoughSaid?: boolean;
  onChangeCatchUpMinutes?: (minutes: CatchUpMinutes) => void;
  onRequestCatchUp?: (minutes?: CatchUpMinutes) => void;
  notesSnapshot?: MeetingNotesSnapshot | null;
  notepadDraft?: string;
  retryingNotes?: boolean;
  keepFinishedSession?: boolean;
  onNotepadChange?: (val: string) => void;
  onRetryNotes?: () => void;
  dictationShortcut?: string | null;
  pushToTalk?: boolean;
  comingUpOpen?: boolean;
  startedUpcomingKey?: string | null;
  onStartUpcoming?: (meeting: UpcomingMeeting) => void;
  onOpenComingUp?: () => void;
  onCloseComingUp?: () => void;
}

export const RecordMeetingSection: React.FC<RecordMeetingSectionProps> = ({
  offerEnabled,
  offer,
  consentOpen,
  renderInlineConsent = false,
  status,
  segments,
  meetingWords: _meetingWords,
  meetingTitle,
  onRenameTitle,
  onToggleOffer,
  onOpenConsent,
  onConfirmStart,
  onCancelConsent,
  onDismissOffer,
  onStopMeeting,
  onAddMeetingWord: _onAddMeetingWord,
  onRemoveMeetingWord: _onRemoveMeetingWord,
  onPauseMeeting,
  onResumeMeeting,
  onAppendAudio,
  startRefusal = null,
  onConfirmStartMicOnly,
  onOpenSystemSettings,
  onDismissStartRefusal,
  catchUpLines = [],
  catchUpOpen = false,
  catchUpMinutes = 5,
  catchUpNotEnoughSaid = false,
  onChangeCatchUpMinutes,
  onRequestCatchUp,
  notesSnapshot = null,
  notepadDraft = "",
  retryingNotes = false,
  keepFinishedSession = false,
  onNotepadChange,
  onRetryNotes,
  dictationShortcut = null,
  pushToTalk = true,
  comingUpOpen = false,
  startedUpcomingKey = null,
  onStartUpcoming,
  onOpenComingUp,
  onCloseComingUp,
}) => {
  const { t } = useTranslation();
  const [searchQuery, setSearchQuery] = useState("");
  const [jumpTarget, setJumpTarget] = useState<{
    startMs: number;
    seq: number;
  } | null>(null);
  const jumpSeqRef = useRef(0);

  const isPaused = status.state === "paused";
  const isStopping = status.state === "stopping";
  const isRecording = status.state === "recording" || isPaused;
  const hasSpokenSegments = segments.some((s) => s.text.trim().length > 0);
  const notesInProgress =
    status.state === "stopping" ||
    notesSnapshot?.state === "running" ||
    retryingNotes;
  const hasPostStopSession =
    status.state !== "consent_open" &&
    status.meeting_id !== null &&
    (notesInProgress ||
      (keepFinishedSession && (segments.length > 0 || notesSnapshot !== null)));
  const activeSession = isRecording || hasPostStopSession;
  const sessionTitle =
    meetingTitle?.trim() || t("meetings.record.defaultTitle", "Meeting");
  const startProblem = classifyStartRefusal(startRefusal);

  const handleJumpToSegment = useCallback((startMs: number) => {
    jumpSeqRef.current += 1;
    setJumpTarget({ startMs, seq: jumpSeqRef.current });
  }, []);

  if (activeSession) {
    const hasNotesSoFar = (notesSnapshot?.lines?.length ?? 0) > 0;
    const handleRetry = onRetryNotes ?? (() => {});
    return (
      <div
        data-testid="page-2-record-meeting"
        className="max-w-3xl w-full mx-auto space-y-3.5"
      >
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-[14rem] flex-1">
            <p className={EYEBROW}>{t("meetings.sidebar")}</p>
            <EditableMeetingTitle
              title={sessionTitle}
              onRename={onRenameTitle}
            />
            <p
              data-testid="live-meeting-participants"
              className="mt-0.5 text-[12.5px] text-mid-gray"
            >
              {formatMeetingParticipantsLine(
                status.attendees,
                undefined,
                t,
                Boolean(status.mic_only),
              )}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2 pt-1">
            {isPaused ? (
              <>
                <span className="inline-flex items-center gap-1.5 rounded-full border border-mid-gray/35 bg-background px-3 py-1 text-[12.5px] font-semibold text-mid-gray">
                  <span aria-hidden="true">{DOT_OPEN}</span>
                  <span>{t("meetings.storage.pausedBadge", "Paused")}</span>
                  <span className="font-mono tabular-nums">
                    {formatMeetingClock(status.elapsed_secs)}
                  </span>
                </span>
                {status.mic_only ? (
                  <span
                    data-testid="mic-only-chip"
                    className="inline-flex items-center rounded-full border border-mid-gray/40 px-2 py-0.5 text-[11px] font-bold tracking-[0.04em] text-text/80"
                  >
                    {t("meetings.pill.micOnly", "Mic only")}
                  </span>
                ) : null}
              </>
            ) : isStopping ? (
              <span
                role="status"
                className="inline-flex items-center gap-1.5 rounded-full border border-mid-gray/35 bg-background px-3 py-1 text-[12.5px] font-semibold text-mid-gray"
              >
                <span>
                  {t("meetings.list.finishingNotes", "Finishing notes")}
                </span>
              </span>
            ) : isRecording ? (
              <>
                <MeetingFloatingPill
                  elapsedSecs={status.elapsed_secs}
                  micOnly={Boolean(status.mic_only)}
                  onStop={onStopMeeting}
                />
                {status.state === "recording" && onPauseMeeting ? (
                  <button
                    type="button"
                    onClick={onPauseMeeting}
                    className="inline-flex min-h-[30px] items-center rounded-full border border-mid-gray/40 bg-background px-3 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("meetings.storage.pause", "Pause")}
                  </button>
                ) : null}
              </>
            ) : null}
          </div>
        </div>

        {renderInlineConsent ? (
          <ConsentDialog
            open={consentOpen}
            onConfirmStart={onConfirmStart}
            onCancel={onCancelConsent}
          />
        ) : null}

        {isPaused ? (
          <section className="rounded-[14px] border border-mid-gray/25 bg-background p-3.5 space-y-2.5">
            <div className="flex flex-wrap items-center gap-2">
              {onResumeMeeting ? (
                <button
                  type="button"
                  onClick={onResumeMeeting}
                  className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
                >
                  {t("meetings.storage.resume", "Resume")}
                </button>
              ) : null}
              <button
                type="button"
                onClick={onStopMeeting}
                className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[13px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("meetings.pill.stop", "Stop")}
              </button>
            </div>
            <p className="text-[12.5px] text-mid-gray">
              {t(
                "meetings.storage.pausedHint",
                "Paused: nothing is recorded. Still one meeting.",
              )}
            </p>
          </section>
        ) : null}

        {status.state === "recording" && onRequestCatchUp ? (
          <CatchUpCard
            lines={catchUpLines}
            open={catchUpOpen}
            minutes={catchUpMinutes}
            notEnoughSaid={catchUpNotEnoughSaid}
            onSelectMinutes={onChangeCatchUpMinutes}
            onRequestCatchUp={onRequestCatchUp}
            onJumpToSegment={handleJumpToSegment}
          />
        ) : null}

        {(!isRecording || hasNotesSoFar) && onNotepadChange ? (
          <MeetingNotesSection
            recording={isRecording}
            stopping={isStopping}
            snapshot={notesSnapshot}
            notepadDraft={notepadDraft}
            retrying={retryingNotes}
            segments={segments}
            hasSpokenSegments={hasSpokenSegments}
            showNotesSoFarCard={isRecording}
            onNotepadChange={onNotepadChange}
            onJumpToSegment={handleJumpToSegment}
            onRetry={handleRetry}
          />
        ) : null}

        <LiveTranscriptSection
          status={status}
          segments={segments}
          hideHeaderControls={true}
          jumpTarget={jumpTarget}
          dictationShortcut={dictationShortcut}
          pushToTalk={pushToTalk}
          onStop={onStopMeeting}
          onPause={onPauseMeeting}
          onResume={onResumeMeeting}
          searchQuery={searchQuery}
          searchPlaceholder={
            segments.length > 3
              ? t(
                  "meetings.storage.searchTranscriptPlaceholder",
                  "Search this transcript",
                )
              : undefined
          }
          onSearchChange={segments.length > 3 ? setSearchQuery : undefined}
        />

        {isPaused && onAppendAudio ? (
          <AddAudioControl onConfirmAppendAudio={onAppendAudio} />
        ) : null}

        {isRecording && !hasNotesSoFar && !isPaused && onNotepadChange ? (
          <MeetingNotesSection
            recording={true}
            snapshot={notesSnapshot}
            notepadDraft={notepadDraft}
            retrying={retryingNotes}
            segments={segments}
            hasSpokenSegments={hasSpokenSegments}
            showNotesSoFarCard={true}
            onNotepadChange={onNotepadChange}
            onJumpToSegment={handleJumpToSegment}
            onRetry={handleRetry}
          />
        ) : null}
      </div>
    );
  }

  if (!consentOpen && comingUpOpen && !startRefusal) {
    return (
      <div
        data-testid="page-2-coming-up-view"
        className="max-w-3xl w-full mx-auto space-y-3"
      >
        {renderInlineConsent ? (
          <ConsentDialog
            open={consentOpen}
            onConfirmStart={onConfirmStart}
            onCancel={onCancelConsent}
          />
        ) : null}
        <UpcomingMeetings
          mode="page"
          startedKey={startedUpcomingKey}
          onOpenConsent={onOpenConsent}
          onStartNotes={onStartUpcoming}
          onBack={onCloseComingUp}
        />
      </div>
    );
  }

  return (
    <div
      data-testid="page-2-record-meeting"
      className="max-w-3xl w-full mx-auto space-y-3.5"
    >
      <div className="space-y-1">
        <p className={EYEBROW}>{t("meetings.sidebar")}</p>
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {consentOpen ? sessionTitle : t("sidebar.recordMeeting")}
        </h2>
        {consentOpen ? (
          <div className="flex flex-wrap items-center gap-2 pt-0.5 text-[12.5px] text-mid-gray">
            <span className="inline-flex items-center gap-1.5 font-semibold text-mid-gray">
              <span aria-hidden="true">{DOT_OPEN}</span>
              <span>{t("meetings.offer.notRecording")}</span>
            </span>
            <span aria-hidden="true">·</span>
            <span>
              {t(
                "meetings.consent.nothingSavedYet",
                "Nothing is saved until you start.",
              )}
            </span>
          </div>
        ) : null}
      </div>

      {offer ? (
        <OfferPanel
          offer={offer}
          onStartNotes={onOpenConsent}
          onDismiss={onDismissOffer}
        />
      ) : null}

      {renderInlineConsent ? (
        <ConsentDialog
          open={consentOpen}
          onConfirmStart={onConfirmStart}
          onCancel={onCancelConsent}
        />
      ) : null}

      {!consentOpen ? (
        <>
          <section className="rounded-[14px] border border-mid-gray/25 bg-surface p-4 space-y-2.5">
            <h3 className="text-[15px] font-semibold text-text">
              {t("meetings.record.startNowHeading")}
            </h3>
            <p className="text-[13px] leading-[19px] text-mid-gray">
              {t(
                "meetings.record.startNowDescription",
                "Records you (Me) from your microphone and everyone else (Others) from the sound of the call. Transcribed on this Mac. You will be asked to tell the room first.",
              )}
            </p>
            <button
              type="button"
              onClick={onOpenConsent}
              className="inline-flex min-h-[34px] items-center gap-1.5 rounded-[10px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              <span aria-hidden="true">{DOT_FILLED}</span>
              <span>{t("sidebar.startRecording")}</span>
            </button>
          </section>

          {startProblem &&
          (startProblem.kind === "permission" ||
            startProblem.kind === "device") ? (
            <section
              data-testid="meeting-start-problem-panel"
              aria-label={t(
                "meetings.startProblem.title",
                "Your Mac's sound isn't being recorded",
              )}
              className="rounded-[14px] border border-mid-gray/25 bg-surface p-4 space-y-2.5"
            >
              <h3 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
                {t(
                  "meetings.startProblem.title",
                  "Your Mac's sound isn't being recorded",
                )}
              </h3>
              <p className="text-[13px] leading-[19px] text-mid-gray">
                {startProblem.kind === "permission"
                  ? t(
                      "meetings.startProblem.permissionBody",
                      "Coco Voice needs permission to record your Mac's audio.",
                    )
                  : t("meetings.startProblem.deviceBody", {
                      code: startProblem.code,
                      defaultValue: `The audio device didn't start (error ${startProblem.code}).`,
                    })}
              </p>
              <div className="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  data-testid="meeting-start-mic-only-btn"
                  onClick={onConfirmStartMicOnly}
                  className="inline-flex min-h-[34px] items-center rounded-[10px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
                >
                  {t(
                    "meetings.startProblem.recordMicOnly",
                    "Record my mic only",
                  )}
                </button>
                {startProblem.kind === "permission" ? (
                  <button
                    type="button"
                    data-testid="meeting-open-system-settings-btn"
                    onClick={onOpenSystemSettings}
                    className="inline-flex min-h-[34px] items-center rounded-[10px] border border-mid-gray/40 px-3.5 text-[13px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("accessibility.openSettings", "Open System Settings")}
                  </button>
                ) : null}
                <button
                  type="button"
                  data-testid="meeting-start-problem-cancel-btn"
                  onClick={onDismissStartRefusal ?? onCancelConsent}
                  className="inline-flex min-h-[34px] items-center rounded-[10px] border border-mid-gray/40 px-3.5 text-[13px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("meetings.consent.cancel", "Cancel")}
                </button>
              </div>
            </section>
          ) : startProblem ? (
            <section
              role="alert"
              data-testid="meeting-start-refusal-message"
              className="rounded-[14px] border border-mid-gray/25 bg-surface p-4"
            >
              <p className="text-[13px] font-medium text-text">
                {t(
                  "meetings.consent.cannotStart",
                  "Could not start the meeting.",
                )}
              </p>
            </section>
          ) : null}

          <section className="rounded-[14px] border border-mid-gray/25 bg-surface p-4 space-y-2">
            <h3 className="text-[14px] font-semibold text-text">
              {t("meetings.record.whenCallStartsHeading")}
            </h3>
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0 space-y-0.5">
                <p className="text-[13.5px] font-normal text-text">
                  {t("meetings.record.offerLabel")}
                </p>
                <p className="text-[12.5px] text-mid-gray">
                  {t("meetings.record.offerDescription")}
                </p>
              </div>
              <label className="inline-flex shrink-0 items-center cursor-pointer">
                <input
                  type="checkbox"
                  role="switch"
                  className="sr-only peer"
                  checked={offerEnabled}
                  aria-label={t("meetings.record.offerLabel")}
                  onChange={(e) => onToggleOffer(e.target.checked)}
                />
                <div className="relative h-6 w-11 rounded-full bg-mid-gray/20 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-logo-primary peer-checked:bg-background-ui after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white" />
              </label>
            </div>
          </section>

          <UpcomingMeetings
            mode="card"
            startedKey={startedUpcomingKey}
            onOpenConsent={onOpenConsent}
            onStartNotes={onStartUpcoming}
            onOpenView={onOpenComingUp}
          />

          <p className="text-[12px] text-mid-gray">
            {t("meetings.record.dictationKeepsWorking")}
          </p>
        </>
      ) : null}
    </div>
  );
};

export const RecordMeetingPage: React.FC = () => {
  const { settings } = useSettings();
  const [comingUpOpen, setComingUpOpen] = useState(false);
  const [startedUpcomingKey, setStartedUpcomingKey] = useState<string | null>(
    null,
  );
  const [offerEnabled, setOfferEnabled] = useState(true);
  const [offer, setOffer] = useState<MeetingOffer | null>(null);
  const [consentOpen, setConsentOpen] = useState(false);
  const [startRefusal, setStartRefusal] = useState<
    MeetingRefusal | string | null
  >(() => peekPendingMeetingStartRefusal());
  const [status, setStatus] = useState<MeetingStatus>({
    state: "idle",
    elapsed_secs: 0,
    meeting_id: null,
    mic_name: null,
    warnings: [],
  });
  const [segments, setSegments] = useState<MeetingSegmentView[]>([]);
  const [meetingTitle, setMeetingTitle] = useState<string>("");
  const [meetingWords, setMeetingWords] = useState<string[]>([]);
  const [catchUpLines, setCatchUpLines] = useState<MeetingNoteLineView[]>([]);
  const [catchUpOpen, setCatchUpOpen] = useState(false);
  const [catchUpMinutes, setCatchUpMinutes] = useState<CatchUpMinutes>(5);
  const [catchUpNotEnoughSaid, setCatchUpNotEnoughSaid] = useState(false);
  const [notesSnapshot, setNotesSnapshot] =
    useState<MeetingNotesSnapshot | null>(null);
  const [notepadDraft, setNotepadDraft] = useState("");
  const [retryingNotes, setRetryingNotes] = useState(false);
  const [keepFinishedSession, setKeepFinishedSession] = useState(false);

  const activeMeetingIdRef = useRef<number | null>(null);
  const pendingUpcomingRef = useRef<UpcomingMeeting | null>(null);
  const upcomingConsentRef = useRef(false);
  const notepadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingNotepadRef = useRef<{ meetingId: number; text: string } | null>(
    null,
  );

  const flushNotepad = useCallback(() => {
    if (notepadTimerRef.current !== null) {
      clearTimeout(notepadTimerRef.current);
      notepadTimerRef.current = null;
    }
    const pending = pendingNotepadRef.current;
    pendingNotepadRef.current = null;
    if (pending) {
      void commands.saveMeetingNotepad(pending.meetingId, pending.text);
    }
  }, []);

  const resetFinishedSessionState = useCallback(() => {
    setKeepFinishedSession(false);
    setSegments([]);
    setNotesSnapshot(null);
    setMeetingTitle("");
  }, []);

  const refreshMeetingData = useCallback(async (meetingId: number) => {
    try {
      const rows =
        await invoke<Array<{ id: number; title: string }>>("list_meetings");
      const matched = Array.isArray(rows)
        ? rows.find((m) => m.id === meetingId)
        : undefined;
      if (matched?.title) setMeetingTitle(matched.title);
    } catch {
      // Keep current title if list_meetings is unavailable.
    }
    const snapRes = await commands.meetingNotesSnapshot(meetingId);
    if (snapRes.status === "ok") {
      setNotesSnapshot(snapRes.data);
      if (snapRes.data.state === "running") {
        setKeepFinishedSession(true);
      }
      setNotepadDraft((prev) =>
        pendingNotepadRef.current ? prev : (snapRes.data.notepad_text ?? ""),
      );
    }
  }, []);

  useEffect(() => {
    const handedOff = consumePendingMeetingStartRefusal();
    if (handedOff !== null) {
      setStartRefusal(handedOff);
      setConsentOpen(false);
    }
    return onMeetingStartRefusalHandOff(() => {
      const next = consumePendingMeetingStartRefusal();
      if (next !== null) {
        setStartRefusal(next);
        setConsentOpen(false);
      }
    });
  }, []);

  useEffect(() => {
    activeMeetingIdRef.current = status.meeting_id;
  }, [status.meeting_id]);

  // D-B7: when PAGE 2 is open and showing an in-page offer, keep the floating
  // native offer panel hidden so both surfaces never appear at once.
  useEffect(() => {
    if (offer) {
      void commands.setMeetingOfferVisible(false);
    }
  }, [offer]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const current = await commands.meetingStatus();
      if (!cancelled) {
        setStatus(current);
        activeMeetingIdRef.current = current.meeting_id;
        if (current.title) setMeetingTitle(current.title);
        if (isActiveMeetingState(current.state)) {
          setKeepFinishedSession(true);
        }
      }
      if (current.meeting_id !== null) {
        const segRes = await commands.meetingSegments(current.meeting_id);
        if (!cancelled && segRes.status === "ok") setSegments(segRes.data);
        if (!cancelled) await refreshMeetingData(current.meeting_id);
      }
      const offerRes = await commands.currentMeetingOffer();
      if (!cancelled && offerRes.status === "ok") setOffer(offerRes.data);
      const wordsRes = await commands.getMeetingCustomWords();
      if (!cancelled && wordsRes.status === "ok") {
        setMeetingWords(wordsRes.data);
      }
    })();

    const unlisteners: Array<() => void> = [];
    void listen<MeetingStatus>(MEETING_STATE_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      const prevId = activeMeetingIdRef.current;
      activeMeetingIdRef.current = ev.payload.meeting_id;
      setStatus(ev.payload);
      if (ev.payload.state !== "consent_open") {
        setConsentOpen(false);
      } else {
        resetFinishedSessionState();
      }
      if (isActiveMeetingState(ev.payload.state)) {
        setStartRefusal(null);
        setKeepFinishedSession(true);
      }
      if (ev.payload.title) {
        setMeetingTitle(ev.payload.title);
      }
      if (ev.payload.meeting_id !== null && ev.payload.meeting_id !== prevId) {
        setSegments([]);
        setNotesSnapshot(null);
        setMeetingTitle(ev.payload.title ?? "");
        void commands.meetingSegments(ev.payload.meeting_id).then((res) => {
          if (!cancelled && res.status === "ok") setSegments(res.data);
        });
        void refreshMeetingData(ev.payload.meeting_id);
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<MeetingSegmentView>(MEETING_SEGMENT_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (
        activeMeetingIdRef.current === null ||
        ev.payload.meeting_id === activeMeetingIdRef.current
      ) {
        setSegments((prev) => [...prev, ev.payload]);
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<MeetingOffer | null>(MEETINGS_OFFER_EVENT, (ev) => {
      if (!cancelled) setOffer(ev.payload ?? null);
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen(MEETINGS_OPEN_CONSENT_EVENT, () => {
      if (!cancelled) {
        if (upcomingConsentRef.current) {
          upcomingConsentRef.current = false;
        } else {
          pendingUpcomingRef.current = null;
        }
        resetFinishedSessionState();
        setStartRefusal(null);
        setConsentOpen(true);
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<NotesProgressEvent>(MEETING_NOTES_PROGRESS_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (activeMeetingIdRef.current !== ev.payload.meeting_id) return;
      setRetryingNotes(false);
      setKeepFinishedSession(true);
      setNotesSnapshot((prev) => ({
        meeting_id: ev.payload.meeting_id,
        state: "running",
        progress_label: ev.payload.progress_label,
        progress_step: ev.payload.calls_done,
        progress_total: ev.payload.calls_total,
        elapsed_secs: ev.payload.elapsed_secs,
        estimate_secs: ev.payload.estimate_secs,
        time_to_notes_secs: prev?.time_to_notes_secs ?? null,
        failure_reason: null,
        notepad_text: prev?.notepad_text ?? "",
        lines: prev?.lines ?? [],
        oversized_count: prev?.oversized_count ?? 0,
      }));
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<NotesSoFarEvent>(MEETING_NOTES_SO_FAR_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (activeMeetingIdRef.current !== ev.payload.meeting_id) return;
      setNotesSnapshot((prev) => ({
        meeting_id: ev.payload.meeting_id,
        state: prev?.state ?? "idle",
        progress_label: prev?.progress_label ?? null,
        progress_step: prev?.progress_step,
        progress_total: prev?.progress_total,
        elapsed_secs: prev?.elapsed_secs ?? 0,
        estimate_secs: prev?.estimate_secs ?? null,
        time_to_notes_secs: prev?.time_to_notes_secs ?? null,
        failure_reason: null,
        notepad_text: prev?.notepad_text ?? "",
        lines: ev.payload.lines,
        oversized_count: prev?.oversized_count ?? 0,
      }));
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<NotesReadyEvent>(MEETING_NOTES_READY_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (activeMeetingIdRef.current !== ev.payload.meeting_id) return;
      setRetryingNotes(false);
      if (ev.payload.title) setMeetingTitle(ev.payload.title);
      void refreshMeetingData(ev.payload.meeting_id);
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<SpeakersReadyEvent>(MEETING_SPEAKERS_READY_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (activeMeetingIdRef.current !== ev.payload.meeting_id) return;
      void commands.meetingSegments(ev.payload.meeting_id).then((res) => {
        if (!cancelled && res.status === "ok") setSegments(res.data);
      });
      void refreshMeetingData(ev.payload.meeting_id);
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<ExtrasReadyEvent>(MEETING_EXTRAS_READY_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (activeMeetingIdRef.current !== ev.payload.meeting_id) return;
      void refreshMeetingData(ev.payload.meeting_id);
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<NotesFailedEvent>(MEETING_NOTES_FAILED_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      if (activeMeetingIdRef.current !== ev.payload.meeting_id) return;
      setRetryingNotes(false);
      setNotesSnapshot((prev) => ({
        meeting_id: ev.payload.meeting_id,
        state: "failed",
        progress_label: null,
        elapsed_secs: prev?.elapsed_secs ?? 0,
        estimate_secs: null,
        time_to_notes_secs: null,
        failure_reason: ev.payload.reason,
        notepad_text: prev?.notepad_text ?? "",
        lines: prev?.lines ?? [],
        oversized_count: prev?.oversized_count ?? 0,
      }));
      void refreshMeetingData(ev.payload.meeting_id);
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    return () => {
      cancelled = true;
      flushNotepad();
      for (const u of unlisteners) u();
    };
  }, [flushNotepad, refreshMeetingData, resetFinishedSessionState]);

  const persistWords = (next: string[]) => {
    setMeetingWords(next);
    void commands.setMeetingCustomWords(next);
  };

  const handleNotepadChange = (val: string) => {
    setNotepadDraft(val);
    if (status.meeting_id === null) return;
    pendingNotepadRef.current = { meetingId: status.meeting_id, text: val };
    if (notepadTimerRef.current !== null) {
      clearTimeout(notepadTimerRef.current);
    }
    notepadTimerRef.current = setTimeout(() => {
      flushNotepad();
    }, NOTEPAD_SAVE_DEBOUNCE_MS);
  };

  const handleRetryNotes = () => {
    if (status.meeting_id === null) return;
    const mid = status.meeting_id;
    setRetryingNotes(true);
    setNotesSnapshot((prev) => ({
      meeting_id: mid,
      state: "running",
      progress_label: prev?.progress_label ?? null,
      progress_step: 1,
      progress_total: 1,
      elapsed_secs: 0,
      estimate_secs: null,
      time_to_notes_secs: null,
      failure_reason: null,
      notepad_text: notepadDraft,
      lines: prev?.lines ?? [],
      oversized_count: prev?.oversized_count ?? 0,
    }));
    void commands.retryMeetingNotes(mid).then((res) => {
      setRetryingNotes(false);
      if (res.status === "ok") {
        setNotesSnapshot(res.data);
      }
    });
  };

  const handleRenameTitle = async (nextTitle: string) => {
    setMeetingTitle(nextTitle);
    if (status.meeting_id !== null) {
      const res = await commands.renameMeeting(status.meeting_id, nextTitle);
      if (res.status === "ok") {
        setMeetingTitle(res.data);
      }
    }
  };

  const dictationShortcut = formatDictationShortcutLabel(
    settings?.bindings?.transcribe?.current_binding,
  );
  const pushToTalk = settings?.push_to_talk ?? true;

  const commitPendingUpcoming = () => {
    const pending = pendingUpcomingRef.current;
    upcomingConsentRef.current = false;
    pendingUpcomingRef.current = null;
    if (!pending) return;
    setStartedUpcomingKey(upcomingEventKey(pending));
    void commands.keepUpcomingForNotes(
      pending.start,
      pending.title,
      pending.calendar_name,
    );
  };

  return (
    <RecordMeetingSection
      offerEnabled={offerEnabled}
      offer={offer}
      consentOpen={
        (consentOpen || status.state === "consent_open") &&
        startRefusal === null
      }
      renderInlineConsent={true}
      status={status}
      segments={segments}
      meetingWords={meetingWords}
      meetingTitle={meetingTitle}
      dictationShortcut={dictationShortcut}
      pushToTalk={pushToTalk}
      comingUpOpen={comingUpOpen}
      startedUpcomingKey={startedUpcomingKey}
      onStartUpcoming={(meeting) => {
        pendingUpcomingRef.current = meeting;
        upcomingConsentRef.current = true;
        resetFinishedSessionState();
        setStartRefusal(null);
        setConsentOpen(true);
        void commands
          .openMeetingConsent()
          .then((res) => {
            if (res.status !== "ok" || res.data?.state !== "consent_open") {
              upcomingConsentRef.current = false;
              pendingUpcomingRef.current = null;
            }
          })
          .catch(() => {
            upcomingConsentRef.current = false;
            pendingUpcomingRef.current = null;
          });
      }}
      onOpenComingUp={() => setComingUpOpen(true)}
      onCloseComingUp={() => setComingUpOpen(false)}
      onRenameTitle={handleRenameTitle}
      onToggleOffer={(enabled) => {
        setOfferEnabled(enabled);
        void commands.changeMeetingsEnabledSetting(enabled);
      }}
      onOpenConsent={() => {
        upcomingConsentRef.current = false;
        pendingUpcomingRef.current = null;
        resetFinishedSessionState();
        setStartRefusal(null);
        setConsentOpen(true);
        void commands.openMeetingConsent();
      }}
      onConfirmStart={() => {
        setConsentOpen(false);
        setStartRefusal(null);
        void commands.confirmMeetingStart().then((res) => {
          if (res.status === "ok") {
            commitPendingUpcoming();
            setStartRefusal(null);
            setKeepFinishedSession(true);
            setStatus(res.data);
          } else {
            setStartRefusal(res.error ?? "system_audio_unavailable");
          }
        });
      }}
      startRefusal={startRefusal}
      onConfirmStartMicOnly={() => {
        setConsentOpen(false);
        void commands.confirmMeetingStartMicOnly().then((res) => {
          if (res.status === "ok") {
            commitPendingUpcoming();
            setStartRefusal(null);
            setKeepFinishedSession(true);
            setStatus(res.data);
          } else {
            setStartRefusal(res.error ?? "system_audio_unavailable");
          }
        });
      }}
      onOpenSystemSettings={() => {
        void commands.openSystemAudioPrivacySettings();
      }}
      onDismissStartRefusal={() => {
        upcomingConsentRef.current = false;
        pendingUpcomingRef.current = null;
        setStartRefusal(null);
        void commands.cancelMeetingConsent();
      }}
      onCancelConsent={() => {
        upcomingConsentRef.current = false;
        pendingUpcomingRef.current = null;
        setConsentOpen(false);
        setStartRefusal(null);
        void commands.cancelMeetingConsent();
      }}
      onDismissOffer={(bundleId) => {
        setOffer(null);
        void commands.dismissMeetingOffer(bundleId);
      }}
      onStopMeeting={() => {
        flushNotepad();
        setKeepFinishedSession(true);
        void commands.stopMeeting().then((res) => {
          if (res.status === "ok") setStatus(res.data);
        });
      }}
      onPauseMeeting={() => {
        void commands.pauseMeeting().then((res) => {
          if (res.status === "ok") setStatus(res.data);
        });
      }}
      onResumeMeeting={() => {
        void commands.resumeMeeting().then((res) => {
          if (res.status === "ok") setStatus(res.data);
        });
      }}
      onAppendAudio={() => {
        if (status.meeting_id !== null) {
          void commands.appendMeetingAudio(status.meeting_id).then((res) => {
            if (res.status === "ok") setStatus(res.data);
          });
        }
      }}
      catchUpLines={catchUpLines}
      catchUpOpen={catchUpOpen}
      catchUpMinutes={catchUpMinutes}
      catchUpNotEnoughSaid={catchUpNotEnoughSaid}
      onChangeCatchUpMinutes={(mins) => {
        setCatchUpMinutes(mins);
        if (catchUpOpen && status.meeting_id !== null) {
          void commands.catchUpMeeting(status.meeting_id, mins).then((res) => {
            if (res.status === "ok") {
              setCatchUpLines(res.data);
              setCatchUpNotEnoughSaid(res.data.length === 0);
            }
          });
        }
      }}
      onRequestCatchUp={(mins) => {
        const chosen = mins ?? catchUpMinutes;
        setCatchUpOpen(true);
        if (status.meeting_id !== null) {
          void commands
            .catchUpMeeting(status.meeting_id, chosen)
            .then((res) => {
              if (res.status === "ok") {
                setCatchUpLines(res.data);
                setCatchUpNotEnoughSaid(res.data.length === 0);
              }
            });
        }
      }}
      notesSnapshot={notesSnapshot}
      notepadDraft={notepadDraft}
      retryingNotes={retryingNotes}
      keepFinishedSession={keepFinishedSession}
      onNotepadChange={handleNotepadChange}
      onRetryNotes={handleRetryNotes}
      onAddMeetingWord={(w) => {
        persistWords(normalizeMeetingWordList(meetingWords, w));
      }}
      onRemoveMeetingWord={(w) => {
        persistWords(meetingWords.filter((item) => item !== w));
      }}
    />
  );
};
