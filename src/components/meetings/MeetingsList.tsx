import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

export interface MeetingListItem {
  id: number;
  title: string;
  started_at: number;
  ended_at: number | null;
  notes_ready: boolean;
}

interface MeetingDeleteResult {
  audio_left: boolean;
}

export const SEARCH_DEBOUNCE_MS = 200;

export const OUTLINED_BTN_SM =
  "inline-flex min-h-[30px] items-center justify-center rounded-[10px] border border-text/75 bg-transparent px-2.5 text-[13px] font-semibold text-text transition-colors cursor-pointer hover:bg-text hover:text-background disabled:opacity-50 disabled:cursor-not-allowed";

export const NOTES_READY_TONE = "text-emerald-700 dark:text-emerald-400";
const TRANSCRIPT_ONLY_TONE = "text-mid-gray";
const KIND_NOTES = "notes-ready";
const KIND_TRANSCRIPT = "transcript-only";
const DATE_OPTS: Intl.DateTimeFormatOptions = {
  weekday: "short",
  day: "numeric",
  month: "short",
};

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
}

export function MeetingsListSection({
  items,
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
            const dateLabel = new Date(
              item.started_at * 1000,
            ).toLocaleDateString(i18n.language, DATE_OPTS);
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
            const statusText = item.notes_ready
              ? t("meetings.list.notesReady")
              : t("meetings.list.transcriptOnly");
            const statusTone = item.notes_ready
              ? NOTES_READY_TONE
              : TRANSCRIPT_ONLY_TONE;
            const statusKind = item.notes_ready ? KIND_NOTES : KIND_TRANSCRIPT;
            const confirming = confirmingId === item.id;

            return (
              <li
                key={item.id}
                className="flex flex-wrap items-center justify-between gap-3 py-3"
              >
                <div className="min-w-0">
                  <p className="truncate text-[14px] font-semibold text-text">
                    {title}
                  </p>
                  <p className="mt-0.5 text-[13px] text-mid-gray">
                    <span className="font-mono text-[12px] font-semibold text-text">
                      {dateLabel}
                    </span>
                    {suffix}
                  </p>
                </div>

                <div className="flex flex-wrap items-center gap-2">
                  <span
                    data-status-kind={statusKind}
                    className={`inline-flex items-center gap-1.5 text-xs font-semibold ${statusTone}`}
                  >
                    <StatusDot filled={item.notes_ready} />
                    <span>{statusText}</span>
                  </span>

                  <button
                    type="button"
                    className={OUTLINED_BTN_SM}
                    onClick={() => onExport(item)}
                  >
                    {t("meetings.list.exportMarkdown")}
                  </button>

                  {confirming ? (
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs text-mid-gray">
                        {t("meetings.list.confirmPrompt")}
                      </span>
                      <button
                        type="button"
                        disabled={busyDeleteId === item.id}
                        onClick={() => onConfirmDelete(item.id)}
                        className="inline-flex min-h-[30px] items-center justify-center rounded-[10px] border border-red-600 bg-red-600 px-2.5 text-[13px] font-semibold text-white cursor-pointer disabled:opacity-50"
                      >
                        {t("meetings.list.confirmDelete")}
                      </button>
                      <button
                        type="button"
                        disabled={busyDeleteId === item.id}
                        onClick={onCancelDelete}
                        className={OUTLINED_BTN_SM}
                      >
                        {t("meetings.list.cancelDelete")}
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      className={OUTLINED_BTN_SM}
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
  const [items, setItems] = useState<MeetingListItem[]>(initialItems ?? []);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(!initialItems);
  const [confirmingId, setConfirmingId] = useState<number | null>(null);
  const [busyDeleteId, setBusyDeleteId] = useState<number | null>(null);
  const [audioLeftWarning, setAudioLeftWarning] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const requestSeq = useRef(0);

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
      const res = await invoke<MeetingDeleteResult>("delete_meeting", { id });
      setItems((prev) => prev.filter((row) => row.id !== id));
      setConfirmingId(null);
      setAudioLeftWarning(res.audio_left);
    } catch {
      setErrorKey("meetings.list.deleteFailed");
    } finally {
      setBusyDeleteId(null);
    }
  };

  return (
    <MeetingsListSection
      items={items}
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
