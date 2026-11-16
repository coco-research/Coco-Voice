import React, { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { useTranslation } from "react-i18next";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import {
  createHistoryCommandHandlers,
  displayTextFor,
  formatDictationTime,
  HistoryRow,
  selectDictationEntries,
} from "./HistoryRow";

export {
  createHistoryCommandHandlers,
  displayTextFor,
  formatDictationTime,
  selectDictationEntries,
};

const PAGE_SIZE = 30;

export interface HistoryProps {
  initialEntries?: readonly unknown[];
  initialQuery?: string;
  initialOpenMenuId?: number | null;
  retentionText?: string;
  nowTimestamp?: number;
}

export interface HistoryViewState {
  entries: HistoryEntry[];
  loading: boolean;
  query: string;
  openMenuId: number | null;
  editingId: number | null;
  editDraft: string;
  savingEdit: boolean;
}

export interface HistoryViewActions {
  setQuery: (q: string) => void;
  setOpenMenuId: (id: number | null) => void;
  startEdit: (entry: HistoryEntry) => void;
  setEditDraft: (draft: string) => void;
  cancelEdit: () => void;
  commitEdit: (id: number, draft: string) => Promise<void>;
  toggleSaved: (id: number) => Promise<void>;
  copyText: (text: string) => Promise<void>;
  deleteEntry: (id: number) => Promise<void>;
  retryEntry: (id: number) => Promise<void>;
  openFolder: () => Promise<void>;
  getAudioUrl: (fileName: string) => Promise<string | null>;
}

export const HistorySettings: React.FC<HistoryProps> = (props) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>(() =>
    props.initialEntries
      ? selectDictationEntries(props.initialEntries, "")
      : [],
  );
  const [loading, setLoading] = useState(!props.initialEntries);
  const [query, setQuery] = useState(props.initialQuery ?? "");
  const [hasMore, setHasMore] = useState(true);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef<HistoryEntry[]>([]);
  const loadingRef = useRef(false);

  // Keep ref in sync for use in IntersectionObserver callback
  useEffect(() => {
    entriesRef.current = entries;
  }, [entries]);

  const loadPage = useCallback(async (cursor?: number) => {
    const isFirstPage = cursor === undefined;
    if (!isFirstPage && loadingRef.current) return;
    loadingRef.current = true;

    if (isFirstPage) setLoading(true);

    try {
      const result = await commands.getHistoryEntries(
        cursor ?? null,
        PAGE_SIZE,
      );
      if (result.status === "ok") {
        const { entries: newEntries, has_more } = result.data;
        setEntries((prev) =>
          isFirstPage ? newEntries : [...prev, ...newEntries],
        );
        setHasMore(has_more);
      }
    } catch (error) {
      console.error("Failed to load history entries:", error);
    } finally {
      setLoading(false);
      loadingRef.current = false;
    }
  }, []);

  // Initial load
  useEffect(() => {
    if (props.initialEntries) return;
    void loadPage();
  }, [props.initialEntries, loadPage]);

  // Infinite scroll via IntersectionObserver
  useEffect(() => {
    if (loading || props.initialEntries) return;

    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;

    const observer = new IntersectionObserver(
      (observerEntries) => {
        const first = observerEntries[0];
        if (first?.isIntersecting) {
          const lastEntry = entriesRef.current[entriesRef.current.length - 1];
          if (lastEntry) {
            void loadPage(lastEntry.id);
          }
        }
      },
      { threshold: 0 },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [loading, hasMore, loadPage, props.initialEntries]);

  // Listen for new entries added from the transcription pipeline
  useEffect(() => {
    if (props.initialEntries) return;
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        setEntries((prev) => [payload.entry, ...prev]);
      } else if (payload.action === "updated") {
        setEntries((prev) =>
          prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
        );
      }
    });

    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [props.initialEntries]);

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
    }
  };

  const getAudioUrl = useCallback(
    async (fileName: string) => {
      try {
        const result = await commands.getAudioFilePath(fileName);
        if (result.status === "ok") {
          if (osType === "linux") {
            const fileData = await readFile(result.data);
            const blob = new Blob([fileData], { type: "audio/wav" });
            return URL.createObjectURL(blob);
          }
          return convertFileSrc(result.data, "asset");
        }
        return null;
      } catch (error) {
        console.error("Failed to get audio file path:", error);
        return null;
      }
    },
    [osType],
  );

  const {
    toggleSaved,
    deleteAudioEntry,
    retryHistoryEntry,
    saveEditAndLearn,
    openRecordingsFolder,
  } = createHistoryCommandHandlers({
    getEntries: () => entriesRef.current,
    setEntries,
    reload: () => void loadPage(),
    t,
  });

  const [openMenuId, setOpenMenuId] = useState<number | null>(
    props.initialOpenMenuId ?? null,
  );
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [savingEdit, setSavingEdit] = useState(false);

  useEffect(() => {
    if (openMenuId === null || typeof document === "undefined") return;
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") setOpenMenuId(null);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [openMenuId]);

  const state: HistoryViewState = {
    entries,
    loading,
    query,
    openMenuId,
    editingId,
    editDraft,
    savingEdit,
  };
  const actions: HistoryViewActions = {
    setQuery,
    setOpenMenuId,
    startEdit: (entry) => {
      setEditingId(entry.id);
      setEditDraft(displayTextFor(entry));
    },
    setEditDraft,
    cancelEdit: () => setEditingId(null),
    commitEdit: async (id, draft) => {
      setSavingEdit(true);
      try {
        await saveEditAndLearn(id, draft);
        setEditingId(null);
      } finally {
        setSavingEdit(false);
      }
    },
    toggleSaved,
    copyText: copyToClipboard,
    deleteEntry: deleteAudioEntry,
    retryEntry: retryHistoryEntry,
    openFolder: openRecordingsFolder,
    getAudioUrl,
  };

  return (
    <HistorySettingsSection
      state={state}
      actions={actions}
      retentionText={props.retentionText}
      nowTimestamp={props.nowTimestamp}
      sentinelRef={sentinelRef}
    />
  );
};

export const HistorySettingsSection: React.FC<{
  state: HistoryViewState;
  actions: HistoryViewActions;
  retentionText?: string;
  nowTimestamp?: number;
  sentinelRef?: React.RefObject<HTMLDivElement>;
}> = ({ state, actions, retentionText, nowTimestamp, sentinelRef }) => {
  const { t } = useTranslation();
  const filtered = selectDictationEntries(state.entries, state.query);
  const retentionLabel = retentionText ?? t("sidebar.keptDays");

  let content: React.ReactNode;
  if (state.loading) {
    content = (
      <p className="mt-5 text-sm text-mid-gray">
        {t("settings.history.loading")}
      </p>
    );
  } else if (state.entries.length === 0) {
    content = (
      <p className="mt-5 text-sm text-mid-gray">
        {t("settings.history.empty")}
      </p>
    );
  } else if (filtered.length === 0) {
    content = (
      <p className="mt-5 text-sm text-mid-gray">
        {t("sidebar.emptyDictationSearch")}
      </p>
    );
  } else {
    content = (
      <>
        <div className="mt-3 divide-y divide-mid-gray/20 border-b-[1.5px] border-text/80">
          {filtered.map((entry) => (
            <HistoryRow
              key={entry.id}
              entry={entry}
              nowTimestamp={nowTimestamp}
              menuOpen={state.openMenuId === entry.id}
              editing={state.editingId === entry.id}
              editDraft={state.editingId === entry.id ? state.editDraft : ""}
              savingEdit={state.savingEdit}
              onMenuToggle={() =>
                actions.setOpenMenuId(
                  state.openMenuId === entry.id ? null : entry.id,
                )
              }
              onMenuClose={() => actions.setOpenMenuId(null)}
              onToggleSaved={() => void actions.toggleSaved(entry.id)}
              onCopyText={() => void actions.copyText(displayTextFor(entry))}
              onStartEdit={() => actions.startEdit(entry)}
              onEditDraftChange={actions.setEditDraft}
              onCancelEdit={actions.cancelEdit}
              onCommitEdit={() => actions.commitEdit(entry.id, state.editDraft)}
              getAudioUrl={actions.getAudioUrl}
              deleteAudio={actions.deleteEntry}
              retryTranscription={actions.retryEntry}
            />
          ))}
        </div>
        <div ref={sentinelRef} className="h-1" />
      </>
    );
  }

  return (
    <div
      data-testid="dictation-history-section"
      className="max-w-3xl w-full mx-auto"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("sidebar.voiceSection")}
      </p>
      <header className="mt-1 flex items-baseline justify-between gap-3">
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("sidebar.dictationHistory")}
        </h2>
        <div className="flex items-baseline gap-3">
          <button
            type="button"
            onClick={() => void actions.openFolder()}
            className="text-xs font-medium text-mid-gray hover:text-text hover:underline cursor-pointer"
          >
            {t("settings.history.openFolder")}
          </button>
          <span className="text-xs font-medium text-mid-gray">
            {retentionLabel}
          </span>
        </div>
      </header>
      <label className="mt-3 block">
        <span className="block text-xs font-medium text-mid-gray">
          {t("meetings.list.searchLabel")}
        </span>
        <input
          type="search"
          value={state.query}
          onChange={(e) => actions.setQuery(e.target.value)}
          placeholder={t("sidebar.dictationSearchPlaceholder")}
          aria-label={t("sidebar.dictationSearchAria")}
          className="mt-1 w-full rounded-[10px] border border-mid-gray/40 bg-background px-3 py-1.5 text-sm text-text focus:outline-none focus:ring-2 focus:ring-logo-primary"
        />
      </label>
      {content}
    </div>
  );
};
