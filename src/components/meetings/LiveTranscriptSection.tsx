import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import type {
  MeetingSegmentView,
  MeetingStatus,
  MeetingWarning,
} from "../../bindings.meetings";
import { segmentChannelLabel } from "./channelLabels";
import { formatMeetingClock, MeetingFloatingPill } from "./MeetingEntryPanels";

const HALF_CIRCLE = "◐";

export function formatSegmentOffset(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const mins = String(Math.floor(total / 60)).padStart(2, "0");
  const secs = String(total % 60).padStart(2, "0");
  return `${mins}:${secs}`;
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
  const channelLabels = {
    me: t("meetings.view.channel.me"),
    others: t("meetings.view.channel.others"),
  };
  const isPaused = status.state === "paused";
  const isActive = status.state === "recording" || isPaused;
  const qLower = searchQuery.trim().toLowerCase();
  const visibleSegments = qLower
    ? segments.filter((s) => s.text.toLowerCase().includes(qLower))
    : segments;

  return (
    <section
      data-testid="frame-4-live-transcript"
      className="space-y-3 rounded-[14px] border border-mid-gray/25 bg-background p-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 className="text-[16px] font-semibold text-text">
            {t("meetings.view.channels")}
          </h3>
          {status.mic_name ? (
            <p className="text-[12px] text-mid-gray">
              {t("meetings.view.mic", { name: status.mic_name })}
            </p>
          ) : null}
        </div>
        <div className="flex flex-wrap items-center gap-2">
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
          <span className="inline-flex items-center gap-1 text-[12px] text-mid-gray">
            <span aria-hidden="true">{HALF_CIRCLE}</span>
            <span>
              {following
                ? t("meetings.view.autoScrolling")
                : t("meetings.view.scrollPaused")}
            </span>
          </span>
        </div>
      </div>

      {isPaused && pausedHintLabel ? (
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
        className="max-h-72 overflow-y-auto rounded-[10px] border border-mid-gray/20 bg-mid-gray/[0.03]"
      >
        <ol className="divide-y divide-mid-gray/15">
          {visibleSegments.map((seg) => {
            const speaker =
              seg.speaker_label?.trim() ||
              segmentChannelLabel(seg.channel, channelLabels);
            return (
              <li
                key={`${seg.channel}-${seg.start_ms}-${seg.end_ms}`}
                data-start-ms={seg.start_ms}
                className="flex items-start gap-3 px-3 py-2 text-[13px]"
              >
                <span className="w-11 shrink-0 font-mono text-[12px] tabular-nums text-mid-gray">
                  {formatSegmentOffset(seg.start_ms)}
                </span>
                <span className="w-16 shrink-0 text-[12px] font-semibold text-text/80">
                  {speaker}
                </span>
                <span className="min-w-0 flex-1 text-text">{seg.text}</span>
                {onDeleteSpan && deleteSpanLabel ? (
                  <button
                    type="button"
                    onClick={() => onDeleteSpan(seg)}
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

      <footer className="space-y-0.5 pt-1 text-[12px] text-mid-gray">
        <p>{t("meetings.view.hints.headphones")}</p>
        <p>{t("meetings.view.hints.dictation")}</p>
      </footer>
    </section>
  );
};
