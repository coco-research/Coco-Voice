import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type LeftOutSegmentView,
  type MeetingNoteLineView,
  type MeetingNotesSnapshot,
  type MeetingSegmentView,
  type Result,
} from "../../bindings.meetings";
import { segmentChannelLabel } from "./channelLabels";
import {
  formatSegmentOffset,
  resolveTranscriptSpeakerLabel,
} from "./LiveTranscriptSection";

const HALF_CIRCLE = "◐";
const SQUARE_MARK = "□";
const TIME_CHIP_CLASS =
  "shrink-0 rounded-md border border-mid-gray/20 bg-logo-primary/10 px-1.5 py-0.5 font-mono text-[11.5px] font-semibold text-logo-primary cursor-pointer hover:bg-logo-primary/15 dark:bg-logo-primary/15";
const LEFT_OUT_CATEGORIES = [
  ["small_talk", "meetings.notes.smallTalkLabel"],
  ["uncovered", "meetings.notes.uncoveredLabel"],
] as const;

export interface FailedTranscriptLoadResult {
  segments: MeetingSegmentView[];
  loadFailed: boolean;
}

export async function loadFailedTranscriptSegments(
  meetingId: number,
  fetchSegments: (
    id: number,
  ) => Promise<Result<MeetingSegmentView[], string>> = (id) =>
    commands.meetingSegments(id),
): Promise<FailedTranscriptLoadResult> {
  try {
    const res = await fetchSegments(meetingId);
    if (res.status === "ok") {
      return { segments: res.data, loadFailed: false };
    }
    return { segments: [], loadFailed: true };
  } catch {
    return { segments: [], loadFailed: true };
  }
}

export function formatNoteTime(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const minutes = String(Math.floor(total / 60)).padStart(2, "0");
  const seconds = String(total % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}

export interface ScrollableTranscriptContainer {
  querySelector: (selector: string) => {
    scrollIntoView: (options: ScrollIntoViewOptions) => void;
  } | null;
}

export function scrollTranscriptToSegment(
  container: ScrollableTranscriptContainer | null,
  startMs: number,
): boolean {
  if (!container) return false;
  const row = container.querySelector(`[data-start-ms="${startMs}"]`);
  if (!row) return false;
  row.scrollIntoView({ behavior: "smooth", block: "center" });
  return true;
}

export function normalizeNoteKind(
  kind: string,
): "summary" | "decision" | "action" | "question" | string {
  switch (kind) {
    case "summary":
      return "summary";
    case "decision":
    case "decisions":
      return "decision";
    case "action":
    case "actions":
      return "action";
    case "question":
    case "questions":
      return "question";
    default:
      return kind;
  }
}

export function failureReasonMessage(
  reason: string | null | undefined,
  t: (key: string, defaultValue: string) => string,
): string | null {
  switch (reason) {
    case "no_model":
      return t(
        "meetings.notes.reasonNoModel",
        "No clean-up model is downloaded. Get one in Models.",
      );
    case "unavailable":
      return t(
        "meetings.notes.reasonUnavailable",
        "Notes aren't available on this system yet.",
      );
    case "interrupted":
      return t(
        "meetings.notes.reasonInterrupted",
        "Coco Voice closed while making notes.",
      );
    default:
      return null;
  }
}

// Every displayed note line must link to a real transcript timestamp;
// uncited lines are dropped before rendering.
export function filterCitedNoteLines(
  lines: MeetingNoteLineView[],
): Array<MeetingNoteLineView & { start_ms: number }> {
  return lines.filter(
    (line): line is MeetingNoteLineView & { start_ms: number } =>
      typeof line.start_ms === "number" && Number.isFinite(line.start_ms),
  );
}

const CATCH_UP_WINDOWS = [5, 10, 15] as const;
export type CatchUpMinutes = (typeof CATCH_UP_WINDOWS)[number];

export const CatchUpCard: React.FC<{
  lines: MeetingNoteLineView[];
  open: boolean;
  minutes?: CatchUpMinutes;
  notEnoughSaid?: boolean;
  onSelectMinutes?: (minutes: CatchUpMinutes) => void;
  onRequestCatchUp: (minutes?: CatchUpMinutes) => void;
  onJumpToSegment: (startMs: number) => void;
}> = ({
  lines,
  open,
  minutes = 5,
  notEnoughSaid = false,
  onSelectMinutes,
  onRequestCatchUp,
  onJumpToSegment,
}) => {
  const { t } = useTranslation();
  const cited = filterCitedNoteLines(lines);
  const windowLabel =
    minutes === 5
      ? t("meetings.notes.catchUpWindow", "Last 5 minutes")
      : t("meetings.notes.catchUpWindowMinutes", {
          defaultValue: "Last {{minutes}} minutes",
          minutes,
        });
  return (
    <div
      data-testid="catch-up-card"
      className="rounded-[12px] border border-mid-gray/25 bg-mid-gray/[0.04] p-3 space-y-2"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2.5">
          <button
            type="button"
            onClick={() => onRequestCatchUp(minutes)}
            className="inline-flex min-h-[28px] items-center rounded-[8px] border border-mid-gray/45 bg-background px-3 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("meetings.notes.catchUpButton", "What did I miss?")}
          </button>
          <span className="text-[12px] text-mid-gray">{windowLabel}</span>
        </div>
        {onSelectMinutes ? (
          <div
            role="group"
            aria-label={t(
              "meetings.notes.catchUpChooseWindow",
              "Catch-up window",
            )}
            className="inline-flex items-center gap-1"
          >
            {CATCH_UP_WINDOWS.map((m) => {
              const active = minutes === m;
              return (
                <button
                  key={m}
                  type="button"
                  aria-pressed={active}
                  onClick={() => onSelectMinutes(m)}
                  className={`rounded-full px-2 py-0.5 font-mono text-[11px] font-semibold cursor-pointer ${
                    active
                      ? "bg-logo-primary text-white"
                      : "border border-mid-gray/35 bg-background text-mid-gray hover:text-text"
                  }`}
                >
                  {t("meetings.notes.catchUpMinutesPill", {
                    defaultValue: "{{minutes}}m",
                    minutes: m,
                  })}
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
      {open && cited.length > 0 ? (
        <div className="rounded-[10px] border border-mid-gray/20 bg-background p-2.5 space-y-1.5">
          <p className="text-[12px] font-semibold text-mid-gray">
            {windowLabel}
          </p>
          <ul className="space-y-1.5 text-[13px] text-text">
            {cited.map((line, idx) => {
              const cite = `[${formatNoteTime(line.start_ms)}]`;
              return (
                <li
                  key={`${line.start_ms}-${idx}`}
                  className="flex flex-wrap items-baseline justify-between gap-2"
                >
                  <span>{line.text}</span>
                  <button
                    type="button"
                    onClick={() => onJumpToSegment(line.start_ms)}
                    className={TIME_CHIP_CLASS}
                  >
                    {cite}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ) : open && notEnoughSaid ? (
        <p
          role="status"
          data-testid="catch-up-not-enough"
          className="rounded-[10px] border border-mid-gray/20 bg-background px-3 py-2 text-[12.5px] text-mid-gray"
        >
          {t("meetings.notes.notEnoughSaid", "Not enough has been said yet.")}
        </p>
      ) : null}
    </div>
  );
};

export interface MeetingNotesSectionProps {
  recording: boolean;
  snapshot: MeetingNotesSnapshot | null;
  notepadDraft: string;
  initialView?: "both" | "left_out";
  retrying?: boolean;
  stopping?: boolean;
  segments?: MeetingSegmentView[];
  hasSpokenSegments?: boolean;
  showNotesSoFarCard?: boolean;
  onNotepadChange: (value: string) => void;
  onJumpToSegment: (startMs: number) => void;
  onRetry: () => void;
}

export const MeetingNotesSection: React.FC<MeetingNotesSectionProps> = ({
  recording,
  snapshot,
  notepadDraft,
  initialView = "both",
  retrying = false,
  stopping = false,
  segments,
  hasSpokenSegments,
  showNotesSoFarCard = false,
  onNotepadChange,
  onJumpToSegment,
  onRetry,
}) => {
  const { t } = useTranslation();
  const [activeView, setActiveView] = useState<"both" | "left_out">(
    initialView,
  );
  const [failedSegments, setFailedSegments] = useState<MeetingSegmentView[]>(
    segments ?? [],
  );
  const [segmentsLoadFailed, setSegmentsLoadFailed] = useState(false);
  const citedLines = filterCitedNoteLines(snapshot?.lines ?? []);
  const leftOut: LeftOutSegmentView[] = snapshot?.left_out ?? [];
  const totalSteps = Math.max(1, snapshot?.progress_total ?? 5);
  const doneSteps = Math.min(
    totalSteps,
    Math.max(1, snapshot?.progress_step ?? 2),
  );

  useEffect(() => {
    setFailedSegments(segments ?? []);
    if (segments && segments.length > 0) {
      setSegmentsLoadFailed(false);
    }
  }, [segments, snapshot?.meeting_id]);

  useEffect(() => {
    if (recording || snapshot?.state !== "failed" || !snapshot.meeting_id) {
      return;
    }
    if (segments && segments.length > 0) {
      return;
    }
    let cancelled = false;
    setSegmentsLoadFailed(false);
    void loadFailedTranscriptSegments(snapshot.meeting_id).then((res) => {
      if (!cancelled) {
        setFailedSegments(res.segments);
        setSegmentsLoadFailed(res.loadFailed);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [recording, snapshot?.state, snapshot?.meeting_id, segments]);

  const renderGroup = (kind: string, headingKey: string) => {
    const items = citedLines.filter(
      (l) => normalizeNoteKind(l.kind) === normalizeNoteKind(kind),
    );
    if (items.length === 0) return null;
    return (
      <div className="space-y-0.5">
        <h4 className="text-[12px] font-semibold text-mid-gray">
          {t(headingKey)}
        </h4>
        <ul className="space-y-0.5 text-[12.5px] text-text">
          {items.map((item, idx) => {
            const timeLabel = formatNoteTime(item.start_ms);
            const cite = `[${timeLabel}]`;
            const ownerPrefix = item.owner ? `${item.owner}: ` : "";
            return (
              <li
                key={`${kind}-${item.start_ms}-${idx}`}
                className="flex items-baseline justify-between gap-2"
              >
                <span className="min-w-0 flex-1 leading-[16.5px]">
                  {ownerPrefix ? (
                    <strong className="font-semibold text-text">
                      {ownerPrefix}
                    </strong>
                  ) : null}
                  <span>{item.text}</span>
                  {item.from_notepad ? (
                    <span className="ml-1.5 inline-flex items-center gap-1 rounded-[5px] border border-mid-gray/35 px-1.5 py-0.2 align-baseline text-[11px] font-medium text-mid-gray">
                      <span aria-hidden="true">{SQUARE_MARK}</span>
                      <span>{t("meetings.notes.yourNote")}</span>
                    </span>
                  ) : null}
                </span>
                <button
                  type="button"
                  aria-label={t("meetings.notes.goToTime", {
                    defaultValue: "Go to {{time}}",
                    time: timeLabel,
                  })}
                  onClick={() => onJumpToSegment(item.start_ms)}
                  className={TIME_CHIP_CLASS}
                >
                  {cite}
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    );
  };

  const summaryGroup = renderGroup("summary", "meetings.notes.summary");
  const decisionsGroup = renderGroup("decision", "meetings.notes.decisions");
  const actionsGroup = renderGroup("action", "meetings.notes.actions");
  const questionsGroup = renderGroup(
    "question",
    "meetings.notes.openQuestions",
  );
  const leftOutGroups = LEFT_OUT_CATEGORIES.map(([cat, labelKey]) => {
    const rows = leftOut.filter((r) =>
      cat === "small_talk"
        ? r.category === "small_talk"
        : r.category === "uncovered" || r.category === "not_covered",
    );
    if (rows.length === 0) return null;
    return (
      <div key={cat} className="space-y-1">
        <p className="text-[12px] font-semibold text-mid-gray">{t(labelKey)}</p>
        <ul className="space-y-1 text-[12.5px] text-text">
          {rows.map((r, i) => {
            const startLabel = formatNoteTime(r.start_ms);
            const range =
              typeof r.end_ms === "number" && r.end_ms > r.start_ms
                ? `${startLabel}-${formatNoteTime(r.end_ms)}`
                : startLabel;
            let rowText: string;
            if (cat === "small_talk") {
              const topic =
                r.topic?.trim() ||
                r.reason?.trim() ||
                r.text?.trim() ||
                t("meetings.notes.smallTalkFallback", "Small talk");
              rowText = t("meetings.notes.smallTalkRow", {
                defaultValue:
                  "{{topic}}, {{range}}. Not a decision, action or question.",
                topic: topic.replace(/\.$/, ""),
                range,
              });
            } else {
              const rawReason = (
                r.reason?.trim() ||
                r.text?.trim() ||
                t(
                  "meetings.notes.notCoveredFallback",
                  "No note covers this part.",
                )
              ).replace(/\.$/, "");
              const topicPrefix = r.topic?.trim() ? `${r.topic.trim()}: ` : "";
              rowText = t("meetings.notes.notCoveredRow", {
                defaultValue: "{{range}}. {{reason}}.",
                range,
                reason: `${topicPrefix}${rawReason}`,
              });
            }
            return (
              <li
                key={`${cat}-${r.start_ms}-${i}`}
                className="flex items-baseline justify-between gap-2"
              >
                <span className="min-w-0 flex-1 leading-[18px]">{rowText}</span>
                <button
                  type="button"
                  onClick={() => onJumpToSegment(r.start_ms)}
                  className="shrink-0 text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
                >
                  {t("meetings.notes.showInTranscript", "Show in transcript")}
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    );
  });

  const notepadCard = (rows = 5) => (
    <section
      data-testid="frame-5-notepad"
      className="rounded-[12px] border border-mid-gray/25 bg-background p-4 flex flex-col space-y-2"
    >
      <label
        htmlFor="meeting-notepad"
        className="block text-[14px] font-semibold text-text"
      >
        {t("meetings.notes.yourNotes")}
      </label>
      <textarea
        id="meeting-notepad"
        value={notepadDraft}
        onChange={(e) => onNotepadChange(e.target.value)}
        placeholder={t("meetings.notes.placeholder")}
        rows={rows}
        className="w-full flex-1 rounded-[9px] border border-mid-gray/30 bg-background px-3 py-2 text-[13px] leading-[19px] text-text"
      />
    </section>
  );

  // Scene 15 (Frame 5b): If notes fail -> compact 2-column Transcript | Your notes + Retry banner above 570px (W30, s5)
  if (!recording && snapshot?.state === "failed") {
    const channelLabels = {
      me: t("meetings.view.channel.me", "Me"),
      others: t("meetings.view.channel.others", "Others"),
    };
    return (
      <div className="space-y-2.5">
        <div className="grid grid-cols-2 gap-3 items-stretch">
          <section
            data-testid="notes-failed-transcript"
            className="rounded-[12px] border border-mid-gray/25 bg-background p-2.5 space-y-1.5 max-h-[216px] overflow-y-auto"
          >
            <h3 className="text-[13.5px] font-semibold text-text">
              {t("meetings.notes.transcriptHeading", "Transcript")}
            </h3>
            {segmentsLoadFailed ? (
              <p role="alert" className="text-sm text-red-600">
                {t("meetings.view.loadFailed")}
              </p>
            ) : null}
            <ol className="divide-y divide-mid-gray/15 text-[12px]">
              {failedSegments.slice(0, 4).map((seg) => {
                const speaker =
                  resolveTranscriptSpeakerLabel(seg, channelLabels, false) ||
                  segmentChannelLabel(seg.channel, channelLabels);
                return (
                  <li
                    key={`failed-${seg.channel}-${seg.start_ms}`}
                    className="flex flex-col md:flex-row md:items-start gap-0.5 md:gap-2 py-1.5"
                  >
                    <div className="flex items-baseline gap-1.5 md:contents">
                      <span className="md:w-10 shrink-0 font-mono text-[11px] tabular-nums text-mid-gray">
                        {formatSegmentOffset(seg.start_ms)}
                      </span>
                      <span className="md:w-11 shrink-0 font-semibold text-text/80">
                        {speaker}
                      </span>
                    </div>
                    <span className="min-w-0 flex-1 leading-[17px] text-text">
                      {seg.text}
                    </span>
                  </li>
                );
              })}
            </ol>
          </section>
          <section
            data-testid="frame-5-notepad"
            className="rounded-[12px] border border-mid-gray/25 bg-background p-2.5 flex flex-col space-y-1.5 max-h-[216px]"
          >
            <label
              htmlFor="meeting-notepad"
              className="block text-[13.5px] font-semibold text-text"
            >
              {t("meetings.notes.yourNotes")}
            </label>
            <textarea
              id="meeting-notepad"
              value={notepadDraft}
              onChange={(e) => onNotepadChange(e.target.value)}
              placeholder={t("meetings.notes.placeholder")}
              rows={5}
              className="w-full flex-1 rounded-[9px] border border-mid-gray/30 bg-background px-2.5 py-1.5 text-[12.5px] leading-[17px] text-text"
            />
          </section>
        </div>

        <section
          role="alert"
          data-testid="notes-failed-card"
          className="rounded-[12px] border border-amber-500/35 bg-amber-500/10 px-3.5 py-2.5 flex items-center justify-between gap-3"
        >
          <div className="space-y-0.5">
            <p className="text-[13px] font-medium text-text">
              {t("meetings.notes.failed")}
            </p>
            {failureReasonMessage(snapshot.failure_reason, t) ? (
              <p className="text-[12px] text-mid-gray">
                {failureReasonMessage(snapshot.failure_reason, t)}
              </p>
            ) : null}
          </div>
          <button
            type="button"
            data-testid="notes-retry-btn"
            disabled={retrying}
            onClick={onRetry}
            className="shrink-0 rounded-[8px] bg-logo-primary px-3 py-1.5 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90 disabled:opacity-50"
          >
            {t("meetings.notes.retry")}
          </button>
        </section>
      </div>
    );
  }

  // Scene 26 (Frame 16): Notes while you talk -> one merged list of lines, each with its time, NO "your note" chips (B1-9 / W2 / S5)
  if (recording && (citedLines.length > 0 || showNotesSoFarCard)) {
    const missingModelDuringCall = snapshot?.failure_reason === "no_model";
    return (
      <section
        data-testid="frame-16-notes-so-far"
        className="rounded-[12px] border border-mid-gray/25 bg-background p-4 space-y-2.5"
      >
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h3 className="text-[14px] font-semibold text-text">
            {t("meetings.notes.notesSoFar", "Notes so far")}
          </h3>
          <span className="text-[12px] text-mid-gray">
            {t(
              "meetings.notes.notesSoFarSubtitle",
              "Notes so far. Updated every few minutes.",
            )}
          </span>
        </div>
        {citedLines.length > 0 ? (
          <ul className="space-y-1.5 text-[13.5px] text-text">
            {citedLines.map((item, idx) => {
              const timeLabel = formatNoteTime(item.start_ms);
              const cite = `[${timeLabel}]`;
              const ownerPrefix = item.owner ? `${item.owner}: ` : "";
              return (
                <li
                  key={`sofar-${item.start_ms}-${idx}`}
                  className="flex items-baseline justify-between gap-3"
                >
                  <span className="min-w-0 flex-1 leading-[20px]">
                    {ownerPrefix ? (
                      <strong className="font-semibold text-text">
                        {ownerPrefix}
                      </strong>
                    ) : null}
                    <span>{item.text}</span>
                  </span>
                  <button
                    type="button"
                    aria-label={t("meetings.notes.goToTime", {
                      defaultValue: "Go to {{time}}",
                      time: timeLabel,
                    })}
                    onClick={() => onJumpToSegment(item.start_ms)}
                    className={TIME_CHIP_CLASS}
                  >
                    {cite}
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="text-[12.5px] text-mid-gray">
            {missingModelDuringCall
              ? t(
                  "meetings.notes.notesSoFarNoModel",
                  "Notes need a clean-up model. Get one in Models.",
                )
              : t(
                  "meetings.notes.notesSoFarPending",
                  "Notes appear after a few minutes.",
                )}
          </p>
        )}
      </section>
    );
  }

  // Empty stopped meeting (D-B4): nothing was said
  if (
    !recording &&
    !stopping &&
    hasSpokenSegments === false &&
    (!snapshot || snapshot.state === "idle") &&
    citedLines.length === 0
  ) {
    return (
      <div className="space-y-3.5">
        <section
          data-testid="notes-empty-meeting"
          role="status"
          className="rounded-[12px] border border-mid-gray/25 bg-background p-4"
        >
          <p className="text-[13px] text-mid-gray">
            {t(
              "meetings.notes.nothingWasSaid",
              "Nothing was said, so there are no notes.",
            )}
          </p>
        </section>
        {notepadCard(3)}
      </div>
    );
  }

  // Scene 14 (Frame 5), Scene 24 (Frame 14), Scene 25 (Frame 15): compact 2-column Notes + dedicated Left out view (W29, W39, W43, S4)
  if (!recording && snapshot?.state === "ready") {
    const leftOutSection = (
      <section data-testid="frame-15-left-out" className="space-y-2.5">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 items-start">
          <div className="rounded-[12px] border border-mid-gray/25 bg-background p-3 space-y-1.5">
            <h4 className="text-[14px] font-semibold text-text">
              {t("meetings.notes.heading")}
            </h4>
            {summaryGroup}
            {decisionsGroup}
            {actionsGroup}
            {questionsGroup}
          </div>
          <div className="rounded-[12px] border border-mid-gray/25 bg-mid-gray/[0.04] p-3 space-y-1.5">
            <h4 className="text-[14px] font-semibold text-text">
              {t("meetings.notes.leftOutHeading", "Left out")}
            </h4>
            {leftOut.length > 0 ? (
              leftOutGroups
            ) : (
              <p
                data-testid="left-out-empty"
                className="text-[12.5px] text-mid-gray"
              >
                {t("meetings.notes.nothingLeftOut", "Nothing was left out.")}
              </p>
            )}
          </div>
        </div>
        <p className="text-[12px] text-mid-gray">
          {t(
            "meetings.notes.leftOutRule",
            "Ideas that were rejected or left as maybe are not notes. They stay in the transcript.",
          )}
        </p>
      </section>
    );

    return (
      <div className="!mt-0">
        <div className="my-4 flex items-center justify-start">
          <div
            role="group"
            aria-label={t("meetings.notes.viewSwitchAria", "Notes view")}
            data-testid="notes-view-segmented"
            className="inline-flex items-center rounded-[8px] border border-mid-gray/25 bg-mid-gray/10 p-0.5 dark:bg-mid-gray/20"
          >
            <button
              type="button"
              data-testid="notes-tab-notes"
              aria-pressed={activeView === "both"}
              onClick={() => setActiveView("both")}
              className={`rounded-[6px] px-2.5 py-0.5 text-[12px] font-medium transition-colors cursor-pointer ${
                activeView === "both"
                  ? "bg-background text-text shadow-xs dark:bg-surface-elevated dark:text-text"
                  : "text-mid-gray hover:text-text"
              }`}
            >
              {t("meetings.notes.viewYourNotes", "Your notes")}
            </button>
            <button
              type="button"
              data-testid="notes-tab-left-out"
              aria-pressed={activeView === "left_out"}
              onClick={() => setActiveView("left_out")}
              className={`rounded-[6px] px-2.5 py-0.5 text-[12px] font-medium transition-colors cursor-pointer ${
                activeView === "left_out"
                  ? "bg-background text-text shadow-xs dark:bg-surface-elevated dark:text-text"
                  : "text-mid-gray hover:text-text"
              }`}
            >
              {t("meetings.notes.leftOutHeading", "Left out")}
            </button>
          </div>
        </div>

        {activeView === "left_out" ? (
          leftOutSection
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3 items-stretch">
            {notepadCard(5)}
            <section
              data-testid="frame-14-notes"
              className="rounded-[12px] border border-mid-gray/25 bg-background px-2.5 py-2 space-y-1"
            >
              <span className="sr-only" role="status">
                {t("meetings.list.notesReady", "Notes ready")}
              </span>
              <div>
                <h3 className="text-[14.5px] font-semibold text-text">
                  {t("meetings.notes.heading")}
                </h3>
                <p className="text-[11.5px] text-mid-gray">
                  {t("meetings.notes.subtitle")}
                </p>
              </div>
              {summaryGroup}
              {decisionsGroup}
              {actionsGroup}
              {questionsGroup}
              <p
                data-testid="notes-cite-rule"
                className="pt-0.5 text-[11px] leading-[15px] text-mid-gray"
              >
                {t("meetings.notes.citeRule")}
              </p>
            </section>
          </div>
        )}
      </div>
    );
  }

  const finishingLabel = (() => {
    if (snapshot?.state !== "running") {
      return t("meetings.list.finishingNotes", "Finishing notes");
    }
    if (snapshot.progress_label) {
      return snapshot.progress_label;
    }
    if (snapshot.estimate_secs != null) {
      return t("meetings.notes.finishingAbout", {
        defaultValue: "Finishing notes: about {{seconds}} s",
        seconds: snapshot.estimate_secs,
      });
    }
    return t("meetings.notes.finishing", {
      seconds: snapshot.elapsed_secs,
    });
  })();

  // Scene 27 (Frame 16b After Stop) or active recording notepad without notes-so-far yet
  return (
    <div className="space-y-3.5">
      {!recording && (snapshot?.state === "running" || stopping) ? (
        <section
          data-testid="frame-16-progress"
          role="status"
          className="rounded-[12px] border border-mid-gray/25 bg-background p-4 space-y-2.5"
        >
          <div className="flex items-center justify-between gap-2">
            <span className="inline-flex items-center gap-2 text-[13.5px] font-semibold text-text">
              <span aria-hidden="true">{HALF_CIRCLE}</span>
              <span>{finishingLabel}</span>
            </span>
          </div>
          <div
            className="grid gap-1.5"
            style={{
              gridTemplateColumns: `repeat(${totalSteps}, minmax(0, 1fr))`,
            }}
            aria-hidden="true"
          >
            {Array.from({ length: totalSteps }).map((_, i) => (
              <span
                key={i}
                className={`h-1.5 rounded-full ${
                  i < doneSteps ? "bg-logo-primary" : "bg-mid-gray/25"
                }`}
              />
            ))}
          </div>
        </section>
      ) : null}

      {notepadCard(3)}
    </div>
  );
};
