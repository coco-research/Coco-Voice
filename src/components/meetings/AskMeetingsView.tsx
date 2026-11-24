import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { commands, type MeetingTodoItem } from "../../bindings.meetings";
import { formatNoteTime } from "./MeetingNotesPanel";

const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
const MID_SEP = " · ";
const MENU_CARET = "▾";

export type AskScope = "this_meeting" | "all" | "folder" | "picked";

export interface AskCitation {
  meetingId?: number;
  segmentId?: number;
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
    if (c.meetingTitle.endsWith(`${MID_SEP}${time}`)) {
      return `[${c.meetingTitle}]`;
    }
    return `[${c.meetingTitle}${MID_SEP}${time}]`;
  }
  return `[${time}]`;
}

export interface AskMeetingsSectionProps {
  singleMeetingMode?: boolean;
  meetingTitle?: string;
  scope: AskScope;
  turns: AskTurn[];
  answering?: boolean;
  voiceListening?: boolean;
  voiceError?: string | null;
  draftValue?: string;
  availableFolders?: string[];
  selectedFolder?: string;
  availableMeetings?: Array<{ id: number; title: string }>;
  pickedMeetingIds?: number[];
  onSelectFolder?: (folder: string) => void;
  onTogglePickedMeeting?: (id: number) => void;
  onDraftChange?: (draft: string) => void;
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

function scopeLabelKey(scope: AskScope, singleMeetingMode: boolean): string {
  if (singleMeetingMode || scope === "this_meeting") {
    return "meetings.ask.scopeThisMeeting";
  }
  if (scope === "folder") return "meetings.ask.scopeFolder";
  if (scope === "picked") return "meetings.ask.scopePicked";
  return "meetings.ask.scopeAll";
}

export const AskMeetingsSection: React.FC<AskMeetingsSectionProps> = ({
  singleMeetingMode = false,
  meetingTitle,
  scope,
  turns,
  answering = false,
  voiceListening = false,
  voiceError = null,
  draftValue,
  availableFolders = [],
  selectedFolder = "",
  availableMeetings = [],
  pickedMeetingIds = [],
  onSelectFolder,
  onTogglePickedMeeting,
  onDraftChange,
  onScopeChange,
  onAsk,
  onToggleVoiceAsk,
  onJumpToCitation,
}) => {
  const { t } = useTranslation();
  const [internalDraft, setInternalDraft] = useState("");
  const [scopeMenuOpen, setScopeMenuOpen] = useState(false);
  const draft = draftValue ?? internalDraft;
  const updateDraft = (next: string) => {
    setInternalDraft(next);
    onDraftChange?.(next);
  };

  const testId = singleMeetingMode
    ? "frame-6-ask-single"
    : "frame-13-ask-across";
  const singleHeadingText = t("meetings.ask.singleHeading", "Ask this meeting");
  const heading = singleMeetingMode
    ? meetingTitle?.trim() || singleHeadingText
    : t("meetings.ask.acrossHeading");
  const subtitle = singleMeetingMode
    ? t("meetings.ask.singleSubtitle", "Questions use this meeting only.")
    : t(
        "meetings.ask.acrossSubtitle",
        "Questions can use every meeting on this Mac.",
      );
  const placeholder = singleMeetingMode
    ? t("meetings.ask.placeholderSingle")
    : t("meetings.ask.placeholderAcross");

  const submit = () => {
    const q = draft.trim();
    if (!q || answering) return;
    onAsk(q);
    updateDraft("");
  };

  return (
    <div data-testid={testId} className="max-w-3xl w-full mx-auto space-y-3.5">
      <p className={EYEBROW}>
        {singleMeetingMode && meetingTitle
          ? `${t("meetings.sidebar")}${MID_SEP}${singleHeadingText}`
          : t("meetings.sidebar")}
      </p>
      <div>
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {heading}
        </h2>
        <p className="mt-0.5 text-[13px] text-mid-gray">{subtitle}</p>
      </div>

      {!singleMeetingMode &&
      scope === "folder" &&
      availableFolders.length > 0 ? (
        <div className="flex flex-wrap items-center gap-2 rounded-[10px] border border-mid-gray/20 bg-mid-gray/[0.03] px-3 py-2 text-[12.5px]">
          <span className="text-mid-gray">
            {t("meetings.ask.chooseFolder", "Folder:")}
          </span>
          <select
            aria-label={t("meetings.ask.chooseFolder", "Folder:")}
            value={selectedFolder}
            onChange={(e) => onSelectFolder?.(e.target.value)}
            className="rounded-[8px] border border-mid-gray/35 bg-background px-2.5 py-1 text-[12.5px] text-text"
          >
            {availableFolders.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </div>
      ) : null}

      {!singleMeetingMode &&
      scope === "picked" &&
      availableMeetings.length > 0 ? (
        <div className="flex flex-wrap gap-1.5 rounded-[10px] border border-mid-gray/20 bg-mid-gray/[0.03] p-2.5">
          {availableMeetings.map((m) => {
            const picked = pickedMeetingIds.includes(m.id);
            return (
              <button
                key={m.id}
                type="button"
                aria-pressed={picked}
                onClick={() => onTogglePickedMeeting?.(m.id)}
                className={`rounded-full px-2.5 py-0.5 text-[12px] font-medium cursor-pointer ${
                  picked
                    ? "bg-logo-primary text-white"
                    : "border border-mid-gray/35 bg-background text-text"
                }`}
              >
                {m.title}
              </button>
            );
          })}
        </div>
      ) : null}

      {turns.length > 0 ? (
        <div className="space-y-3 rounded-[14px] border border-mid-gray/25 bg-background p-4">
          {turns.map((turn) => {
            const validCitations = filterValidCitations(turn.citations);
            return (
              <div
                key={turn.id}
                className="space-y-2.5 border-b border-mid-gray/15 pb-3 last:border-b-0 last:pb-0"
              >
                <div className="space-y-0.5">
                  <p className="text-[11.5px] font-semibold text-mid-gray">
                    {t("meetings.view.channel.me", "Me")}
                  </p>
                  <p className="text-[13.5px] leading-[20px] text-text">
                    {turn.question}
                  </p>
                </div>
                <div className="space-y-1 rounded-[10px] bg-mid-gray/[0.04] px-3 py-2.5">
                  <p className="text-[11.5px] font-semibold text-logo-primary">
                    {t("meetings.ask.assistantSpeaker", "Coco Voice")}
                  </p>
                  <p className="text-[13.5px] leading-[20px] text-text whitespace-pre-line">
                    {turn.answer}
                  </p>
                  {validCitations.length > 0 ? (
                    <div className="flex flex-wrap gap-1.5 pt-1">
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
              </div>
            );
          })}
        </div>
      ) : null}

      <div className="rounded-[14px] border border-mid-gray/25 bg-background p-3.5 space-y-2.5">
        <textarea
          value={draft}
          onChange={(e) => updateDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder={placeholder}
          aria-label={heading}
          rows={2}
          className="w-full rounded-[10px] border border-mid-gray/35 bg-background px-3 py-2 text-[13px] text-text"
        />
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="relative flex items-center gap-2.5">
            {onScopeChange && !singleMeetingMode ? (
              <button
                type="button"
                data-testid="ask-scope-chip"
                aria-expanded={scopeMenuOpen}
                onClick={() => setScopeMenuOpen((v) => !v)}
                className="inline-flex items-center gap-1 rounded-full border border-mid-gray/35 px-2.5 py-0.5 text-[12px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
              >
                <span>{t(scopeLabelKey(scope, singleMeetingMode))}</span>
                <span aria-hidden="true">{MENU_CARET}</span>
              </button>
            ) : (
              <span className="text-[12px] text-mid-gray">
                {t(scopeLabelKey(scope, singleMeetingMode))}
              </span>
            )}
            {scopeMenuOpen && onScopeChange && !singleMeetingMode ? (
              <div
                role="menu"
                className="absolute bottom-8 left-0 z-20 min-w-[200px] rounded-[10px] border border-mid-gray/30 bg-background p-1 shadow-lg"
              >
                {ACROSS_SCOPES.map(([id, key]) => (
                  <button
                    key={id}
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      onScopeChange(id);
                      setScopeMenuOpen(false);
                    }}
                    className="block w-full rounded-[7px] px-2.5 py-1.5 text-left text-[12.5px] text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t(key)}
                  </button>
                ))}
              </div>
            ) : null}
            <button
              type="button"
              data-testid="ask-by-voice-btn"
              aria-label={t("meetings.ask.voiceAskAria")}
              aria-pressed={voiceListening}
              onClick={onToggleVoiceAsk}
              className={`inline-flex min-h-[28px] items-center gap-1.5 rounded-[8px] border px-2.5 text-[12px] font-medium cursor-pointer ${
                voiceListening
                  ? "border-logo-primary bg-logo-primary/10 text-logo-primary"
                  : "border-mid-gray/40 text-mid-gray hover:text-text hover:bg-mid-gray/10"
              }`}
            >
              <Mic className="h-3.5 w-3.5" aria-hidden="true" />
              <span>
                {voiceListening
                  ? t("meetings.ask.listeningQuestion")
                  : t("meetings.ask.voiceAskAria")}
              </span>
            </button>
          </div>
          <button
            type="button"
            disabled={answering}
            onClick={submit}
            className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-4 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90 disabled:opacity-50"
          >
            {answering
              ? t("meetings.ask.answering", "Answering…")
              : t("meetings.ask.askButton")}
          </button>
        </div>
      </div>

      {voiceError ? (
        <p
          role="alert"
          data-testid="ask-voice-error"
          className="text-[12px] text-red-600"
        >
          {voiceError}
        </p>
      ) : null}

      <p className="text-[12px] text-mid-gray">
        {t("meetings.ask.citationRule")}
      </p>
    </div>
  );
};

export function resolveMicPermissionErrorKey(platformHint?: string): string {
  const hint =
    platformHint ??
    (typeof navigator !== "undefined"
      ? `${navigator.platform ?? ""} ${navigator.userAgent ?? ""}`.trim()
      : "Mac");
  if (/win/i.test(hint)) return "errors.micPermissionDenied.windows";
  if (/linux/i.test(hint)) return "errors.micPermissionDenied.linux";
  if (/mac/i.test(hint)) return "errors.micPermissionDenied.macos";
  return "errors.micPermissionDenied.generic";
}

export function buildAskMeetingsV2Params(
  question: string,
  opts: {
    isSingle: boolean;
    scope: AskScope;
    meetingId?: number;
    selectedFolder: string;
    pickedMeetingIds: number[];
    recentTurns: AskTurn[];
  },
) {
  return {
    question: question.trim(),
    scope: opts.isSingle ? "this" : opts.scope,
    meetingId: opts.isSingle ? (opts.meetingId ?? null) : null,
    folderName:
      !opts.isSingle && opts.scope === "folder" ? opts.selectedFolder : null,
    meetingIds:
      !opts.isSingle && opts.scope === "picked" ? opts.pickedMeetingIds : null,
    history: opts.recentTurns.slice(-2).map((turn) => ({
      question: turn.question,
      answer: turn.answer,
    })),
  };
}

export interface AskByVoiceControllerDeps {
  startAskByVoice: () => Promise<
    { status: "ok"; data: null } | { status: "error"; error: string }
  >;
  finishAskByVoice: () => Promise<
    { status: "ok"; data: string } | { status: "error"; error: string }
  >;
  cancelAskByVoice: () => Promise<
    { status: "ok"; data: null } | { status: "error"; error: string }
  >;
  onListeningChange: (listening: boolean) => void;
  onVoiceErrorKeyChange: (key: string | null) => void;
  onSpokenQuestion: (spoken: string) => void | Promise<void>;
  platformHint?: string;
}

export function createAskByVoiceController(deps: AskByVoiceControllerDeps) {
  let listening = false;
  let busy = false;

  return {
    isListening: () => listening,
    async toggle(): Promise<void> {
      if (busy) return;
      busy = true;
      try {
        if (!listening) {
          deps.onVoiceErrorKeyChange(null);
          const res = await deps.startAskByVoice();
          if (res.status === "ok") {
            listening = true;
            deps.onListeningChange(true);
          } else {
            listening = false;
            deps.onListeningChange(false);
            deps.onVoiceErrorKeyChange(
              resolveMicPermissionErrorKey(deps.platformHint),
            );
          }
          return;
        }

        listening = false;
        deps.onListeningChange(false);
        const res = await deps.finishAskByVoice();
        if (res.status === "ok") {
          const spoken = res.data.trim();
          if (spoken.length > 0) {
            await deps.onSpokenQuestion(spoken);
          }
        } else {
          deps.onVoiceErrorKeyChange(
            resolveMicPermissionErrorKey(deps.platformHint),
          );
        }
      } finally {
        busy = false;
      }
    },
    async cancel(): Promise<boolean> {
      if (!listening) return false;
      listening = false;
      deps.onListeningChange(false);
      await deps.cancelAskByVoice();
      return true;
    },
  };
}

export const AskMeetingsView: React.FC<{
  meetingId?: number;
  initialMeetingTitle?: string;
  onJumpToCitation?: (c: AskCitation) => void;
}> = ({ meetingId, initialMeetingTitle, onJumpToCitation }) => {
  const { t } = useTranslation();
  const isSingle = meetingId !== undefined;
  const [scope, setScope] = useState<AskScope>(
    isSingle ? "this_meeting" : "all",
  );
  const [turns, setTurns] = useState<AskTurn[]>([]);
  const [answering, setAnswering] = useState(false);
  const [listening, setListening] = useState(false);
  const [voiceErrorKey, setVoiceErrorKey] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [availableFolders, setAvailableFolders] = useState<string[]>([]);
  const [selectedFolder, setSelectedFolder] = useState("");
  const [availableMeetings, setAvailableMeetings] = useState<
    Array<{ id: number; title: string }>
  >([]);
  const [pickedMeetingIds, setPickedMeetingIds] = useState<number[]>([]);
  const draftRef = useRef("");
  const turnsRef = useRef<AskTurn[]>([]);

  useEffect(() => {
    turnsRef.current = turns;
  }, [turns]);

  useEffect(() => {
    if (isSingle) return;
    let cancelled = false;
    void commands.listMeetingFolders().then((res) => {
      if (!cancelled && res.status === "ok") {
        const names = res.data.map((f) => f.name);
        setAvailableFolders(names);
        if (names[0]) setSelectedFolder((prev) => prev || names[0]);
      }
    });
    void invoke<Array<{ id: number; title: string }>>("list_meetings")
      .then((rows) => {
        if (!cancelled && Array.isArray(rows)) {
          setAvailableMeetings(rows.map((r) => ({ id: r.id, title: r.title })));
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [isSingle]);

  const runAsk = useCallback(
    (q: string, options?: { keepDraft?: boolean }) => {
      const trimmed = q.trim();
      if (!trimmed) return;
      if (options?.keepDraft) {
        setDraft(trimmed);
        draftRef.current = trimmed;
      } else {
        setDraft("");
        draftRef.current = "";
      }
      setAnswering(true);
      const params = buildAskMeetingsV2Params(trimmed, {
        isSingle,
        scope,
        meetingId,
        selectedFolder,
        pickedMeetingIds,
        recentTurns: turnsRef.current,
      });
      void commands
        .askMeetingsV2(params)
        .then((res) => {
          setAnswering(false);
          if (res.status === "ok") {
            const nextTurn: AskTurn = {
              id: `t-${Date.now()}`,
              question: trimmed,
              answer: res.data.answer,
              citations: res.data.citations.map((c) => ({
                meetingId: c.meeting_id,
                segmentId: c.segment_id,
                meetingTitle: isSingle ? undefined : c.meeting_title,
                meetingDate: isSingle ? undefined : c.date || undefined,
                start_ms: c.at_ms,
              })),
            };
            setTurns((prev) => [...prev, nextTurn]);
          }
        })
        .catch(() => {
          setAnswering(false);
        });
    },
    [isSingle, meetingId, scope, selectedFolder, pickedMeetingIds],
  );

  const runAskRef = useRef(runAsk);
  useEffect(() => {
    runAskRef.current = runAsk;
  }, [runAsk]);

  const voiceControllerRef = useRef<ReturnType<
    typeof createAskByVoiceController
  > | null>(null);
  if (!voiceControllerRef.current) {
    voiceControllerRef.current = createAskByVoiceController({
      startAskByVoice: () => commands.startAskByVoice(),
      finishAskByVoice: () => commands.finishAskByVoice(),
      cancelAskByVoice: () => commands.cancelAskByVoice(),
      onListeningChange: setListening,
      onVoiceErrorKeyChange: setVoiceErrorKey,
      onSpokenQuestion: (spoken) => {
        runAskRef.current(spoken, { keepDraft: true });
      },
    });
  }

  useEffect(() => {
    if (!listening) return;
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        void voiceControllerRef.current?.cancel();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [listening]);

  useEffect(() => {
    return () => {
      void voiceControllerRef.current?.cancel();
    };
  }, []);

  useEffect(() => {
    const onDocClick = (ev: MouseEvent) => {
      const target = ev.target as HTMLElement | null;
      const navAsk = target?.closest?.('[data-nav-id="meetings-ask"]');
      if (navAsk && draftRef.current.trim().length > 0) {
        runAsk(draftRef.current);
      }
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, [runAsk]);

  return (
    <AskMeetingsSection
      singleMeetingMode={isSingle}
      meetingTitle={initialMeetingTitle}
      scope={scope}
      turns={turns}
      answering={answering}
      voiceListening={listening}
      voiceError={voiceErrorKey ? t(voiceErrorKey) : null}
      draftValue={draft}
      availableFolders={availableFolders}
      selectedFolder={selectedFolder}
      availableMeetings={availableMeetings}
      pickedMeetingIds={pickedMeetingIds}
      onSelectFolder={setSelectedFolder}
      onTogglePickedMeeting={(id) =>
        setPickedMeetingIds((prev) =>
          prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
        )
      }
      onDraftChange={(val) => {
        setDraft(val);
        draftRef.current = val;
      }}
      onScopeChange={isSingle ? undefined : setScope}
      onToggleVoiceAsk={() => {
        void voiceControllerRef.current?.toggle();
      }}
      onAsk={runAsk}
      onJumpToCitation={onJumpToCitation}
    />
  );
};

export function formatTodoSourceLine(
  item: Pick<
    MeetingTodoItem,
    "meeting_id" | "meeting_title" | "meeting_date" | "said_by_me"
  >,
  prevMeetingId: number | null,
  t: (
    key: string,
    opts?: { defaultValue?: string; title?: string; date?: string },
  ) => string,
): string {
  const sameMeetingAsPrev =
    prevMeetingId !== null && prevMeetingId === item.meeting_id;
  if (item.said_by_me) {
    return sameMeetingAsPrev || !item.meeting_date
      ? t("meetings.todos.youSaidInNoDate", {
          defaultValue: "You said it in {{title}}",
          title: item.meeting_title,
        })
      : t("meetings.todos.youSaidIn", {
          defaultValue: "You said it in {{title}}, {{date}}",
          title: item.meeting_title,
          date: item.meeting_date,
        });
  }
  return sameMeetingAsPrev || !item.meeting_date
    ? item.meeting_title
    : `${item.meeting_title}, ${item.meeting_date}`;
}

export type MeetingTodosLoadResult = {
  items: MeetingTodoItem[];
  loadFailed: boolean;
};

export async function loadMeetingTodos(
  listTodos: () => Promise<
    | { status: "ok"; data: MeetingTodoItem[] }
    | { status: "error"; error: string }
  > = () => commands.listMeetingTodos(),
): Promise<MeetingTodosLoadResult> {
  try {
    const res = await listTodos();
    if (res.status === "ok") {
      return { items: res.data, loadFailed: false };
    }
    return { items: [], loadFailed: true };
  } catch {
    return { items: [], loadFailed: true };
  }
}

export const MeetingTodosPanel: React.FC<{
  items?: MeetingTodoItem[];
  loadFailed?: boolean;
  onComplete?: (id: string) => void;
  onOpenMoment?: (item: MeetingTodoItem) => void;
}> = ({
  items: propItems,
  loadFailed: propLoadFailed,
  onComplete,
  onOpenMoment,
}) => {
  const { t } = useTranslation();
  const [localItems, setLocalItems] = useState<MeetingTodoItem[]>([]);
  const [localLoadFailed, setLocalLoadFailed] = useState(false);

  useEffect(() => {
    if (propItems || propLoadFailed !== undefined) return;
    let cancelled = false;
    void loadMeetingTodos().then((result) => {
      if (!cancelled) {
        setLocalItems(result.items);
        setLocalLoadFailed(result.loadFailed);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [propItems, propLoadFailed]);

  const items = propItems ?? localItems;
  const loadFailed = propLoadFailed ?? localLoadFailed;
  const markDone = (id: string) => {
    if (onComplete) {
      onComplete(id);
    } else {
      setLocalItems((prev) => prev.filter((item) => item.id !== id));
      void commands.completeMeetingTodo(id, true);
    }
  };

  return (
    <section
      data-testid="meetings-todos-panel"
      className="max-w-3xl w-full mx-auto space-y-3"
    >
      <div>
        <p className={EYEBROW}>{t("meetings.sidebar")}</p>
        <h3 className="mt-1 text-[20px] font-semibold tracking-[-0.02em] text-text">
          {t("meetings.ask.todosTab")}
        </h3>
        <p className="mt-0.5 text-[12.5px] text-mid-gray">
          {t("meetings.agent.todosSubtitle")}
        </p>
      </div>

      {loadFailed ? (
        <p
          role="alert"
          data-testid="meetings-todos-error"
          className="text-[12px] text-red-600"
        >
          {t("meetings.todos.loadFailed", "Could not load to-dos.")}
        </p>
      ) : items.length > 0 ? (
        <ul className="divide-y divide-mid-gray/20 rounded-[14px] border border-mid-gray/25 bg-background px-4">
          {items.map((item, idx) => {
            const prevMeetingId = idx > 0 ? items[idx - 1].meeting_id : null;
            const sourceLine = formatTodoSourceLine(item, prevMeetingId, t);
            const badge = formatNoteTime(item.start_ms);
            return (
              <li
                key={item.id}
                className="flex flex-wrap items-center justify-between gap-2 py-3"
              >
                <div className="min-w-0 flex-1 space-y-0.5">
                  <p className="text-[13.5px] font-medium text-text">
                    {item.text}
                  </p>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[12px] text-mid-gray">
                      {sourceLine}
                    </span>
                    <button
                      type="button"
                      onClick={() => onOpenMoment?.(item)}
                      className="font-mono text-[12px] font-semibold text-logo-primary cursor-pointer hover:underline"
                    >
                      {badge}
                    </button>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => markDone(item.id)}
                  className="shrink-0 rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("meetings.agent.todosDone")}
                </button>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="rounded-[14px] border border-mid-gray/25 bg-background p-4 text-[13px] text-mid-gray">
          {t("meetings.todos.empty", "No to-dos from the last 14 days.")}
        </p>
      )}

      <p className="text-[12px] text-mid-gray">
        {t("meetings.agent.todosFooter")}
      </p>
    </section>
  );
};
