import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  MeetingSegmentView,
  MeetingStatus,
  MeetingWarning,
} from "../../bindings.meetings";
import {
  formatMeetingParticipantsLine,
  segmentChannelLabel,
  typedSpeakerNamesFromMap,
} from "./channelLabels";
import { formatMeetingClock, MeetingFloatingPill } from "./MeetingEntryPanels";
import { scrollTranscriptToSegment } from "./MeetingNotesPanel";

const HALF_CIRCLE = "◐";
const GENERIC_SPEAKER_SLOT_RE = /^Speaker\s+\d+$/i;
const SCROLL_BOTTOM_TOLERANCE_PX = 24;
export const TRANSCRIPT_HIGHLIGHT_MS = 2_000;

export function isNearTranscriptBottom(metrics: {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}): boolean {
  return (
    metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight <=
    SCROLL_BOTTOM_TOLERANCE_PX
  );
}

export function formatSegmentOffset(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const mins = String(Math.floor(total / 60)).padStart(2, "0");
  const secs = String(total % 60).padStart(2, "0");
  return `${mins}:${secs}`;
}

export function segmentKey(
  seg: Pick<MeetingSegmentView, "id" | "start_ms" | "end_ms">,
): string {
  return seg.id != null ? String(seg.id) : `${seg.start_ms}:${seg.end_ms}`;
}

export function resolveSelectedSpanBounds(
  allSegments: MeetingSegmentView[],
  selectedKeys: string[],
): {
  picked: MeetingSegmentView[];
  startMs: number;
  endMs: number;
} | null {
  if (selectedKeys.length === 0) return null;
  const keySet = new Set(selectedKeys);
  const picked = allSegments.filter(
    (s) => s.text.trim().length > 0 && keySet.has(segmentKey(s)),
  );
  if (picked.length === 0) return null;
  return {
    picked,
    startMs: Math.min(...picked.map((s) => s.start_ms)),
    endMs: Math.max(...picked.map((s) => s.end_ms)),
  };
}

export function resolveTranscriptSpeakerLabel(
  seg: MeetingSegmentView,
  channelLabels: { me: string; others: string },
  showSpeakerLabels = false,
  speakerNames?: Record<string, string>,
): string {
  if (seg.channel === "me") {
    return channelLabels.me;
  }
  const raw = seg.speaker_label?.trim() ?? "";
  if (showSpeakerLabels && raw) {
    const mapped = speakerNames?.[raw]?.trim();
    if (mapped) return mapped;
    return raw;
  }
  if (raw && !GENERIC_SPEAKER_SLOT_RE.test(raw)) {
    return raw;
  }
  return channelLabels.others;
}

function renderHighlightedText(text: string, query: string): React.ReactNode {
  const q = query.trim();
  if (!q) return text;
  const lowerText = text.toLowerCase();
  const lowerQ = q.toLowerCase();
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  let idx = lowerText.indexOf(lowerQ, cursor);
  let k = 0;
  while (idx !== -1) {
    if (idx > cursor) {
      parts.push(text.slice(cursor, idx));
    }
    parts.push(
      <mark
        key={`m-${k++}`}
        className="rounded bg-amber-300/60 dark:bg-amber-400/35 px-0.5 text-text"
      >
        {text.slice(idx, idx + q.length)}
      </mark>,
    );
    cursor = idx + q.length;
    idx = lowerText.indexOf(lowerQ, cursor);
  }
  if (cursor < text.length) {
    parts.push(text.slice(cursor));
  }
  return parts.length > 0 ? parts : text;
}

function warningTranslationKey(warning: MeetingWarning): string {
  return warning === "system_audio_silent"
    ? "meetings.view.warnings.systemAudioSilent"
    : "meetings.view.warnings.micFellBackToDefault";
}

export interface LiveTranscriptSectionProps {
  status: MeetingStatus;
  segments: MeetingSegmentView[];
  following?: boolean;
  showSpeakerLabels?: boolean;
  speakerNames?: Record<string, string>;
  hideHeaderControls?: boolean;
  jumpTarget?: { startMs: number; seq: number } | null;
  dictationShortcut?: string | null;
  pushToTalk?: boolean;
  selectedKeys?: string[];
  onSelectSegment?: (seg: MeetingSegmentView, shiftKey: boolean) => void;
  childrenBelowHeader?: React.ReactNode;
  childrenAfterTranscript?: React.ReactNode;
  onStop: () => void;
  onPause?: () => void;
  onResume?: () => void;
  pauseLabel?: string;
  pausedBadgeLabel?: string;
  pausedHintLabel?: string;
  resumeLabel?: string;
  searchQuery?: string;
  searchPlaceholder?: string;
  onSearchChange?: (q: string) => void;
  deleteSpanLabel?: string;
  onDeleteSpan?: (seg: MeetingSegmentView) => void;
}

export const LiveTranscriptSection: React.FC<LiveTranscriptSectionProps> = ({
  status,
  segments,
  following = true,
  showSpeakerLabels = false,
  speakerNames,
  hideHeaderControls = false,
  jumpTarget = null,
  dictationShortcut = null,
  pushToTalk = true,
  selectedKeys = [],
  onSelectSegment,
  childrenBelowHeader,
  childrenAfterTranscript,
  onStop,
  onPause,
  onResume,
  pauseLabel,
  pausedBadgeLabel,
  pausedHintLabel,
  resumeLabel,
  searchQuery = "",
  searchPlaceholder,
  onSearchChange,
  deleteSpanLabel,
  onDeleteSpan,
}) => {
  const { t } = useTranslation();
  const listRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(following);
  const [highlightedStartMs, setHighlightedStartMs] = useState<number | null>(
    null,
  );
  const channelLabels = {
    me: t("meetings.view.channel.me"),
    others: t("meetings.view.channel.others"),
  };
  const isPaused = status.state === "paused";
  const isLiveRecording = status.state === "recording";
  const isActive = isLiveRecording || isPaused;
  const nonEmptySegments = segments.filter((s) => s.text.trim().length > 0);
  const qLower = searchQuery.trim().toLowerCase();
  // W28: search MUST highlight in place and NEVER filter rows out.
  const visibleSegments = nonEmptySegments;

  useEffect(() => {
    setAutoScroll(following);
  }, [following]);

  useEffect(() => {
    if (!autoScroll || !listRef.current) return;
    listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [autoScroll, visibleSegments.length]);

  useEffect(() => {
    if (!jumpTarget) return;
    scrollTranscriptToSegment(listRef.current, jumpTarget.startMs);
    setHighlightedStartMs(jumpTarget.startMs);
    const timer = window.setTimeout(() => {
      setHighlightedStartMs((cur) =>
        cur === jumpTarget.startMs ? null : cur,
      );
    }, TRANSCRIPT_HIGHLIGHT_MS);
    return () => window.clearTimeout(timer);
  }, [jumpTarget]);

  const handleScroll = () => {
    const el = listRef.current;
    if (!el) return;
    const atBottom = isNearTranscriptBottom({
      scrollTop: el.scrollTop,
      clientHeight: el.clientHeight,
      scrollHeight: el.scrollHeight,
    });
    setAutoScroll(atBottom);
  };

  const jumpToLatest = () => {
    setAutoScroll(true);
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  };

  const autoScrollChip = isLiveRecording ? (
    autoScroll ? (
      <span
        data-testid="transcript-auto-scroll"
        className="inline-flex items-center gap-1 text-[12px] text-mid-gray"
      >
        <span aria-hidden="true">{HALF_CIRCLE}</span>
        <span>{t("meetings.view.autoScrolling")}</span>
      </span>
    ) : (
      <button
        type="button"
        data-testid="transcript-jump-latest"
        onClick={jumpToLatest}
        className="inline-flex min-h-[26px] items-center rounded-full border border-logo-primary/45 bg-logo-primary/10 px-2.5 text-[12px] font-semibold text-logo-primary cursor-pointer hover:bg-logo-primary/20"
      >
        {t("meetings.view.jumpToLatest", "Jump to latest")}
      </button>
    )
  ) : null;

  const showChannelsOutsideCard = isActive && hideHeaderControls;
  const participantsLine = formatMeetingParticipantsLine(
    status.attendees,
    typedSpeakerNamesFromMap(speakerNames),
    t,
    Boolean(status.mic_only),
  );

  return (
    <div className="space-y-1.5">
      {showChannelsOutsideCard ? (
        <div
          data-testid="transcript-channels-outside"
          className="flex flex-wrap items-center justify-between gap-2 px-0.5"
        >
          <div>
            <h3 className="text-[14px] font-semibold text-mid-gray">
              {participantsLine}
            </h3>
            {status.mic_name ? (
              <p className="text-[12px] text-mid-gray">
                {t("meetings.view.mic", { name: status.mic_name })}
              </p>
            ) : null}
          </div>
          {autoScrollChip}
        </div>
      ) : null}
      <section
        data-testid="frame-4-live-transcript"
        className="space-y-3 rounded-[14px] border border-mid-gray/25 bg-background p-4"
      >
        {!showChannelsOutsideCard ? (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <h3 className="text-[15px] font-semibold text-text">
                {isActive
                  ? participantsLine
                  : t("meetings.notes.transcriptHeading", "Transcript")}
              </h3>
              {status.mic_name ? (
                <p className="text-[12px] text-mid-gray">
                  {t("meetings.view.mic", { name: status.mic_name })}
                </p>
              ) : null}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {!hideHeaderControls ? (
                <>
                  {isActive ? (
                    <MeetingFloatingPill
                      elapsedSecs={status.elapsed_secs}
                      paused={isPaused}
                      pausedLabel={pausedBadgeLabel}
                      resumeLabel={resumeLabel}
                      onResume={onResume}
                      onStop={onStop}
                    />
                  ) : (
                    <span className="text-[12.5px] font-semibold text-mid-gray">
                      {t("meetings.view.finished")}{" "}
                      {formatMeetingClock(status.elapsed_secs)}
                    </span>
                  )}
                  {status.state === "recording" && onPause && pauseLabel ? (
                    <button
                      type="button"
                      onClick={onPause}
                      className="inline-flex min-h-[28px] items-center rounded-[8px] border border-mid-gray/40 px-2.5 text-[12px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
                    >
                      {pauseLabel}
                    </button>
                  ) : null}
                </>
              ) : null}
              {autoScrollChip}
            </div>
          </div>
        ) : null}

      {!hideHeaderControls && isPaused && pausedHintLabel ? (
        <p
          role="status"
          className="rounded-[9px] border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 text-[12.5px] text-text"
        >
          {pausedHintLabel}
        </p>
      ) : null}

      {status.shortcut_failed ? (
        <p
          role="alert"
          data-testid="shortcut-failure-banner"
          className="rounded-[9px] border border-amber-500/35 bg-amber-500/10 px-3 py-1.5 text-[12.5px] text-text"
        >
          {t("meetings.view.shortcutFailed")}
        </p>
      ) : null}

      {status.warnings.map((w) => (
        <p
          key={w}
          role="status"
          className="rounded-[9px] border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 text-[12.5px] text-text"
        >
          {t(warningTranslationKey(w))}
        </p>
      ))}

      {childrenBelowHeader}

      {onSearchChange && searchPlaceholder ? (
        <input
          type="search"
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder={searchPlaceholder}
          aria-label={searchPlaceholder}
          className="w-full rounded-[9px] border border-mid-gray/35 bg-background px-3 py-1.5 text-[13px] text-text"
        />
      ) : null}

      <div
        ref={listRef}
        role="log"
        aria-live="polite"
        onScroll={handleScroll}
        className="max-h-72 overflow-y-auto rounded-[10px] border border-mid-gray/20 bg-mid-gray/[0.03]"
      >
        <ol className="divide-y divide-mid-gray/15">
          {visibleSegments.map((seg) => {
            const key = segmentKey(seg);
            const speaker = resolveTranscriptSpeakerLabel(
              seg,
              channelLabels,
              showSpeakerLabels,
              speakerNames,
            );
            const highlighted = highlightedStartMs === seg.start_ms;
            const searchMatch =
              qLower.length > 0 && seg.text.toLowerCase().includes(qLower);
            const selected = selectedKeys.includes(key);
            return (
              <li
                key={key}
                data-segment-key={key}
                data-start-ms={seg.start_ms}
                data-highlighted={highlighted ? "true" : undefined}
                data-search-match={searchMatch ? "true" : undefined}
                data-selected={selected ? "true" : undefined}
                onClick={
                  onSelectSegment
                    ? (e) => onSelectSegment(seg, e.shiftKey)
                    : undefined
                }
                className={`flex items-start gap-3 px-3 py-2 text-[13px] transition-colors ${
                  onSelectSegment ? "cursor-pointer hover:bg-mid-gray/10" : ""
                } ${
                  selected
                    ? "bg-logo-primary/12 ring-1 ring-inset ring-logo-primary/35"
                    : highlighted
                      ? "bg-logo-primary/15 ring-1 ring-logo-primary/40"
                      : searchMatch
                        ? "bg-amber-400/10"
                        : ""
                }`}
              >
                <span className="w-11 shrink-0 font-mono text-[12px] tabular-nums text-mid-gray">
                  {formatSegmentOffset(seg.start_ms)}
                </span>
                <span className="w-16 shrink-0 text-[12px] font-semibold text-text/80">
                  {speaker}
                </span>
                <span className="min-w-0 flex-1 text-text">
                  {searchMatch ? renderHighlightedText(seg.text, searchQuery) : seg.text}
                  {selected ? (
                    <span className="ml-1.5 text-[11.5px] font-semibold text-logo-primary">
                      {t("meetings.storage.selectedTag", "(selected)")}
                    </span>
                  ) : null}
                </span>
                {onDeleteSpan && deleteSpanLabel ? (
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      onDeleteSpan(seg);
                    }}
                    className="shrink-0 rounded-[6px] border border-mid-gray/35 px-2 py-0.5 text-[11px] font-medium text-mid-gray cursor-pointer hover:text-red-600"
                  >
                    {deleteSpanLabel}
                  </button>
                ) : null}
              </li>
            );
          })}
        </ol>
      </div>

      {childrenAfterTranscript}

      {isLiveRecording ? (
        <footer className="space-y-0.5 pt-1 text-[12px] text-mid-gray">
          <p>{t("meetings.view.hints.headphones")}</p>
          <p>
            {t(
              "meetings.view.hints.noHiddenMode",
              "No hidden or background mode. The pill stays while recording.",
            )}
          </p>
          <p>
            {dictationShortcut
              ? pushToTalk
                ? t("meetings.view.hints.dictationWithKey", {
                    defaultValue: "Dictation still works (hold {{key}}).",
                    key: dictationShortcut,
                  })
                : t("meetings.view.hints.dictationWithKeyPress", {
                    defaultValue: "Dictation still works (press {{key}}).",
                    key: dictationShortcut,
                  })
              : t("meetings.view.hints.dictation")}
          </p>
        </footer>
      ) : null}
      </section>
    </div>
  );
};
