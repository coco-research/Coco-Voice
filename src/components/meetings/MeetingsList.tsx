import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import {
  commands as meetingCommands,
  type MeetingNotesSnapshot,
  type MeetingSegmentView,
  type MeetingStatus,
} from "../../bindings.meetings";
import {
  formatMeetingParticipantsLine,
  typedSpeakerNamesFromMap,
} from "./channelLabels";
import {
  LiveTranscriptSection,
  resolveSelectedSpanBounds,
  segmentKey,
} from "./LiveTranscriptSection";
import {
  buildSpeakerNameMapFromSlots,
  useSpeakerSplitSection,
} from "./gate";
import {
  AddAudioControl,
  OlderNoteWhileRecordingBanner,
} from "./MeetingHistoryExtras";
import { EditableMeetingTitle } from "./MeetingEntryPanels";
import { MeetingNotesSection } from "./MeetingNotesPanel";
import { AskMeetingsView } from "./AskMeetingsView";
import { NOTEPAD_SAVE_DEBOUNCE_MS } from "./RecordMeetingPage";
import { formatShortDateDddDDMon } from "./UpcomingMeetings";
import {
  MEETING_EXTRAS_READY_EVENT,
  MEETING_NOTES_FAILED_EVENT,
  MEETING_NOTES_PROGRESS_EVENT,
  MEETING_NOTES_READY_EVENT,
  MEETING_SPEAKERS_READY_EVENT,
  type ExtrasReadyEvent,
  type NotesFailedEvent,
  type NotesProgressEvent,
  type NotesReadyEvent,
  type SpeakersReadyEvent,
} from "./meetingEvents";

export interface MeetingListItem {
  id: number;
  title: string;
  started_at: number;
  ended_at: number | null;
  notes_ready: boolean;
  attendees?: string[];
  notes_finishing?: boolean;
  system_audio_silent?: boolean;
}

interface MeetingDeleteResult {
  audio_left: boolean;
}

export const SEARCH_DEBOUNCE_MS = 200;

export const OUTLINED_BTN_SM =
  "inline-flex min-h-[30px] items-center justify-center rounded-[10px] border border-text/75 bg-transparent px-2.5 text-[13px] font-semibold text-text transition-colors cursor-pointer hover:bg-text hover:text-background disabled:opacity-50 disabled:cursor-not-allowed";

export const NOTES_READY_TONE = "text-emerald-700 dark:text-emerald-400";
const FINISHING_NOTES_TONE = "text-logo-primary";
const TRANSCRIPT_ONLY_TONE = "text-mid-gray";
const HALF_CIRCLE = "◐";
const BACK_ARROW = "←";
const KIND_NOTES = "notes-ready";
const KIND_FINISHING = "finishing-notes";
const KIND_TRANSCRIPT = "transcript-only";

export function defaultMarkdownFilename(title: string): string {
  const slug = title
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${slug || "meeting"}.md`;
}

export function formatDurationMinutes(
  startedAt: number,
  endedAt: number | null,
): number | null {
  if (endedAt === null || endedAt < startedAt) return null;
  return Math.max(1, Math.round((endedAt - startedAt) / 60));
}

function StatusDot({ filled }: { filled: boolean }) {
  return (
    <svg
      viewBox="0 0 10 10"
      className="h-2.5 w-2.5 shrink-0"
      aria-hidden="true"
    >
      <circle
        cx="5"
        cy="5"
        r="4"
        fill={filled ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth="1.5"
      />
    </svg>
  );
}

export interface MeetingsListSectionProps {
  items: MeetingListItem[];
  finishingIds?: number[];
  query: string;
  loading: boolean;
  confirmingId: number | null;
  busyDeleteId: number | null;
  audioLeftWarning: boolean;
  errorKey: string | null;
  onQueryChange: (next: string) => void;
  onExport: (item: MeetingListItem) => void;
  onRequestDelete: (id: number) => void;
  onCancelDelete: () => void;
  onConfirmDelete: (id: number) => void;
  onSelectMeeting?: (item: MeetingListItem) => void;
}

export function MeetingsListSection({
  items,
  finishingIds = [],
  query,
  loading,
  confirmingId,
  busyDeleteId,
  audioLeftWarning,
  errorKey,
  onQueryChange,
  onExport,
  onRequestDelete,
  onCancelDelete,
  onConfirmDelete,
  onSelectMeeting,
}: MeetingsListSectionProps) {
  const { t, i18n } = useTranslation();
  const hasQuery = query.trim().length > 0;
  const countLabel =
    items.length === 1
      ? t("meetings.list.countOne")
      : t("meetings.list.countMany", { count: items.length });
  const emptyLabel = hasQuery
    ? t("meetings.list.emptySearch")
    : t("meetings.list.emptyList");

  return (
    <div
      data-testid="meeting-history-section"
      className="max-w-3xl w-full mx-auto"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("meetings.sidebar")}
      </p>
      <header className="mt-1 flex items-baseline justify-between gap-3">
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("meetings.list.heading")}
        </h2>
        <span
          data-testid="meeting-count"
          className="text-xs font-medium text-mid-gray"
        >
          {countLabel}
        </span>
      </header>

      <label className="mt-3 block">
        <span className="block text-xs font-medium text-mid-gray">
          {t("meetings.list.searchLabel")}
        </span>
        <input
          type="search"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={t("meetings.list.searchPlaceholder")}
          aria-label={t("meetings.list.searchAria")}
          className="mt-1 w-full rounded-[10px] border border-mid-gray/40 bg-background px-3 py-1.5 text-sm text-text focus:outline-none focus:ring-2 focus:ring-logo-primary"
        />
      </label>

      {audioLeftWarning && (
        <p
          role="status"
          className="mt-3 rounded-[10px] border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-text"
        >
          {t("meetings.list.audioLeftWarning")}
        </p>
      )}

      {errorKey !== null && (
        <p role="alert" className="mt-3 text-sm text-red-600">
          {t(errorKey)}
        </p>
      )}

      {!loading && items.length === 0 ? (
        <p className="mt-5 text-sm text-mid-gray">{emptyLabel}</p>
      ) : (
        <ul className="mt-3 divide-y divide-mid-gray/20 border-b-[1.5px] border-text/80">
          {items.map((item) => {
            const title = item.title.trim() || t("meetings.list.untitled");
            const dateLabel = formatShortDateDddDDMon(
              item.started_at,
              i18n.language,
            );
            const mins = formatDurationMinutes(item.started_at, item.ended_at);
            const fullMeta =
              mins !== null
                ? t("meetings.list.dateAndDuration", {
                    date: dateLabel,
                    minutes: mins,
                  })
                : dateLabel;
            const suffix = fullMeta.startsWith(dateLabel)
              ? fullMeta.slice(dateLabel.length)
              : "";
            const isFinishing =
              Boolean(item.notes_finishing) || finishingIds.includes(item.id);
            const statusText = isFinishing
              ? t("meetings.list.finishingNotes", "Finishing notes")
              : item.notes_ready
                ? t("meetings.list.notesReady")
                : t("meetings.list.transcriptOnly");
            const statusTone = isFinishing
              ? FINISHING_NOTES_TONE
              : item.notes_ready
                ? NOTES_READY_TONE
                : TRANSCRIPT_ONLY_TONE;
            const statusKind = isFinishing
              ? KIND_FINISHING
              : item.notes_ready
                ? KIND_NOTES
                : KIND_TRANSCRIPT;
            const confirming = confirmingId === item.id;

            return (
              <li
                key={item.id}
                className="flex items-center justify-between gap-2 py-2.5"
              >
                <div
                  className="min-w-0 flex-1 cursor-pointer"
                  onClick={() => onSelectMeeting?.(item)}
                >
                  <p className="truncate text-[14px] font-semibold text-text hover:text-logo-primary">
                    {title}
                  </p>
                  <p className="mt-0.5 truncate text-[12.5px] text-mid-gray">
                    <span className="font-mono text-[12px] font-semibold text-text">
                      {dateLabel}
                    </span>
                    {suffix}
                  </p>
                </div>

                <div className="flex shrink-0 flex-nowrap items-center gap-1.5 whitespace-nowrap">
                  <span
                    data-status-kind={statusKind}
                    className={`inline-flex shrink-0 items-center gap-1 text-[12px] font-semibold whitespace-nowrap ${statusTone}`}
                  >
                    {isFinishing ? (
                      <span aria-hidden="true">{HALF_CIRCLE}</span>
                    ) : (
                      <StatusDot filled={item.notes_ready} />
                    )}
                    <span>{statusText}</span>
                  </span>

                  <button
                    type="button"
                    className={`${OUTLINED_BTN_SM} shrink-0 whitespace-nowrap px-2 text-[12px]`}
                    onClick={() => onExport(item)}
                  >
                    {t("meetings.list.exportMarkdown")}
                  </button>

                  {confirming ? (
                    <div className="flex shrink-0 items-center gap-1.5">
                      <span className="text-xs text-mid-gray">
                        {t("meetings.list.confirmPrompt")}
                      </span>
                      <button
                        type="button"
                        disabled={busyDeleteId === item.id}
                        onClick={() => onConfirmDelete(item.id)}
                        className="inline-flex min-h-[30px] items-center justify-center rounded-[10px] border border-red-600 bg-red-600 px-2 text-[12px] font-semibold text-white cursor-pointer disabled:opacity-50"
                      >
                        {t("meetings.list.confirmDelete")}
                      </button>
                      <button
                        type="button"
                        disabled={busyDeleteId === item.id}
                        onClick={onCancelDelete}
                        className={`${OUTLINED_BTN_SM} px-2 text-[12px]`}
                      >
                        {t("meetings.list.cancelDelete")}
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      className={`${OUTLINED_BTN_SM} shrink-0 whitespace-nowrap px-2 text-[12px]`}
                      onClick={() => onRequestDelete(item.id)}
                    >
                      {t("meetings.list.delete")}
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

export function MeetingsList({
  initialItems,
}: {
  initialItems?: MeetingListItem[];
}) {
  const { t, i18n } = useTranslation();
  const SpeakerSplitSection = useSpeakerSplitSection();
  const [items, setItems] = useState<MeetingListItem[]>(initialItems ?? []);
  const [finishingIds, setFinishingIds] = useState<number[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(!initialItems);
  const [confirmingId, setConfirmingId] = useState<number | null>(null);
  const [busyDeleteId, setBusyDeleteId] = useState<number | null>(null);
  const [audioLeftWarning, setAudioLeftWarning] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const requestSeq = useRef(0);
  const [selectedMeeting, setSelectedMeeting] =
    useState<MeetingListItem | null>(null);
  const [detailNotes, setDetailNotes] = useState<MeetingNotesSnapshot | null>(
    null,
  );
  const [detailNotepad, setDetailNotepad] = useState("");
  const [detailSegments, setDetailSegments] = useState<MeetingSegmentView[]>(
    [],
  );
  const [transcriptQuery, setTranscriptQuery] = useState("");
  const [matchCursor, setMatchCursor] = useState(0);
  const [selectedSpanKeys, setSelectedSpanKeys] = useState<string[]>([]);
  const [lastClickedKey, setLastClickedKey] = useState<string | null>(null);
  const [confirmDeletePart, setConfirmDeletePart] = useState(false);
  const [speakerNames, setSpeakerNames] = useState<Record<string, string>>({});
  const [liveStatus, setLiveStatus] = useState<MeetingStatus | null>(null);
  const [jumpTarget, setJumpTarget] = useState<{
    startMs: number;
    seq: number;
  } | null>(null);
  const jumpSeqRef = useRef(0);
  const selectedIdRef = useRef<number | null>(null);
  const notepadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingNotepadRef = useRef<{ meetingId: number; text: string } | null>(
    null,
  );

  const flushDetailNotepad = useCallback(() => {
    if (notepadTimerRef.current !== null) {
      clearTimeout(notepadTimerRef.current);
      notepadTimerRef.current = null;
    }
    const pending = pendingNotepadRef.current;
    pendingNotepadRef.current = null;
    if (pending) {
      void meetingCommands.saveMeetingNotepad(pending.meetingId, pending.text);
    }
  }, []);

  useEffect(() => {
    selectedIdRef.current = selectedMeeting?.id ?? null;
  }, [selectedMeeting]);

  const openMeetingDetail = async (item: MeetingListItem) => {
    flushDetailNotepad();
    const switchingMeeting = selectedIdRef.current !== item.id;
    selectedIdRef.current = item.id;
    setSelectedMeeting(item);
    setDetailNotes(null);
    setDetailNotepad("");
    setDetailSegments([]);
    setSelectedSpanKeys([]);
    setLastClickedKey(null);
    setConfirmDeletePart(false);
    if (switchingMeeting) {
      setSpeakerNames({});
    }
    setTranscriptQuery("");
    setMatchCursor(0);
    const [st, snapRes, segRes, speakersRes] = await Promise.all([
      meetingCommands.meetingStatus(),
      meetingCommands.meetingNotesSnapshot(item.id),
      meetingCommands.meetingSegments(item.id),
      meetingCommands.listMeetingSpeakers(item.id),
    ]);
    if (selectedIdRef.current !== item.id) return;
    setLiveStatus(st);
    if (snapRes.status === "ok") {
      setDetailNotes(snapRes.data);
      setDetailNotepad(snapRes.data.notepad_text ?? "");
    }
    if (segRes.status === "ok") {
      setDetailSegments(segRes.data);
    }
    if (speakersRes.status === "ok") {
      setSpeakerNames(buildSpeakerNameMapFromSlots(speakersRes.data));
    }
  };

  const loadList = useCallback(async (searchText: string) => {
    const seq = ++requestSeq.current;
    const trimmed = searchText.trim();
    try {
      const data = trimmed
        ? await invoke<MeetingListItem[]>("search_meetings", { query: trimmed })
        : await invoke<MeetingListItem[]>("list_meetings");
      if (seq !== requestSeq.current) return;
      setItems(data);
      setErrorKey(null);
    } catch {
      if (seq !== requestSeq.current) return;
      setErrorKey("meetings.list.loadFailed");
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (initialItems && query.trim().length === 0) return;
    const delay = query.trim().length > 0 ? SEARCH_DEBOUNCE_MS : 0;
    const timer = window.setTimeout(() => {
      void loadList(query);
    }, delay);
    return () => window.clearTimeout(timer);
  }, [initialItems, query, loadList]);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    void listen<NotesProgressEvent>(MEETING_NOTES_PROGRESS_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      const mid = ev.payload.meeting_id;
      setFinishingIds((prev) => (prev.includes(mid) ? prev : [...prev, mid]));
      if (selectedIdRef.current === mid) {
        setDetailNotes((prev) => ({
          meeting_id: mid,
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
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<NotesReadyEvent>(MEETING_NOTES_READY_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      const mid = ev.payload.meeting_id;
      setFinishingIds((prev) => prev.filter((id) => id !== mid));
      setItems((prev) =>
        prev.map((row) =>
          row.id === mid
            ? {
                ...row,
                notes_ready: true,
                notes_finishing: false,
                title: ev.payload.title || row.title,
              }
            : row,
        ),
      );
      if (selectedIdRef.current === mid) {
        setSelectedMeeting((prev) =>
          prev
            ? {
                ...prev,
                notes_ready: true,
                title: ev.payload.title || prev.title,
              }
            : prev,
        );
        void meetingCommands.meetingNotesSnapshot(mid).then((res) => {
          if (!cancelled && res.status === "ok") setDetailNotes(res.data);
        });
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<NotesFailedEvent>(MEETING_NOTES_FAILED_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      const mid = ev.payload.meeting_id;
      setFinishingIds((prev) => prev.filter((id) => id !== mid));
      if (selectedIdRef.current === mid) {
        void meetingCommands.meetingNotesSnapshot(mid).then((res) => {
          if (!cancelled && res.status === "ok") setDetailNotes(res.data);
        });
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<SpeakersReadyEvent>(MEETING_SPEAKERS_READY_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      const mid = ev.payload.meeting_id;
      if (selectedIdRef.current === mid) {
        void meetingCommands.meetingSegments(mid).then((res) => {
          if (!cancelled && res.status === "ok") setDetailSegments(res.data);
        });
        void meetingCommands.meetingNotesSnapshot(mid).then((res) => {
          if (!cancelled && res.status === "ok") setDetailNotes(res.data);
        });
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    void listen<ExtrasReadyEvent>(MEETING_EXTRAS_READY_EVENT, (ev) => {
      if (cancelled || !ev.payload) return;
      const mid = ev.payload.meeting_id;
      if (selectedIdRef.current === mid) {
        void meetingCommands.meetingNotesSnapshot(mid).then((res) => {
          if (!cancelled && res.status === "ok") setDetailNotes(res.data);
        });
      }
    })
      .then((u) => unlisteners.push(u))
      .catch(() => {});

    return () => {
      cancelled = true;
      flushDetailNotepad();
      for (const u of unlisteners) u();
    };
  }, [flushDetailNotepad]);

  const handleExport = async (item: MeetingListItem) => {
    setErrorKey(null);
    try {
      const dest = await save({
        defaultPath: defaultMarkdownFilename(item.title),
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!dest) return;
      await invoke("export_meeting_markdown", { id: item.id, destPath: dest });
    } catch {
      setErrorKey("meetings.list.exportFailed");
    }
  };

  const handleConfirmDelete = async (id: number) => {
    setBusyDeleteId(id);
    setErrorKey(null);
    try {
      const res = await meetingCommands.trashMeeting(id);
      if (res.status === "error") {
        // Fallback for older test harness mocking delete_meeting directly.
        const legacy = await invoke<MeetingDeleteResult>("delete_meeting", {
          id,
        });
        setAudioLeftWarning(legacy.audio_left);
      }
      setItems((prev) => prev.filter((row) => row.id !== id));
      setConfirmingId(null);
    } catch {
      setErrorKey("meetings.list.deleteFailed");
    } finally {
      setBusyDeleteId(null);
    }
  };

  const handleRenameSelected = async (nextTitle: string) => {
    if (!selectedMeeting) return;
    const mid = selectedMeeting.id;
    setSelectedMeeting((prev) => (prev ? { ...prev, title: nextTitle } : prev));
    setItems((prev) =>
      prev.map((row) => (row.id === mid ? { ...row, title: nextTitle } : row)),
    );
    const res = await meetingCommands.renameMeeting(mid, nextTitle);
    if (res.status === "ok") {
      setSelectedMeeting((prev) =>
        prev ? { ...prev, title: res.data } : prev,
      );
      setItems((prev) =>
        prev.map((row) => (row.id === mid ? { ...row, title: res.data } : row)),
      );
    }
  };

  if (selectedMeeting) {
    const nonEmptyDetailSegments = detailSegments.filter(
      (s) => s.text.trim().length > 0,
    );
    const hasSpoken = nonEmptyDetailSegments.length > 0;
    const trimmedQuery = transcriptQuery.trim().toLowerCase();
    const matchingSegments = trimmedQuery
      ? nonEmptyDetailSegments.filter((s) =>
          s.text.toLowerCase().includes(trimmedQuery),
        )
      : [];
    const safeMatchIndex =
      matchingSegments.length > 0 ? matchCursor % matchingSegments.length : 0;

    // W28: select strictly by segment identity (never filtered or unfiltered list index).
    const handleSelectSegment = (
      seg: MeetingSegmentView,
      shiftKey: boolean,
    ) => {
      setConfirmDeletePart(false);
      const key = segmentKey(seg);
      if (shiftKey && lastClickedKey !== null) {
        const keys = nonEmptyDetailSegments.map(segmentKey);
        const a = keys.indexOf(lastClickedKey);
        const b = keys.indexOf(key);
        if (a >= 0 && b >= 0) {
          const start = Math.min(a, b);
          const end = Math.max(a, b);
          setSelectedSpanKeys(keys.slice(start, end + 1));
          return;
        }
      }
      setLastClickedKey(key);
      setSelectedSpanKeys((prev) =>
        prev.includes(key) ? prev.filter((k) => k !== key) : [key],
      );
    };

    const handleDeleteSelectedPart = async () => {
      const bounds = resolveSelectedSpanBounds(
        detailSegments,
        selectedSpanKeys,
      );
      if (!bounds) return;
      const { startMs, endMs } = bounds;
      const deletedKeys = new Set(bounds.picked.map(segmentKey));
      setConfirmDeletePart(false);
      setSelectedSpanKeys([]);
      setLastClickedKey(null);
      setDetailSegments((prev) =>
        prev.filter(
          (s) =>
            !deletedKeys.has(segmentKey(s)) &&
            !(s.end_ms > startMs && s.start_ms < endMs),
        ),
      );
      await meetingCommands.deleteTranscriptSpanSecure(
        selectedMeeting.id,
        startMs,
        endMs,
      );
      const snapRes = await meetingCommands.meetingNotesSnapshot(
        selectedMeeting.id,
      );
      if (snapRes.status === "ok") setDetailNotes(snapRes.data);
    };

    const detailDateLabel = formatShortDateDddDDMon(
      selectedMeeting.started_at,
      i18n.language,
    );
    const detailMins = formatDurationMinutes(
      selectedMeeting.started_at,
      selectedMeeting.ended_at,
    );
    const detailMeta =
      detailMins !== null
        ? t("meetings.list.dateAndDuration", {
            date: detailDateLabel,
            minutes: detailMins,
          })
        : detailDateLabel;
    const isDetailFinishing =
      detailNotes?.state === "running" ||
      Boolean(selectedMeeting.notes_finishing) ||
      finishingIds.includes(selectedMeeting.id);
    const isDetailReady =
      !isDetailFinishing &&
      (detailNotes?.state === "ready" || selectedMeeting.notes_ready);
    const detailStatusText = isDetailFinishing
      ? t("meetings.list.finishingNotes", "Finishing notes")
      : isDetailReady
        ? t("meetings.list.notesReady")
        : t("meetings.list.transcriptOnly");
    const detailStatusTone = isDetailFinishing
      ? FINISHING_NOTES_TONE
      : isDetailReady
        ? NOTES_READY_TONE
        : TRANSCRIPT_ONLY_TONE;

    const isLiveActive = Boolean(
      liveStatus &&
        (liveStatus.state === "recording" || liveStatus.state === "paused"),
    );
    const liveBannerTitle =
      liveStatus?.title?.trim() ||
      t("meetings.storage.activeCall", "Active call");
    const detailParticipantsLine = formatMeetingParticipantsLine(
      selectedMeeting.attendees,
      typedSpeakerNamesFromMap(speakerNames),
      t,
    );

    return (
      <div
        data-testid="meeting-detail-view"
        className="max-w-3xl w-full mx-auto space-y-3"
      >
        {isLiveActive && liveStatus ? (
          <OlderNoteWhileRecordingBanner
            liveTitle={liveBannerTitle}
            elapsedSecs={liveStatus.elapsed_secs}
            onBackToLive={() => {
              flushDetailNotepad();
              setSelectedMeeting(null);
              window.dispatchEvent(
                new CustomEvent("coco-voice:navigate-live-meeting"),
              );
            }}
          />
        ) : null}
        <div className="pb-2 border-b border-mid-gray/15 space-y-2">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <button
              type="button"
              onClick={() => {
                flushDetailNotepad();
                setSelectedMeeting(null);
              }}
              className={`${OUTLINED_BTN_SM} shrink-0 whitespace-nowrap`}
            >
              <span aria-hidden="true" className="mr-1">
                {BACK_ARROW}
              </span>
              <span>{t("meetings.list.backToAll", "All meetings")}</span>
            </button>
            <div className="flex flex-wrap items-center gap-1.5">
              <input
                type="search"
                data-testid="meeting-transcript-search"
                value={transcriptQuery}
                onChange={(e) => {
                  setTranscriptQuery(e.target.value);
                  setMatchCursor(0);
                }}
                placeholder={t(
                  "meetings.storage.searchTranscriptPlaceholder",
                  "Search this transcript",
                )}
                aria-label={t(
                  "meetings.storage.searchTranscriptPlaceholder",
                  "Search this transcript",
                )}
                className="w-48 sm:w-56 rounded-[9px] border border-mid-gray/35 bg-background px-2.5 py-1 text-[12.5px] text-text"
              />
              {matchingSegments.length > 0 ? (
                <div className="inline-flex items-center gap-1 text-[12px]">
                  <span className="font-mono text-mid-gray whitespace-nowrap">
                    {t("meetings.storage.searchMatchCounter", {
                      defaultValue: "{{current}} of {{total}}",
                      current: safeMatchIndex + 1,
                      total: matchingSegments.length,
                    })}
                  </span>
                  <button
                    type="button"
                    aria-label={t("meetings.storage.prevMatch", "Previous")}
                    onClick={() => {
                      const prev =
                        (safeMatchIndex - 1 + matchingSegments.length) %
                        matchingSegments.length;
                      setMatchCursor(prev);
                      jumpSeqRef.current += 1;
                      setJumpTarget({
                        startMs: matchingSegments[prev].start_ms,
                        seq: jumpSeqRef.current,
                      });
                    }}
                    className="rounded-[6px] border border-mid-gray/40 px-1.5 py-0.5 text-[11.5px] font-semibold text-text cursor-pointer"
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    aria-label={t("meetings.storage.nextMatch", "Next")}
                    onClick={() => {
                      const next =
                        (safeMatchIndex + 1) % matchingSegments.length;
                      setMatchCursor(next);
                      jumpSeqRef.current += 1;
                      setJumpTarget({
                        startMs: matchingSegments[next].start_ms,
                        seq: jumpSeqRef.current,
                      });
                    }}
                    className="rounded-[6px] border border-mid-gray/40 px-1.5 py-0.5 text-[11.5px] font-semibold text-text cursor-pointer"
                  >
                    ↓
                  </button>
                </div>
              ) : null}
            </div>
          </div>
          <header className="space-y-0.5">
            <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
              {t("meetings.sidebar")}
            </p>
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <EditableMeetingTitle
                title={selectedMeeting.title}
                onRename={handleRenameSelected}
              />
              <span
                data-testid="meeting-detail-status"
                className={`inline-flex items-center gap-1.5 text-[12.5px] font-semibold ${detailStatusTone}`}
              >
                {isDetailFinishing ? (
                  <span aria-hidden="true">{HALF_CIRCLE}</span>
                ) : (
                  <StatusDot filled={isDetailReady} />
                )}
                <span>{detailStatusText}</span>
              </span>
            </div>
            <div className="flex flex-wrap items-center gap-2 text-[12.5px] text-mid-gray">
              <span data-testid="meeting-detail-meta">{detailMeta}</span>
              <span aria-hidden="true">·</span>
              <span data-testid="meeting-detail-participants">
                {detailParticipantsLine}
              </span>
            </div>
          </header>
        </div>
        <MeetingNotesSection
          recording={false}
          snapshot={detailNotes}
          notepadDraft={detailNotepad}
          segments={detailSegments}
          hasSpokenSegments={hasSpoken}
          onNotepadChange={(val) => {
            setDetailNotepad(val);
            pendingNotepadRef.current = {
              meetingId: selectedMeeting.id,
              text: val,
            };
            if (notepadTimerRef.current !== null) {
              clearTimeout(notepadTimerRef.current);
            }
            notepadTimerRef.current = setTimeout(() => {
              flushDetailNotepad();
            }, NOTEPAD_SAVE_DEBOUNCE_MS);
          }}
          onJumpToSegment={(startMs) => {
            jumpSeqRef.current += 1;
            setJumpTarget({ startMs, seq: jumpSeqRef.current });
          }}
          onRetry={() => {
            const mid = selectedMeeting.id;
            setFinishingIds((prev) =>
              prev.includes(mid) ? prev : [...prev, mid],
            );
            setDetailNotes((prev) => ({
              meeting_id: mid,
              state: "running",
              progress_label: prev?.progress_label ?? null,
              progress_step: 1,
              progress_total: 1,
              elapsed_secs: 0,
              estimate_secs: null,
              time_to_notes_secs: null,
              failure_reason: null,
              notepad_text: detailNotepad,
              lines: prev?.lines ?? [],
              oversized_count: prev?.oversized_count ?? 0,
            }));
            void meetingCommands.retryMeetingNotes(mid).then((r) => {
              if (r.status === "ok") setDetailNotes(r.data);
            });
          }}
        />
        {isLiveActive ? (
          <p
            data-testid="older-note-live-hint"
            className="text-[12px] text-mid-gray"
          >
            {t("meetings.storage.olderNoteHint", { title: liveBannerTitle })}
          </p>
        ) : null}
        <AddAudioControl
          onConfirmAppendAudio={() => {
            void meetingCommands
              .requestAppendMeetingConsent(selectedMeeting.id)
              .then((res) => {
                if (res.status === "ok") {
                  return meetingCommands.confirmAppendMeetingAudio(
                    selectedMeeting.id,
                    res.data,
                  );
                }
                return meetingCommands.appendMeetingAudio(selectedMeeting.id);
              });
          }}
        />
        <AskMeetingsView
          meetingId={selectedMeeting.id}
          initialMeetingTitle={selectedMeeting.title}
          onJumpToCitation={(citation) => {
            jumpSeqRef.current += 1;
            setJumpTarget({
              startMs: citation.start_ms,
              seq: jumpSeqRef.current,
            });
          }}
        />
        {SpeakerSplitSection ? (
          <SpeakerSplitSection
            meetingId={selectedMeeting.id}
            segments={detailSegments}
            onSlotsLoaded={(loaded) => {
              setSpeakerNames(buildSpeakerNameMapFromSlots(loaded));
            }}
            onRenamed={(updated) => {
              setSpeakerNames(buildSpeakerNameMapFromSlots(updated));
              void openMeetingDetail(selectedMeeting);
            }}
          />
        ) : null}
        <LiveTranscriptSection
            status={{
              state: "idle",
              elapsed_secs: Math.max(
                0,
                (selectedMeeting.ended_at ?? selectedMeeting.started_at) -
                  selectedMeeting.started_at,
              ),
              attendees: selectedMeeting.attendees ?? [],
              meeting_id: selectedMeeting.id,
              mic_name: null,
              warnings: selectedMeeting.system_audio_silent
                ? ["system_audio_silent"]
                : [],
            }}
            segments={detailSegments}
            showSpeakerLabels={Boolean(SpeakerSplitSection)}
            speakerNames={speakerNames}
            jumpTarget={jumpTarget}
            onStop={() => {}}
            searchQuery={transcriptQuery}
            selectedKeys={selectedSpanKeys}
            onSelectSegment={handleSelectSegment}
            childrenBelowHeader={
              selectedSpanKeys.length > 0 || confirmDeletePart ? (
                <div className="space-y-2">
                  {selectedSpanKeys.length > 0 && !confirmDeletePart ? (
                    <div className="flex flex-wrap items-center justify-end gap-2 text-[12px]">
                      <button
                        type="button"
                        data-testid="delete-this-part-btn"
                        onClick={() => setConfirmDeletePart(true)}
                        className="rounded-[8px] border border-red-500/45 px-2.5 py-1 text-[12px] font-semibold text-red-600 cursor-pointer hover:bg-red-500/10"
                      >
                        {t(
                          "meetings.storage.deleteThisPart",
                          "Delete this part",
                        )}
                      </button>
                    </div>
                  ) : null}

                  {confirmDeletePart ? (
                    <div
                      role="alertdialog"
                      data-testid="confirm-delete-part"
                      className="flex flex-wrap items-center justify-between gap-2 rounded-[10px] border border-red-500/40 bg-red-500/10 p-3 text-[12.5px]"
                    >
                      <p className="font-medium text-text">
                        {t(
                          "meetings.storage.deletePartConfirm",
                          "Delete these lines from the transcript, the audio and the notes? This can't be undone.",
                        )}
                      </p>
                      <div className="flex items-center gap-1.5">
                        <button
                          type="button"
                          onClick={() => void handleDeleteSelectedPart()}
                          className="rounded-[8px] border border-red-600 bg-red-600 px-3 py-1 text-[12px] font-semibold text-white cursor-pointer"
                        >
                          {t("meetings.list.delete")}
                        </button>
                        <button
                          type="button"
                          onClick={() => setConfirmDeletePart(false)}
                          className="rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] text-text cursor-pointer"
                        >
                          {t("meetings.consent.cancel")}
                        </button>
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : null
            }
          />
      </div>
    );
  }

  return (
    <MeetingsListSection
      onSelectMeeting={(item) => void openMeetingDetail(item)}
      items={items}
      finishingIds={finishingIds}
      query={query}
      loading={loading}
      confirmingId={confirmingId}
      busyDeleteId={busyDeleteId}
      audioLeftWarning={audioLeftWarning}
      errorKey={errorKey}
      onQueryChange={setQuery}
      onExport={(item) => void handleExport(item)}
      onRequestDelete={(id) => {
        setAudioLeftWarning(false);
        setConfirmingId(id);
      }}
      onCancelDelete={() => setConfirmingId(null)}
      onConfirmDelete={(id) => void handleConfirmDelete(id)}
    />
  );
}
