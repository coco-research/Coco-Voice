import React, { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { FolderOpen } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  commands,
  events,
  type HistoryEntry,
  type HistoryUpdatePayload,
} from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { learnCorrectionsFromEdit } from "@/lib/utils/autoLearnCorrections";
import { Button } from "../../ui/Button";
import { displayTextFor, HistoryRow } from "./HistoryRow";

const PAGE_SIZE = 30;

interface OpenRecordingsButtonProps {
  onClick: () => void;
  label: string;
}

const OpenRecordingsButton: React.FC<OpenRecordingsButtonProps> = ({
  onClick,
  label,
}) => (
  <Button
    onClick={onClick}
    variant="secondary"
    size="sm"
    className="flex items-center gap-2"
    title={label}
  >
    <FolderOpen className="w-4 h-4" />
    <span>{label}</span>
  </Button>
);

export const HistorySettings: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
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
    loadPage();
  }, [loadPage]);

  // Infinite scroll via IntersectionObserver
  useEffect(() => {
    if (loading) return;

    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;

    const observer = new IntersectionObserver(
      (observerEntries) => {
        const first = observerEntries[0];
        if (first.isIntersecting) {
          const lastEntry = entriesRef.current[entriesRef.current.length - 1];
          if (lastEntry) {
            loadPage(lastEntry.id);
          }
        }
      },
      { threshold: 0 },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [loading, hasMore, loadPage]);

  // Listen for new entries added from the transcription pipeline
  useEffect(() => {
    const unlisten = events.historyUpdatePayload.listen((event) => {
      const payload: HistoryUpdatePayload = event.payload;
      if (payload.action === "added") {
        setEntries((prev) => [payload.entry, ...prev]);
      } else if (payload.action === "updated") {
        setEntries((prev) =>
          prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
        );
      }
      // "deleted" and "toggled" are handled by optimistic updates only,
      // so we intentionally ignore them here to avoid double-mutation.
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const toggleSaved = async (id: number) => {
    // Optimistic update
    setEntries((prev) =>
      prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
    );
    try {
      const result = await commands.toggleHistoryEntrySaved(id);
      if (result.status !== "ok") {
        // Revert on failure
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      }
    } catch (error) {
      console.error("Failed to toggle saved status:", error);
      // Revert on failure
      setEntries((prev) =>
        prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
      );
    }
  };

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

  const deleteAudioEntry = async (id: number) => {
    // Optimistically remove
    setEntries((prev) => prev.filter((e) => e.id !== id));
    try {
      const result = await commands.deleteHistoryEntry(id);
      if (result.status !== "ok") {
        // Reload on failure
        loadPage();
      }
    } catch (error) {
      console.error("Failed to delete entry:", error);
      loadPage();
    }
  };

  const retryHistoryEntry = async (id: number) => {
    const result = await commands.retryHistoryEntryTranscription(id);
    if (result.status !== "ok") {
      throw new Error(String(result.error));
    }
  };

  // Persist a manual edit of a transcription and auto-learn correction pairs
  // from the word-level diff. Conservative single-word swaps are appended to the
  // existing correction map (Feature 1's `update_corrections` command) so the
  // same fix is applied automatically to future transcriptions.
  const saveEditAndLearn = async (id: number, newText: string) => {
    const entry = entriesRef.current.find((e) => e.id === id);
    // Diff and persist against the text the user actually saw.
    const original = entry ? displayTextFor(entry) : "";
    // Write the edit back to the same field that was displayed: the
    // post-processed text when post-processing produced output, otherwise the
    // raw transcript. Keeping the field consistent means auto-learn diffs
    // post-processed-vs-edited and the display stays in sync.
    const usedPostProcessed = !!(
      entry?.post_processed_text && entry.post_processed_text.trim().length > 0
    );
    const trimmed = newText.trim();
    if (!trimmed || trimmed === original.trim()) {
      return;
    }

    const applyText = (e: HistoryEntry, text: string): HistoryEntry =>
      usedPostProcessed
        ? { ...e, post_processed_text: text }
        : { ...e, transcription_text: text };

    // Optimistic update; the backend also emits an "updated" event that resolves
    // to the same text, so this stays consistent on reload.
    setEntries((prev) =>
      prev.map((e) => (e.id === id ? applyText(e, trimmed) : e)),
    );

    try {
      const result = await commands.updateHistoryEntryText(id, trimmed);
      if (result.status !== "ok") {
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? applyText(e, original) : e)),
        );
        toast.error(t("settings.history.editError"));
        return;
      }
    } catch (error) {
      console.error("Failed to save transcription edit:", error);
      setEntries((prev) =>
        prev.map((e) => (e.id === id ? applyText(e, original) : e)),
      );
      toast.error(t("settings.history.editError"));
      return;
    }

    // Turn the edit into correction pairs and append them to the map.
    try {
      const settingsResult = await commands.getAppSettings();
      if (settingsResult.status !== "ok") {
        return;
      }
      const existing = settingsResult.data.corrections ?? [];
      const learned = learnCorrectionsFromEdit(original, trimmed, existing);
      if (learned.length === 0) {
        return;
      }
      const updateResult = await commands.updateCorrections([
        ...existing,
        ...learned,
      ]);
      if (updateResult.status === "ok") {
        toast.success(
          learned.length === 1
            ? t("settings.history.learnedOne", {
                from: learned[0].from,
                to: learned[0].to,
              })
            : t("settings.history.learnedMany", { count: learned.length }),
        );
      }
    } catch (error) {
      console.error("Failed to auto-learn corrections:", error);
    }
  };

  const openRecordingsFolder = async () => {
    try {
      const result = await commands.openRecordingsFolder();
      if (result.status !== "ok") {
        throw new Error(String(result.error));
      }
    } catch (error) {
      console.error("Failed to open recordings folder:", error);
    }
  };

  const [openMenuId, setOpenMenuId] = useState<number | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [savingEdit, setSavingEdit] = useState(false);

  let content: React.ReactNode;

  if (loading) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("settings.history.loading")}
      </div>
    );
  } else if (entries.length === 0) {
    content = (
      <div className="px-4 py-3 text-center text-text/60">
        {t("settings.history.empty")}
      </div>
    );
  } else {
    content = (
      <>
        <div className="divide-y divide-mid-gray/20">
          {entries.map((entry) => (
            <HistoryRow
              key={entry.id}
              entry={entry}
              menuOpen={openMenuId === entry.id}
              editing={editingId === entry.id}
              editDraft={editingId === entry.id ? editDraft : ""}
              savingEdit={savingEdit}
              onMenuToggle={() =>
                setOpenMenuId((cur) => (cur === entry.id ? null : entry.id))
              }
              onMenuClose={() => setOpenMenuId(null)}
              onToggleSaved={() => void toggleSaved(entry.id)}
              onCopyText={() => void copyToClipboard(displayTextFor(entry))}
              onStartEdit={() => {
                setEditingId(entry.id);
                setEditDraft(displayTextFor(entry));
              }}
              onEditDraftChange={setEditDraft}
              onCancelEdit={() => setEditingId(null)}
              onCommitEdit={async () => {
                setSavingEdit(true);
                try {
                  await saveEditAndLearn(entry.id, editDraft);
                  setEditingId(null);
                } finally {
                  setSavingEdit(false);
                }
              }}
              getAudioUrl={getAudioUrl}
              deleteAudio={deleteAudioEntry}
              retryTranscription={retryHistoryEntry}
            />
          ))}
        </div>
        {/* Sentinel for infinite scroll */}
        <div ref={sentinelRef} className="h-1" />
      </>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-between">
          <div>
            <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
              {t("settings.history.title")}
            </h2>
          </div>
          <OpenRecordingsButton
            onClick={openRecordingsFolder}
            label={t("settings.history.openFolder")}
          />
        </div>
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          {content}
        </div>
      </div>
    </div>
  );
};
