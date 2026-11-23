import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands, type HistoryEntry } from "@/bindings";
import { learnCorrectionsFromEdit } from "@/lib/utils/autoLearnCorrections";
import { OUTLINED_BTN_SM } from "../../meetings/MeetingsList";

const QUOTE = '"';
const MID_SEP = " · ";
const TIME_OPTS: Intl.DateTimeFormatOptions = {
  hour: "numeric",
  minute: "2-digit",
  hour12: false,
};
const SHORT_DATE_OPTS: Intl.DateTimeFormatOptions = {
  day: "numeric",
  month: "short",
};
const MENU_ITEM_CLS =
  "block w-full rounded-md px-2.5 py-1.5 text-start text-[13px] font-medium text-text hover:bg-mid-gray/15 disabled:opacity-50 cursor-pointer";

export const displayTextFor = (e: HistoryEntry): string =>
  e.post_processed_text && e.post_processed_text.trim().length > 0
    ? e.post_processed_text
    : e.transcription_text;

function sameCalendarDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function capFirst(value: string): string {
  return value ? value.charAt(0).toUpperCase() + value.slice(1) : value;
}

export function formatDictationTime(
  timestampSec: number,
  lang: string,
  nowSec: number = Math.floor(Date.now() / 1000),
): string {
  const date = new Date(timestampSec * 1000);
  const now = new Date(nowSec * 1000);
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);

  const rawClock = date.toLocaleTimeString(lang, TIME_OPTS);
  const clock = rawClock.replace(/^0(?=\d:)/, "");
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });

  if (sameCalendarDay(date, now)) {
    return `${capFirst(rtf.format(0, "day"))} ${clock}`;
  }
  if (sameCalendarDay(date, yesterday)) {
    return `${capFirst(rtf.format(-1, "day"))} ${clock}`;
  }
  return `${date.toLocaleDateString(lang, SHORT_DATE_OPTS)} ${clock}`;
}

function isDictationEntry(x: unknown): x is HistoryEntry {
  const o =
    typeof x === "object" && x !== null ? (x as Record<string, unknown>) : null;
  return Boolean(
    o &&
    typeof o.transcription_text === "string" &&
    !("notes_ready" in o) &&
    !("started_at" in o),
  );
}

export function selectDictationEntries(
  records: readonly unknown[],
  query: string,
): HistoryEntry[] {
  const q = query.trim().toLowerCase();
  const takes = records.filter(isDictationEntry);
  return q
    ? takes.filter((e) =>
        `${displayTextFor(e)} ${e.app_name ?? ""} ${e.title}`
          .toLowerCase()
          .includes(q),
      )
    : takes;
}

export function createHistoryCommandHandlers(opts: {
  getEntries: () => HistoryEntry[];
  setEntries: (fn: (prev: HistoryEntry[]) => HistoryEntry[]) => void;
  reload?: () => void;
  t?: (key: string, params?: Record<string, unknown>) => string;
}) {
  const tr = opts.t ?? ((k: string) => k);

  const toggleSaved = async (id: number) => {
    opts.setEntries((prev) =>
      prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
    );
    try {
      const result = await commands.toggleHistoryEntrySaved(id);
      if (result.status !== "ok") {
        opts.setEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      }
    } catch {
      opts.setEntries((prev) =>
        prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
      );
    }
  };

  const deleteAudioEntry = async (id: number) => {
    opts.setEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteHistoryEntry(id);
      if (result.status !== "ok") opts.reload?.();
    } catch {
      opts.reload?.();
    }
  };

  const retryHistoryEntry = async (id: number) => {
    const result = await commands.retryHistoryEntryTranscription(id);
    if (result.status !== "ok") throw new Error(String(result.error));
  };

  const saveEditAndLearn = async (id: number, newText: string) => {
    const entry = opts.getEntries().find((e) => e.id === id);
    const original = entry ? displayTextFor(entry) : "";
    const usedPost = Boolean(entry?.post_processed_text?.trim());
    const trimmed = newText.trim();
    if (!trimmed || trimmed === original.trim()) return;
    const applyText = (e: HistoryEntry, text: string): HistoryEntry =>
      usedPost
        ? { ...e, post_processed_text: text }
        : { ...e, transcription_text: text };

    opts.setEntries((prev) =>
      prev.map((e) => (e.id === id ? applyText(e, trimmed) : e)),
    );
    try {
      const res = await commands.updateHistoryEntryText(id, trimmed);
      if (res.status !== "ok") {
        opts.setEntries((prev) =>
          prev.map((e) => (e.id === id ? applyText(e, original) : e)),
        );
        toast.error(tr("settings.history.editError"));
        return;
      }
    } catch {
      opts.setEntries((prev) =>
        prev.map((e) => (e.id === id ? applyText(e, original) : e)),
      );
      toast.error(tr("settings.history.editError"));
      return;
    }
    try {
      const settingsRes = await commands.getAppSettings();
      if (settingsRes.status !== "ok") return;
      const existing = settingsRes.data.corrections ?? [];
      const learned = learnCorrectionsFromEdit(original, trimmed, existing);
      if (learned.length === 0) return;
      const upd = await commands.updateCorrections([...existing, ...learned]);
      if (upd.status === "ok") {
        toast.success(
          learned.length === 1
            ? tr("settings.history.learnedOne", {
                from: learned[0].from,
                to: learned[0].to,
              })
            : tr("settings.history.learnedMany", { count: learned.length }),
        );
      }
    } catch {
      // Ignore correction persistence errors after transcript edit succeeded.
    }
  };

  const openRecordingsFolder = async () => {
    const result = await commands.openRecordingsFolder();
    if (result.status !== "ok") throw new Error(String(result.error));
  };

  return {
    toggleSaved,
    deleteAudioEntry,
    retryHistoryEntry,
    saveEditAndLearn,
    openRecordingsFolder,
  };
}

export interface HistoryRowProps {
  entry: HistoryEntry;
  nowTimestamp?: number;
  menuOpen: boolean;
  editing: boolean;
  editDraft: string;
  savingEdit?: boolean;
  onMenuToggle: () => void;
  onMenuClose: () => void;
  onToggleSaved: () => void;
  onCopyText: () => void;
  onStartEdit: () => void;
  onEditDraftChange: (value: string) => void;
  onCancelEdit: () => void;
  onCommitEdit: () => Promise<void>;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
  retryTranscription: (id: number) => Promise<void>;
}

export const HistoryRow: React.FC<HistoryRowProps> = (p) => {
  const { t, i18n } = useTranslation();
  const [playing, setPlaying] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(
    () => () => {
      audioRef.current?.pause();
      audioRef.current = null;
    },
    [],
  );

  const hasText = p.entry.transcription_text.trim().length > 0;
  const lineOne = retrying
    ? t("settings.history.transcribing")
    : hasText
      ? `${QUOTE}${displayTextFor(p.entry)}${QUOTE}`
      : t("settings.history.transcriptionFailed");
  const appName = p.entry.app_name?.trim() || "";
  const timeLabel = formatDictationTime(
    p.entry.timestamp,
    i18n.language,
    p.nowTimestamp,
  );
  const isCleaned = Boolean(p.entry.post_processed_text?.trim());
  const parts = [timeLabel];
  if (isCleaned) parts.push(t("sidebar.cleanedUp"));
  if (p.entry.detected_language?.trim()) parts.push(p.entry.detected_language.trim().toUpperCase());
  if (p.entry.saved) parts.push(t("sidebar.kept"));
  const metaTail = parts.join(MID_SEP);

  const handlePlay = async () => {
    if (playing && audioRef.current) {
      audioRef.current.pause();
      setPlaying(false);
      return;
    }
    const url = await p.getAudioUrl(p.entry.file_name);
    if (!url) return;
    const audio = new Audio(url);
    audioRef.current = audio;
    audio.onended = () => setPlaying(false);
    audio.onerror = () => setPlaying(false);
    setPlaying(true);
    void audio.play().catch(() => setPlaying(false));
  };

  return (
    <div data-history-row-id={p.entry.id} className="py-3">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0 flex-1">
          {p.editing ? (
            <div className="flex flex-col gap-2">
              <textarea
                aria-label={t("sidebar.editText")}
                value={p.editDraft}
                onChange={(e) => p.onEditDraftChange(e.target.value)}
                disabled={p.savingEdit}
                rows={2}
                className="w-full rounded-[10px] border border-mid-gray/40 bg-background px-2.5 py-1.5 text-sm text-text"
              />
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void p.onCommitEdit()}
                  disabled={p.savingEdit || !p.editDraft.trim()}
                  className={OUTLINED_BTN_SM}
                >
                  {t("settings.history.saveEdit")}
                </button>
                <button
                  type="button"
                  onClick={p.onCancelEdit}
                  disabled={p.savingEdit}
                  className={OUTLINED_BTN_SM}
                >
                  {t("tray.cancel")}
                </button>
              </div>
            </div>
          ) : (
            <p className="text-[14px] font-medium text-text break-words">
              {lineOne}
            </p>
          )}
          <p className="mt-0.5 text-[13px] text-mid-gray">
            {appName ? (
              <>
                <span className="font-mono text-[12px] font-semibold text-text">
                  {appName}
                </span>
                {MID_SEP}
                {metaTail}
              </>
            ) : (
              metaTail
            )}
          </p>
        </div>
        <div className="relative flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={p.onCopyText}
            disabled={!hasText || retrying}
            className={OUTLINED_BTN_SM}
          >
            {t("common.copy")}
          </button>
          <button
            type="button"
            onClick={() => void handlePlay()}
            className={OUTLINED_BTN_SM}
          >
            {playing ? t("common.pause") : t("common.play")}
          </button>
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={p.menuOpen}
            onClick={p.onMenuToggle}
            className={OUTLINED_BTN_SM}
          >
            {t("sidebar.more")}
          </button>
          {p.menuOpen ? (
            <div
              role="menu"
              className="absolute end-0 top-full z-20 mt-1 min-w-[140px] rounded-[10px] border border-mid-gray/30 bg-background p-1 shadow-lg"
            >
              {buildHistoryRowMenuItems(
                p,
                t,
                hasText,
                retrying,
                setRetrying,
              ).map((item) => (
                <button
                  key={item.id}
                  type="button"
                  role="menuitem"
                  disabled={item.disabled}
                  onClick={item.onSelect}
                  className={MENU_ITEM_CLS}
                >
                  {item.label}
                </button>
              ))}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
};

export function buildHistoryRowMenuItems(
  p: Pick<
    HistoryRowProps,
    | "entry"
    | "onMenuClose"
    | "onToggleSaved"
    | "onStartEdit"
    | "retryTranscription"
    | "deleteAudio"
  >,
  t: (key: string) => string,
  hasText: boolean,
  retrying: boolean,
  setRetrying: (v: boolean) => void,
) {
  return [
    {
      id: "keep",
      label: t("sidebar.keep"),
      disabled: false,
      onSelect: () => {
        p.onMenuClose();
        p.onToggleSaved();
      },
    },
    {
      id: "edit",
      label: t("sidebar.editText"),
      disabled: !hasText || retrying,
      onSelect: () => {
        p.onMenuClose();
        p.onStartEdit();
      },
    },
    {
      id: "retry",
      label: t("sidebar.tryAgain"),
      disabled: retrying,
      onSelect: () => {
        p.onMenuClose();
        setRetrying(true);
        void p
          .retryTranscription(p.entry.id)
          .catch(() => toast.error(t("settings.history.retranscribeError")))
          .finally(() => setRetrying(false));
      },
    },
    {
      id: "delete",
      label: t("meetings.list.delete"),
      disabled: retrying,
      onSelect: () => {
        p.onMenuClose();
        void p
          .deleteAudio(p.entry.id)
          .catch(() => toast.error(t("settings.history.deleteError")));
      },
    },
  ];
}
