import React from "react";
import { useTranslation } from "react-i18next";

const HALF_CIRCLE = "◐";
const CHECK_MARK = "✓";
const CLIPBOARD_MARK = "⧉";
const RECORD_DOT = "●";
const QUESTION_MARK = "?";
const WAVE_LISTENING = [5, 10, 14, 8, 16, 11, 6, 13, 5] as const;
const WAVE_STREAMING = [5, 10, 14, 8, 16] as const;

export const MAX_AGENT_QUESTION_CHARS = 300;
export const MAX_PILL_APP_NAME_CHARS = 24;

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
  isMac?: boolean;
}

export function formatMeetingElapsed(elapsedSecs: number): string {
  const total = Math.max(0, Math.floor(elapsedSecs));
  const hours = Math.floor(total / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  if (hours > 0) {
    return `${hours}:${pad(mins)}:${pad(secs)}`;
  }
  return `${pad(mins)}:${pad(secs)}`;
}

export function truncatePillAppName(raw: string | undefined): string {
  const trimmed = (raw ?? "").trim();
  if (!trimmed) return "";
  const chars = Array.from(trimmed);
  if (chars.length <= MAX_PILL_APP_NAME_CHARS) return trimmed;
  return `${chars.slice(0, MAX_PILL_APP_NAME_CHARS).join("")}…`;
}

export function truncateAgentQuestion(
  raw: string,
  maxChars = MAX_AGENT_QUESTION_CHARS,
): string {
  const collapsed = raw.replace(/\s+/g, " ").trim();
  const chars = Array.from(collapsed);
  if (chars.length <= maxChars) return collapsed;
  return `${chars.slice(0, maxChars).join("")}…`;
}

function detectIsMacOs(): boolean {
  try {
    const win = globalThis as {
      window?: { __TAURI_OS_PLUGIN_INTERNALS__?: { os_type?: string } };
    };
    const osType = win.window?.__TAURI_OS_PLUGIN_INTERNALS__?.os_type;
    if (!osType) return true;
    return osType === "macos";
  } catch {
    return true;
  }
}

const MiniWaveform: React.FC<{ bars: readonly number[] }> = ({ bars }) => (
  <span
    aria-hidden="true"
    className="inline-flex items-center gap-[2px] h-[16px]"
  >
    {bars.map((height, idx) => (
      <i
        key={idx}
        style={{ height: `${height}px` }}
        className="w-[2.5px] rounded-full bg-logo-primary inline-block"
      />
    ))}
  </span>
);

export const RecordingPillStateView: React.FC<RecordingPillStateProps> = ({
  kind,
  streamingWords = "",
  targetApp,
  isMac,
}) => {
  const { t } = useTranslation();
  const mac = isMac !== undefined ? isMac : detectIsMacOs();

  let glyph: React.ReactNode = <MiniWaveform bars={WAVE_LISTENING} />;
  let text = t("overlay.pillStates.listening", "Listening");

  if (kind === "streaming") {
    glyph = <MiniWaveform bars={WAVE_STREAMING} />;
    text = streamingWords || t("overlay.pillStates.listening", "Listening");
  } else if (kind === "working") {
    glyph = (
      <span aria-hidden="true" className="text-amber-500">
        {HALF_CIRCLE}
      </span>
    );
    text = t("overlay.pillStates.working", "Working");
  } else if (kind === "sent") {
    glyph = (
      <span
        aria-hidden="true"
        className="text-emerald-600 dark:text-emerald-400"
      >
        {CHECK_MARK}
      </span>
    );
    const cleanApp = truncatePillAppName(targetApp);
    text = cleanApp
      ? t("overlay.pillStates.sentToApp", {
          app: cleanApp,
          defaultValue: `Sent to ${cleanApp}`,
        })
      : t("overlay.pillStates.sentGeneric", "Sent");
  } else if (kind === "clipboard") {
    glyph = (
      <span aria-hidden="true" className="text-logo-primary">
        {CLIPBOARD_MARK}
      </span>
    );
    text = t("overlay.pillStates.onClipboard", "On the clipboard");
  } else if (kind === "kept_press_paste") {
    glyph = (
      <span aria-hidden="true" className="text-amber-600 dark:text-amber-400">
        {CLIPBOARD_MARK}
      </span>
    );
    text = mac
      ? t("overlay.pillStates.wordsKeptPressPaste", "Words kept · press ⌘V")
      : t(
          "overlay.pillStates.wordsKeptPressPasteNonMac",
          "Words kept · press Ctrl+V",
        );
  }

  return (
    <div
      data-testid={`pill-state-${kind}`}
      role="status"
      aria-live="polite"
      className="inline-flex items-center gap-2 rounded-full border border-mid-gray/35 bg-background px-3.5 py-1.5 shadow-sm text-[12.5px] font-medium text-text"
    >
      {glyph}
      <span>{text}</span>
    </div>
  );
};

export interface HandsFreePillProps {
  shortcutKey: string;
}

export const HandsFreePillView: React.FC<HandsFreePillProps> = ({
  shortcutKey,
}) => {
  const { t } = useTranslation();
  const keyLabel = shortcutKey.trim() || "fn";
  return (
    <div
      data-testid="pill-state-hands-free"
      role="status"
      aria-live="polite"
      className="inline-flex items-center gap-2.5 rounded-full border border-mid-gray/35 bg-background px-3.5 py-1.5 shadow-sm text-[12.5px] font-medium text-text"
    >
      <MiniWaveform bars={WAVE_LISTENING} />
      <span>
        {t("overlay.pillStates.handsFreeHint", {
          key: keyLabel,
          defaultValue: `Hands-free · tap ${keyLabel} to stop · Esc cancels`,
        })}
      </span>
    </div>
  );
};

export interface MeetingPillProps {
  elapsedSecs: number;
  stopping?: boolean;
  micOnly?: boolean;
  waitingAgentQuestions?: number;
  onStop: () => void;
}

export const MeetingPillView: React.FC<MeetingPillProps> = ({
  elapsedSecs,
  stopping = false,
  micOnly = false,
  waitingAgentQuestions = 0,
  onStop,
}) => {
  const { t } = useTranslation();
  const formatted = formatMeetingElapsed(elapsedSecs);
  return (
    <div
      data-testid="pill-state-meeting"
      role="status"
      aria-live="polite"
      className="inline-flex items-center gap-2.5 rounded-full border border-mid-gray/35 bg-background px-3.5 py-1.5 shadow-sm text-[12.5px] font-medium text-text"
    >
      <span aria-hidden="true" className="text-red-500">
        {RECORD_DOT}
      </span>
      <span data-testid="meeting-pill-elapsed">
        {t("overlay.meetingPill.recordingTime", {
          time: formatted,
          defaultValue: `Recording ${formatted}`,
        })}
      </span>
      {micOnly ? (
        <span
          data-testid="mic-only-chip"
          className="inline-flex items-center rounded-full border border-mid-gray/40 px-2 py-0.5 text-[11px] font-bold tracking-[0.04em] text-text/80"
        >
          {t("meetings.pill.micOnly", "Mic only")}
        </span>
      ) : null}
      {waitingAgentQuestions > 0 ? (
        <span
          data-testid="meeting-pill-waiting"
          className="font-mono text-[11.5px] text-mid-gray"
        >
          {t("overlay.agentAsk.meetingWaitingBadge", {
            count: waitingAgentQuestions,
            defaultValue: `${QUESTION_MARK} ${waitingAgentQuestions} waiting`,
          })}
        </span>
      ) : null}
      <button
        type="button"
        disabled={stopping}
        onClick={onStop}
        className="rounded-full border border-mid-gray/40 px-2.5 py-0.5 text-[11.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/15 disabled:opacity-50"
      >
        {t("overlay.meetingPill.stopButton", "Stop")}
      </button>
    </div>
  );
};

export interface AgentAskPillProps {
  agentName: string;
  question: string;
  shortcutKey?: string;
  queuedCount?: number;
  answerSentTo?: string | null;
}

export const AgentAskPillView: React.FC<AgentAskPillProps> = ({
  agentName,
  question,
  shortcutKey = "fn",
  queuedCount = 0,
  answerSentTo = null,
}) => {
  const { t } = useTranslation();
  if (answerSentTo) {
    return (
      <div
        data-testid="pill-agent-ask-sent"
        role="status"
        aria-live="polite"
        className="inline-flex items-center gap-2 rounded-full border border-mid-gray/35 bg-background px-3.5 py-1.5 shadow-sm text-[12.5px] font-medium text-text"
      >
        <span
          aria-hidden="true"
          className="text-emerald-600 dark:text-emerald-400"
        >
          {CHECK_MARK}
        </span>
        <span>
          {t("overlay.agentAsk.answerSentTo", {
            agent: answerSentTo,
            defaultValue: `Answer sent to ${answerSentTo}`,
          })}
        </span>
      </div>
    );
  }

  const cleanQuestion = truncateAgentQuestion(question);
  const keyLabel = shortcutKey.trim() || "fn";
  return (
    <div className="flex flex-col items-center gap-1.5">
      <div
        data-testid="pill-agent-ask"
        role="status"
        aria-live="polite"
        className="inline-flex max-w-[440px] items-center gap-2 rounded-full border border-mid-gray/35 bg-background px-4 py-2 shadow-sm text-[12.5px] font-medium text-text"
      >
        <span aria-hidden="true" className="font-bold text-logo-primary">
          {QUESTION_MARK}
        </span>
        <span className="truncate">
          {t("overlay.agentAsk.promptLine", {
            agent: agentName,
            question: cleanQuestion,
            key: keyLabel,
            defaultValue: `${agentName} asks: "${cleanQuestion}" · hold ${keyLabel} to answer`,
          })}
        </span>
      </div>
      {queuedCount > 0 ? (
        <div
          data-testid="pill-agent-ask-queued"
          className="inline-flex items-center gap-1.5 rounded-full border border-mid-gray/25 bg-background/95 px-3 py-1 text-[11.5px] text-mid-gray shadow-xs"
        >
          <span aria-hidden="true">{QUESTION_MARK}</span>
          <span>
            {queuedCount === 1
              ? t("overlay.agentAsk.oneMoreWaiting", "1 more question waiting")
              : t("overlay.agentAsk.manyMoreWaiting", {
                  count: queuedCount,
                  defaultValue: `${queuedCount} more questions waiting`,
                })}
          </span>
        </div>
      ) : null}
    </div>
  );
};

export const RecordingPillShowcase: React.FC = () => {
  const { t } = useTranslation();
  const rows: Array<{
    kind: RecordingPillKind;
    streamingWords?: string;
    targetApp?: string;
    caption: string;
  }> = [
    {
      kind: "listening",
      caption: t(
        "overlay.pillStates.captionListening",
        "Default: the wave, no words",
      ),
    },
    {
      kind: "streaming",
      streamingWords: t(
        "overlay.pillStates.sampleStream",
        '"can we move the review to…"',
      ),
      caption: t(
        "overlay.pillStates.captionStreaming",
        "Live words, only with a model that streams",
      ),
    },
    {
      kind: "working",
      caption: t(
        "overlay.pillStates.captionWorking",
        "Turning speech into text",
      ),
    },
    {
      kind: "sent",
      targetApp: "Mail",
      caption: t("overlay.pillStates.captionSent", "Typed into the app"),
    },
    {
      kind: "clipboard",
      caption: t(
        "overlay.pillStates.captionClipboard",
        "Typing is off: press ⌘V",
      ),
    },
    {
      kind: "kept_press_paste",
      caption: t(
        "overlay.pillStates.captionKept",
        "Pasting failed, nothing lost",
      ),
    },
  ];

  return (
    <div
      data-testid="recording-pill-showcase"
      className="grid gap-4 p-7 content-start"
    >
      {rows.map((row) => (
        <div key={row.kind} className="flex flex-wrap items-center gap-3.5">
          <RecordingPillStateView
            kind={row.kind}
            streamingWords={row.streamingWords}
            targetApp={row.targetApp}
          />
          <span className="text-[12.5px] text-mid-gray">{row.caption}</span>
        </div>
      ))}
    </div>
  );
};
