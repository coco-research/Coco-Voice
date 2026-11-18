import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic } from "lucide-react";
import { commands, type MeetingTodoItem } from "../../bindings.meetings";
import { formatNoteTime } from "./MeetingNotesPanel";

const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
const MID_SEP = " · ";

export type AskScope = "this_meeting" | "all" | "folder" | "picked";

export interface AskCitation {
  meetingTitle?: string;
  meetingDate?: string;
  start_ms: number;
}

export interface AskTurn {
  id: string;
  question: string;
  answer: string;
  citations: AskCitation[];
}

export function filterValidCitations(citations: AskCitation[]): AskCitation[] {
  return citations.filter(
    (c) => typeof c.start_ms === "number" && Number.isFinite(c.start_ms),
  );
}

export function formatAskCitationBadge(c: AskCitation): string {
  const time = formatNoteTime(c.start_ms);
  if (c.meetingTitle && c.meetingDate) {
    return `[${c.meetingTitle}, ${c.meetingDate}${MID_SEP}${time}]`;
  }
  if (c.meetingTitle) {
    return `[${c.meetingTitle}${MID_SEP}${time}]`;
  }
  return `[${time}]`;
}

export interface AskMeetingsSectionProps {
  singleMeetingMode?: boolean;
  scope: AskScope;
  turns: AskTurn[];
  voiceListening?: boolean;
  onScopeChange?: (scope: AskScope) => void;
  onAsk: (question: string) => void;
  onToggleVoiceAsk: () => void;
  onJumpToCitation?: (c: AskCitation) => void;
}

const ACROSS_SCOPES: Array<[AskScope, string]> = [
  ["all", "meetings.ask.scopeAll"],
  ["folder", "meetings.ask.scopeFolder"],
  ["picked", "meetings.ask.scopePicked"],
];

export const AskMeetingsSection: React.FC<AskMeetingsSectionProps> = ({
  singleMeetingMode = false,
  scope,
  turns,
  voiceListening = false,
  onScopeChange,
  onAsk,
  onToggleVoiceAsk,
  onJumpToCitation,
}) => {
  const { t } = useTranslation();
  const [draft, setDraft] = useState("");
  const testId = singleMeetingMode
    ? "frame-6-ask-single"
    : "frame-13-ask-across";
  const heading = singleMeetingMode
    ? t("meetings.ask.singleHeading")
    : t("meetings.ask.acrossHeading");
  const placeholder = singleMeetingMode
    ? t("meetings.ask.placeholderSingle")
    : t("meetings.ask.placeholderAcross");

  const scopeButtons = ACROSS_SCOPES.map(([id, key]) => {
    const active = scope === id;
    return (
      <button
        key={id}
        type="button"
        aria-pressed={active}
        onClick={() => onScopeChange?.(id)}
        className={`rounded-full px-3 py-1 text-[12px] font-semibold cursor-pointer ${
          active
            ? "bg-logo-primary text-white"
            : "border border-mid-gray/35 text-text/80 hover:bg-mid-gray/10"
        }`}
      >
        {t(key)}
      </button>
    );
  });

  const submit = () => {
    const q = draft.trim();
    if (!q) return;
    onAsk(q);
    setDraft("");
  };

  return (
    <div data-testid={testId} className="max-w-3xl w-full mx-auto space-y-3.5">
      {!singleMeetingMode ? (
        <p className={EYEBROW}>{t("meetings.sidebar")}</p>
      ) : null}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-[20px] leading-[26px] font-semibold tracking-[-0.02em] text-text">
          {heading}
        </h2>
        {singleMeetingMode ? (
          <span className="rounded-full border border-mid-gray/35 px-2.5 py-0.5 text-[11.5px] font-medium text-mid-gray">
            {t("meetings.ask.scopeThisMeeting")}
          </span>
        ) : (
          <div className="flex flex-wrap items-center gap-1.5">
            {scopeButtons}
          </div>
        )}
      </div>

      <div className="space-y-2.5 rounded-[14px] border border-mid-gray/25 bg-background p-4">
        {turns.map((turn) => {
          const validCitations = filterValidCitations(turn.citations);
          return (
            <div
              key={turn.id}
              className="space-y-1.5 border-b border-mid-gray/15 pb-3 last:border-b-0 last:pb-0"
            >
              <p className="text-[13.5px] font-semibold text-text">
                {turn.question}
              </p>
              <p className="text-[13.5px] leading-[20px] text-text/90">
                {turn.answer}
              </p>
              {validCitations.length > 0 ? (
                <div className="flex flex-wrap gap-1.5 pt-0.5">
                  {validCitations.map((c, idx) => {
                    const badge = formatAskCitationBadge(c);
                    return (
                      <button
                        key={`${turn.id}-${c.start_ms}-${idx}`}
                        type="button"
                        onClick={() => onJumpToCitation?.(c)}
                        className="font-mono text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
                      >
                        {badge}
                      </button>
                    );
                  })}
                </div>
              ) : null}
            </div>
          );
        })}

        <div className="space-y-2 pt-1">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={placeholder}
            aria-label={heading}
            rows={2}
            className="w-full rounded-[10px] border border-mid-gray/35 bg-background px-3 py-2 text-[13px] text-text"
          />
          <div className="flex items-center justify-between gap-2">
            <button
              type="button"
              data-testid="ask-by-voice-btn"
              aria-label={t("meetings.ask.voiceAskAria")}
              aria-pressed={voiceListening}
              onClick={onToggleVoiceAsk}
              className={`inline-flex min-h-[32px] items-center gap-1.5 rounded-[9px] border px-3 text-[12.5px] font-semibold cursor-pointer ${
                voiceListening
                  ? "border-logo-primary bg-logo-primary/10 text-logo-primary"
                  : "border-mid-gray/45 text-text hover:bg-mid-gray/10"
              }`}
            >
              <Mic className="h-3.5 w-3.5" aria-hidden="true" />
              <span>
                {voiceListening
                  ? t("meetings.ask.listeningQuestion")
                  : t("meetings.ask.voiceAskAria")}
              </span>
            </button>
            <button
              type="button"
              onClick={submit}
              className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-4 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              {t("meetings.ask.askButton")}
            </button>
          </div>
        </div>
      </div>

      <p className="text-[12px] text-mid-gray">
        {t("meetings.ask.citationRule")}
      </p>
    </div>
  );
};

const DEFAULT_TURNS: AskTurn[] = [
  {
    id: "t-1",
    question: "What did we decide on the export format?",
    answer: "Export writes a single plain Markdown file per meeting.",
    citations: [
      {
        meetingTitle: "Product sync",
        meetingDate: "Tue",
        start_ms: 262_000,
      },
    ],
  },
];

export const AskMeetingsView: React.FC = () => {
  const [scope, setScope] = useState<AskScope>("all");
  const [turns, setTurns] = useState<AskTurn[]>(DEFAULT_TURNS);
  const [listening, setListening] = useState(false);

  return (
    <AskMeetingsSection
      scope={scope}
      turns={turns}
      voiceListening={listening}
      onScopeChange={setScope}
      onToggleVoiceAsk={() => setListening((v) => !v)}
      onAsk={(q) => {
        void commands.askMeetings(q, scope).then((res) => {
          if (res.status === "ok") {
            setTurns((prev) => [
              ...prev,
              {
                id: `t-${prev.length + 1}`,
                question: q,
                answer: res.data.answer,
                citations: res.data.citations.map((c) => ({
                  meetingTitle: c.label,
                  start_ms: c.start_ms,
                })),
              },
            ]);
          }
        });
      }}
    />
  );
};

const SAMPLE_TODOS: MeetingTodoItem[] = [
  {
    id: "todo-1",
    text: "Send empty-state copy draft by Thursday",
    meeting_id: 2,
    meeting_title: "Product sync",
    meeting_date: "Tue",
    start_ms: 588_000,
    done: false,
  },
];

export const MeetingTodosPanel: React.FC<{
  items?: MeetingTodoItem[];
  onComplete?: (id: string) => void;
  onOpenMoment?: (item: MeetingTodoItem) => void;
}> = ({ items: propItems, onComplete, onOpenMoment }) => {
  const { t } = useTranslation();
  const [localItems, setLocalItems] = useState<MeetingTodoItem[]>(SAMPLE_TODOS);

  useEffect(() => {
    if (propItems) return;
    let cancelled = false;
    void commands.listMeetingTodos().then((res) => {
      if (!cancelled && res.status === "ok") setLocalItems(res.data);
    });
    return () => {
      cancelled = true;
    };
  }, [propItems]);

  const rows = (propItems ?? localItems).filter((i) => !i.done);

  return (
    <section
      data-testid="meetings-todos-panel"
      className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-3"
    >
      <div>
        <h3 className="text-[16px] font-semibold text-text">
          {t("meetings.ask.todosTab")}
        </h3>
        <p className="text-[12px] text-mid-gray">
          {t("meetings.agent.todosSubtitle")}
        </p>
      </div>
      <ul className="divide-y divide-mid-gray/20">
        {rows.map((item) => {
          const cite = formatAskCitationBadge({
            meetingTitle: item.meeting_title,
            meetingDate: item.meeting_date,
            start_ms: item.start_ms,
          });
          return (
            <li
              key={item.id}
              className="flex flex-wrap items-center justify-between gap-2 py-2.5"
            >
              <div className="space-y-0.5">
                <p className="text-[13.5px] font-semibold text-text">
                  {item.text}
                </p>
                <button
                  type="button"
                  onClick={() => onOpenMoment?.(item)}
                  className="font-mono text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
                >
                  {cite}
                </button>
              </div>
              <button
                type="button"
                onClick={() => {
                  if (onComplete) onComplete(item.id);
                  else {
                    setLocalItems((prev) =>
                      prev.map((r) =>
                        r.id === item.id ? { ...r, done: true } : r,
                      ),
                    );
                    void commands.completeMeetingTodo(item.id, true);
                  }
                }}
                className="rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("meetings.agent.todosDone")}
              </button>
            </li>
          );
        })}
      </ul>
      <p className="text-[12px] text-mid-gray">
        {t("meetings.agent.todosFooter")}
      </p>
    </section>
  );
};
