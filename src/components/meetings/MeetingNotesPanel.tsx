import React from "react";
import { useTranslation } from "react-i18next";
import type {
  LeftOutSegmentView,
  MeetingNoteLineView,
  MeetingNotesSnapshot,
} from "../../bindings.meetings";

const HALF_CIRCLE = "◐";
const SQUARE_MARK = "□";
const LEFT_OUT_CATEGORIES = [
  ["small_talk", "meetings.notes.smallTalkLabel"],
  ["uncovered", "meetings.notes.uncoveredLabel"],
] as const;

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

export const CatchUpCard: React.FC<{
  lines: MeetingNoteLineView[];
  open: boolean;
  onRequestCatchUp: () => void;
  onJumpToSegment: (startMs: number) => void;
}> = ({ lines, open, onRequestCatchUp, onJumpToSegment }) => {
  const { t } = useTranslation();
  const cited = filterCitedNoteLines(lines);
  return (
    <div
      data-testid="catch-up-card"
      className="rounded-[12px] border border-mid-gray/25 bg-mid-gray/[0.04] p-3 space-y-2"
    >
      <div className="flex items-center justify-between gap-2">
        <button
          type="button"
          onClick={onRequestCatchUp}
          className="inline-flex min-h-[28px] items-center rounded-[8px] border border-mid-gray/45 bg-background px-3 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t("meetings.notes.catchUpButton")}
        </button>
        {open ? (
          <span className="text-[11.5px] font-mono text-mid-gray">
            {t("meetings.notes.catchUpWindow")}
          </span>
        ) : null}
      </div>
      {open && cited.length > 0 ? (
        <ul className="space-y-1.5 pt-0.5 text-[13px] text-text">
          {cited.map((line, idx) => {
            const cite = `[${formatNoteTime(line.start_ms)}]`;
            return (
              <li key={`${line.start_ms}-${idx}`} className="flex gap-2">
                <button
                  type="button"
                  onClick={() => onJumpToSegment(line.start_ms)}
                  className="shrink-0 font-mono text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
                >
                  {cite}
                </button>
                <span>{line.text}</span>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
};

export interface MeetingNotesSectionProps {
  recording: boolean;
  snapshot: MeetingNotesSnapshot | null;
  notepadDraft: string;
  retrying?: boolean;
  onNotepadChange: (value: string) => void;
  onJumpToSegment: (startMs: number) => void;
  onRetry: () => void;
}

export const MeetingNotesSection: React.FC<MeetingNotesSectionProps> = ({
  recording,
  snapshot,
  notepadDraft,
  retrying = false,
  onNotepadChange,
  onJumpToSegment,
  onRetry,
}) => {
  const { t } = useTranslation();
  const citedLines = filterCitedNoteLines(snapshot?.lines ?? []);
  const leftOut: LeftOutSegmentView[] = snapshot?.left_out ?? [];
  const totalSteps = Math.max(1, snapshot?.progress_total ?? 4);
  const doneSteps = Math.min(
    totalSteps,
    Math.max(1, snapshot?.progress_step ?? 2),
  );

  const renderGroup = (kind: string, headingKey: string) => {
    const items = citedLines.filter((l) => l.kind === kind);
    if (items.length === 0) return null;
    return (
      <div className="space-y-1.5">
        <h4 className="text-[12px] font-bold uppercase tracking-[0.04em] text-mid-gray">
          {t(headingKey)}
        </h4>
        <ul className="space-y-1.5 text-[13.5px] text-text">
          {items.map((item, idx) => {
            const cite = `[${formatNoteTime(item.start_ms)}]`;
            const ownerPrefix = item.owner ? `${item.owner}: ` : "";
            return (
              <li
                key={`${kind}-${item.start_ms}-${idx}`}
                className="flex flex-wrap items-baseline gap-1.5"
              >
                {ownerPrefix ? (
                  <strong className="font-semibold text-text">
                    {ownerPrefix}
                  </strong>
                ) : null}
                <span>{item.text}</span>
                {item.from_notepad ? (
                  <span className="inline-flex items-center gap-1 rounded-[5px] border border-mid-gray/35 px-1.5 py-0.2 text-[11px] font-medium text-mid-gray">
                    <span aria-hidden="true">{SQUARE_MARK}</span>
                    <span>{t("meetings.notes.yourNote")}</span>
                  </span>
                ) : null}
                <button
                  type="button"
                  onClick={() => onJumpToSegment(item.start_ms)}
                  className="font-mono text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
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
  const decisionsGroup = renderGroup("decisions", "meetings.notes.decisions");
  const actionsGroup = renderGroup("actions", "meetings.notes.actions");
  const questionsGroup = renderGroup(
    "questions",
    "meetings.notes.openQuestions",
  );
  const leftOutGroups = LEFT_OUT_CATEGORIES.map(([cat, labelKey]) => {
    const rows = leftOut.filter((r) => r.category === cat);
    if (rows.length === 0) return null;
    return (
      <div key={cat} className="space-y-1">
        <p className="text-[12px] font-semibold text-mid-gray">{t(labelKey)}</p>
        <ul className="space-y-1 text-[12.5px] text-text">
          {rows.map((r, i) => (
            <li
              key={`${cat}-${r.start_ms}-${i}`}
              className="flex flex-wrap items-baseline justify-between gap-2"
            >
              <span>
                <strong className="font-semibold">{r.speaker}</strong> {r.text}
              </span>
              <button
                type="button"
                onClick={() => onJumpToSegment(r.start_ms)}
                className="shrink-0 text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
              >
                {t("meetings.notes.showInTranscript")}
              </button>
            </li>
          ))}
        </ul>
      </div>
    );
  });

  return (
    <div className="space-y-3.5">
      <section
        data-testid="frame-5-notepad"
        className="rounded-[12px] border border-mid-gray/25 bg-background p-3.5 space-y-2"
      >
        <label
          htmlFor="meeting-notepad"
          className="block text-[13px] font-semibold text-text"
        >
          {t("meetings.notes.yourNotes")}
        </label>
        <textarea
          id="meeting-notepad"
          value={notepadDraft}
          onChange={(e) => onNotepadChange(e.target.value)}
          placeholder={t("meetings.notes.placeholder")}
          rows={3}
          className="w-full rounded-[9px] border border-mid-gray/30 bg-background px-3 py-2 text-[13px] text-text"
        />
      </section>

      {recording && citedLines.length > 0 ? (
        <section
          data-testid="frame-16-notes-so-far"
          className="rounded-[12px] border border-mid-gray/25 bg-background p-3.5 space-y-2"
        >
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-[14px] font-semibold text-text">
              {t("meetings.notes.notesSoFar")}
            </h3>
            <span className="text-[12px] text-mid-gray">
              {t("meetings.notes.notesSoFarHint")}
            </span>
          </div>
          {summaryGroup}
          {decisionsGroup}
        </section>
      ) : null}

      {!recording && snapshot?.state === "running" ? (
        <section
          data-testid="frame-16-progress"
          role="status"
          className="rounded-[12px] border border-mid-gray/25 bg-background p-3.5 space-y-2.5"
        >
          <div className="flex items-center gap-2 text-[13.5px] font-semibold text-text">
            <span aria-hidden="true">{HALF_CIRCLE}</span>
            <span>
              {snapshot.progress_label ??
                t("meetings.notes.finishing", {
                  seconds: snapshot.elapsed_secs,
                })}
            </span>
          </div>
          <div className="grid grid-cols-4 gap-1.5" aria-hidden="true">
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

      {!recording && snapshot?.state === "failed" ? (
        <section
          role="alert"
          data-testid="notes-failed-card"
          className="rounded-[12px] border border-amber-500/35 bg-amber-500/10 p-3.5 flex items-center justify-between gap-3"
        >
          <p className="text-[13px] font-medium text-text">
            {t("meetings.notes.failed")}
          </p>
          <button
            type="button"
            disabled={retrying}
            onClick={onRetry}
            className="shrink-0 rounded-[8px] bg-logo-primary px-3 py-1.5 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90 disabled:opacity-50"
          >
            {t("meetings.notes.retry")}
          </button>
        </section>
      ) : null}

      {!recording && snapshot?.state === "ready" ? (
        <section
          data-testid="frame-14-notes"
          className="rounded-[12px] border border-mid-gray/25 bg-background p-4 space-y-3"
        >
          <div className="flex items-center justify-between gap-2">
            <div>
              <h3 className="text-[16px] font-semibold text-text">
                {t("meetings.notes.heading")}
              </h3>
              <p className="text-[12px] text-mid-gray">
                {t("meetings.notes.subtitle")}
              </p>
            </div>
            {snapshot.time_to_notes_secs != null ? (
              <span className="font-mono text-[11.5px] text-mid-gray">
                {t("meetings.notes.timeToNotes", {
                  seconds: snapshot.time_to_notes_secs,
                })}
              </span>
            ) : null}
          </div>
          {summaryGroup}
          {decisionsGroup}
          {actionsGroup}
          {questionsGroup}
          <p className="pt-1 text-[12px] text-mid-gray">
            {t("meetings.notes.citeRule")}
          </p>
        </section>
      ) : null}

      {!recording && leftOut.length > 0 ? (
        <section
          data-testid="frame-15-left-out"
          className="rounded-[12px] border border-mid-gray/25 bg-mid-gray/[0.04] p-3.5 space-y-2.5"
        >
          <h4 className="text-[14px] font-semibold text-text">
            {t("meetings.notes.leftOutHeading")}
          </h4>
          {leftOutGroups}
          <p className="text-[12px] text-mid-gray">
            {t("meetings.notes.leftOutRule")}
          </p>
        </section>
      ) : null}
    </div>
  );
};
