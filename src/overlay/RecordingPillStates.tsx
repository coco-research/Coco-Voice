import React from "react";
import { useTranslation } from "react-i18next";

const DOT_FILLED = "●";
const HALF_CIRCLE = "◐";
const CHECK_MARK = "✓";
const CLIPBOARD_MARK = "⧉";

export type RecordingPillKind =
  | "listening"
  | "streaming"
  | "working"
  | "sent"
  | "clipboard"
  | "kept_press_paste";

export interface RecordingPillStateProps {
  kind: RecordingPillKind;
  streamingWords?: string;
  targetApp?: string;
}

export const RecordingPillStateView: React.FC<RecordingPillStateProps> = ({
  kind,
  streamingWords = "",
  targetApp = "Notes",
}) => {
  const { t } = useTranslation();

  let glyph = DOT_FILLED;
  let glyphClass = "text-red-500";
  let text = t("overlay.pillStates.listening");

  if (kind === "streaming") {
    text = streamingWords || t("overlay.pillStates.listening");
  } else if (kind === "working") {
    glyph = HALF_CIRCLE;
    glyphClass = "text-amber-500";
    text = t("overlay.pillStates.working");
  } else if (kind === "sent") {
    glyph = CHECK_MARK;
    glyphClass = "text-emerald-600";
    text = t("overlay.pillStates.sentToApp", { app: targetApp });
  } else if (kind === "clipboard") {
    glyph = CLIPBOARD_MARK;
    glyphClass = "text-logo-primary";
    text = t("overlay.pillStates.onClipboard");
  } else if (kind === "kept_press_paste") {
    glyph = CLIPBOARD_MARK;
    glyphClass = "text-amber-600";
    text = t("overlay.pillStates.wordsKeptPressPaste");
  }

  return (
    <div
      data-testid={`pill-state-${kind}`}
      role="status"
      aria-live="polite"
      className="inline-flex items-center gap-2 rounded-full border border-mid-gray/35 bg-background px-3.5 py-1.5 shadow-sm text-[12.5px] font-medium text-text"
    >
      <span aria-hidden="true" className={glyphClass}>
        {glyph}
      </span>
      <span>{text}</span>
    </div>
  );
};
