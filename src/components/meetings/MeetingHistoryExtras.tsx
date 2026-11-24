import React, { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  commands as meetingCommands,
  type ImportBatchSummary,
  type MeetingRetentionState,
  type Result,
} from "../../bindings.meetings";
import { useTranslation } from "react-i18next";
import { ConsentDialog, formatMeetingClock } from "./MeetingEntryPanels";
import { MeetingsList } from "./MeetingsList";
import { runOptimisticCommand } from "./optimisticCommand";

const DOT_FILLED = "●";
const MORE_DOTS = "…";
const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
const DEFAULT_IMPORT_FOLDER = "Imported";

export interface TrashedMeetingItem {
  id: number;
  title: string;
  deletedAt?: number;
  deletedWhen?: string;
}

export async function restoreTrashedMeetingAction(
  id: number,
  currentItems: TrashedMeetingItem[],
  setTrashed: React.Dispatch<React.SetStateAction<TrashedMeetingItem[]>>,
  setErrorKey: (key: string | null) => void,
  restoreCmd: (meetingId: number) => Promise<Result<null, string>> = (
    meetingId,
  ) => meetingCommands.restoreTrashedMeetingV2(meetingId),
): Promise<boolean> {
  const removedIndex = currentItems.findIndex((item) => item.id === id);
  const removed = removedIndex >= 0 ? currentItems[removedIndex] : null;
  const outcome = await runOptimisticCommand(
    () => {
      setErrorKey(null);
      setTrashed((prev) => prev.filter((item) => item.id !== id));
    },
    () => restoreCmd(id),
    () => {
      if (removed) {
        setTrashed((prev) => {
          if (prev.some((item) => item.id === removed.id)) return prev;
          const next = [...prev];
          next.splice(Math.min(removedIndex, next.length), 0, removed);
          return next;
        });
      }
      setErrorKey("meetings.storage.restoreFailed");
    },
  );
  return outcome.ok;
}

export async function deleteTrashedMeetingNowAction(
  id: number,
  currentItems: TrashedMeetingItem[],
  setTrashed: React.Dispatch<React.SetStateAction<TrashedMeetingItem[]>>,
  setErrorKey: (key: string | null) => void,
  setAudioLeftWarning: (show: boolean) => void,
  deleteCmd: (meetingId: number) => Promise<Result<boolean, string>> = (
    meetingId,
  ) => meetingCommands.deleteMeetingForever(meetingId),
): Promise<boolean> {
  const removedIndex = currentItems.findIndex((item) => item.id === id);
  const removed = removedIndex >= 0 ? currentItems[removedIndex] : null;
  const outcome = await runOptimisticCommand(
    () => {
      setErrorKey(null);
      setAudioLeftWarning(false);
      setTrashed((prev) => prev.filter((item) => item.id !== id));
    },
    () => deleteCmd(id),
    () => {
      if (removed) {
        setTrashed((prev) => {
          if (prev.some((item) => item.id === removed.id)) return prev;
          const next = [...prev];
          next.splice(Math.min(removedIndex, next.length), 0, removed);
          return next;
        });
      }
      setErrorKey("meetings.list.deleteFailed");
    },
  );
  if (outcome.ok && outcome.data === true) {
    setAudioLeftWarning(true);
  }
  return outcome.ok;
}

export async function changeTranscriptKeepDaysAction(
  days: number | null,
  previousDays: number | null,
  setTranscriptDeleteDays: (next: number | null) => void,
  setErrorKey: (key: string | null) => void,
  changeCmd: (
    nextDays: number | null,
  ) => Promise<Result<MeetingRetentionState, string>> = (nextDays) =>
    meetingCommands.changeTranscriptDeleteDaysSetting(nextDays),
): Promise<boolean> {
  const outcome = await runOptimisticCommand(
    () => {
      setErrorKey(null);
      setTranscriptDeleteDays(days);
    },
    () => changeCmd(days),
    () => {
      setTranscriptDeleteDays(previousDays);
      setErrorKey("meetings.retention.actionFailed");
    },
  );
  return outcome.ok;
}

export function formatDeletedAgoLabel(
  deletedAtSecs: number | undefined,
  nowSecs: number,
  t: (key: string, opts?: Record<string, unknown>) => string,
): string {
  if (!deletedAtSecs) {
    return t("meetings.storage.kept30Days");
  }
  const days = Math.max(0, Math.floor((nowSecs - deletedAtSecs) / 86_400));
  if (days === 0) {
    return t("meetings.storage.kept30Days");
  }
  return t("meetings.storage.deletedDaysAgoKept30", {
    defaultValue: "Deleted {{count}} days ago · kept 30 days",
    count: days,
  });
}

export const OlderNoteWhileRecordingBanner: React.FC<{
  liveTitle: string;
  elapsedSecs: number;
  onBackToLive: () => void;
}> = ({ liveTitle, elapsedSecs, onBackToLive }) => {
  const { t } = useTranslation();
  const elapsed = formatMeetingClock(elapsedSecs);

  return (
    <div
      data-testid="older-note-live-banner"
      role="status"
      className="rounded-[12px] border-[1.5px] border-logo-primary bg-background dark:bg-surface px-3.5 py-2.5 flex flex-wrap items-center justify-between gap-2"
    >
      <span className="inline-flex items-center gap-2 text-[13px] font-semibold text-logo-primary">
        <span aria-hidden="true" className="text-logo-primary">
          {DOT_FILLED}
        </span>
        <span>
          {t("meetings.storage.olderNoteBanner", {
            title: liveTitle,
            elapsed,
          })}
        </span>
      </span>
      <button
        type="button"
        data-testid="older-note-back-link"
        onClick={onBackToLive}
        className="bg-transparent p-0 text-[13px] font-semibold text-logo-primary cursor-pointer hover:underline"
      >
        {t("meetings.storage.backToLive")}
      </button>
    </div>
  );
};

// Appending audio to a finished or paused meeting always requires participant consent first.
export const AddAudioControl: React.FC<{
  onConfirmAppendAudio: () => void;
}> = ({ onConfirmAppendAudio }) => {
  const { t } = useTranslation();
  const [consentOpen, setConsentOpen] = useState(false);

  return (
    <section
      data-testid="add-audio-control"
      className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2.5"
    >
      <p className="text-[12px] font-semibold text-mid-gray">
        {t("meetings.storage.afterMeetingHeading", "After the meeting")}
      </p>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p className="text-[13.5px] font-semibold text-text">
            {t("meetings.storage.addAudio")}
          </p>
          <p className="text-[12px] text-mid-gray">
            {t(
              "meetings.storage.addAudioDescription",
              "Records more into the same notes. Asks you to tell the room again.",
            )}
          </p>
        </div>
        <button
          type="button"
          onClick={() => setConsentOpen(true)}
          className="inline-flex min-h-[30px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t("meetings.storage.addAudioButton", "Add audio")}
        </button>
      </div>
      <ConsentDialog
        open={consentOpen}
        onConfirmStart={() => {
          setConsentOpen(false);
          onConfirmAppendAudio();
        }}
        onCancel={() => setConsentOpen(false)}
      />
    </section>
  );
};

export const MeetingTrashPanel: React.FC<{
  items: TrashedMeetingItem[];
  transcriptDeleteDays?: number | null;
  onRestore: (id: number) => void;
  onDeleteNow?: (id: number) => void;
  onChangeTranscriptDeleteDays?: (days: number | null) => void;
}> = ({
  items,
  transcriptDeleteDays = null,
  onRestore,
  onDeleteNow,
  onChangeTranscriptDeleteDays,
}) => {
  const { t } = useTranslation();
  const [menuOpenId, setMenuOpenId] = useState<number | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);
  const nowSecs = Math.floor(Date.now() / 1000);

  return (
    <div className="space-y-3.5">
      <section
        data-testid="meetings-trash-panel"
        className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2.5"
      >
        <h3 className="text-[16px] font-semibold text-text">
          {t("meetings.storage.trashTab")}
        </h3>
        {items.length === 0 ? (
          <p className="text-[13px] text-mid-gray">
            {t("meetings.storage.trashEmpty", "Trash is empty.")}
          </p>
        ) : (
          <div className="divide-y divide-mid-gray/20">
            {items.map((item) => {
              const deletedLine =
                item.deletedWhen ??
                formatDeletedAgoLabel(item.deletedAt, nowSecs, t);
              const menuOpen = menuOpenId === item.id;
              const confirming = confirmDeleteId === item.id;
              return (
                <div
                  key={item.id}
                  className="flex flex-wrap items-center justify-between gap-2 py-2.5"
                >
                  <div>
                    <p className="text-[13.5px] font-semibold text-text">
                      {item.title}
                    </p>
                    <p className="text-[12px] text-mid-gray">{deletedLine}</p>
                  </div>
                  <div className="relative flex items-center gap-1.5">
                    <button
                      type="button"
                      onClick={() => onRestore(item.id)}
                      className="rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                    >
                      {t("meetings.storage.restore")}
                    </button>
                    {onDeleteNow ? (
                      confirming ? (
                        <div className="flex items-center gap-1.5">
                          <button
                            type="button"
                            onClick={() => {
                              setConfirmDeleteId(null);
                              setMenuOpenId(null);
                              onDeleteNow(item.id);
                            }}
                            className="rounded-[8px] border border-red-600 bg-red-600 px-2.5 py-1 text-[12px] font-semibold text-white cursor-pointer"
                          >
                            {t("meetings.storage.deleteNow", "Delete now")}
                          </button>
                          <button
                            type="button"
                            onClick={() => setConfirmDeleteId(null)}
                            className="rounded-[8px] border border-mid-gray/45 px-2 py-1 text-[12px] text-text cursor-pointer"
                          >
                            {t("meetings.consent.cancel")}
                          </button>
                        </div>
                      ) : (
                        <>
                          <button
                            type="button"
                            aria-label={t(
                              "meetings.storage.moreActions",
                              "More actions",
                            )}
                            aria-expanded={menuOpen}
                            onClick={() =>
                              setMenuOpenId(menuOpen ? null : item.id)
                            }
                            className="rounded-[8px] border border-mid-gray/35 px-2 py-1 text-[12px] font-semibold text-mid-gray cursor-pointer hover:text-text"
                          >
                            {MORE_DOTS}
                          </button>
                          {menuOpen ? (
                            <div
                              role="menu"
                              className="absolute right-0 top-9 z-20 min-w-[140px] rounded-[9px] border border-mid-gray/30 bg-background p-1 shadow-lg"
                            >
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setMenuOpenId(null);
                                  setConfirmDeleteId(item.id);
                                }}
                                className="block w-full rounded-[7px] px-2.5 py-1.5 text-left text-[12px] font-semibold text-red-600 cursor-pointer hover:bg-red-500/10"
                              >
                                {t("meetings.storage.deleteNow", "Delete now")}
                              </button>
                            </div>
                          ) : null}
                        </>
                      )
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <p className="text-[13.5px] font-semibold text-text">
              {t(
                "meetings.retention.deleteTimerLabel",
                "Delete transcripts after",
              )}
            </p>
            <p className="text-[12px] text-mid-gray">
              {transcriptDeleteDays
                ? t("meetings.storage.daysCount", {
                    defaultValue: "{{count}} days",
                    count: transcriptDeleteDays,
                  })
                : t(
                    "meetings.storage.optionalTimerOff",
                    "Off (optional timer)",
                  )}
            </p>
          </div>
          <div className="flex items-center gap-2.5">
            {transcriptDeleteDays && onChangeTranscriptDeleteDays ? (
              <select
                aria-label={t(
                  "meetings.retention.deleteTimerLabel",
                  "Delete transcripts after",
                )}
                value={String(transcriptDeleteDays)}
                onChange={(e) =>
                  onChangeTranscriptDeleteDays(Number(e.target.value))
                }
                className="rounded-full border border-mid-gray/35 bg-background px-2.5 py-1 text-[12px] font-medium text-text cursor-pointer"
              >
                <option value="7">
                  {t("meetings.storage.daysCount", {
                    defaultValue: "{{count}} days",
                    count: 7,
                  })}
                </option>
                <option value="30">
                  {t("meetings.storage.daysCount", {
                    defaultValue: "{{count}} days",
                    count: 30,
                  })}
                </option>
                <option value="90">
                  {t("meetings.storage.daysCount", {
                    defaultValue: "{{count}} days",
                    count: 90,
                  })}
                </option>
              </select>
            ) : null}
            <label
              className={`inline-flex shrink-0 items-center ${onChangeTranscriptDeleteDays ? "cursor-pointer" : "cursor-default"}`}
            >
              <input
                type="checkbox"
                role="switch"
                className="sr-only peer"
                checked={transcriptDeleteDays !== null}
                disabled={!onChangeTranscriptDeleteDays}
                aria-label={t(
                  "meetings.retention.deleteTimerLabel",
                  "Delete transcripts after",
                )}
                onChange={(e) =>
                  onChangeTranscriptDeleteDays?.(e.target.checked ? 30 : null)
                }
              />
              <div className="relative h-6 w-11 rounded-full bg-mid-gray/25 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-logo-primary peer-checked:bg-background-ui peer-disabled:opacity-50 after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white" />
            </label>
          </div>
        </div>
      </section>
    </div>
  );
};

export const ImportNotesPanel: React.FC<{
  selectedPaths?: string[];
  importableCount?: number;
  displayFolder?: string | null;
  targetFolder?: string;
  availableFolders?: string[];
  resultSummary?: ImportBatchSummary | null;
  onChooseFiles?: () => void;
  onChangeTargetFolder?: (folder: string) => void;
  onImport: (paths: string[], folder: string) => void;
}> = ({
  selectedPaths = [],
  importableCount,
  displayFolder = null,
  targetFolder = DEFAULT_IMPORT_FOLDER,
  availableFolders = [DEFAULT_IMPORT_FOLDER],
  resultSummary = null,
  onChooseFiles,
  onChangeTargetFolder,
  onImport,
}) => {
  const { t } = useTranslation();
  const [changingFolder, setChangingFolder] = useState(false);
  const [customFolderDraft, setCustomFolderDraft] = useState("");
  const count = importableCount ?? selectedPaths.length;
  const folderOptions = Array.from(
    new Set([DEFAULT_IMPORT_FOLDER, targetFolder, ...availableFolders]),
  ).filter(Boolean);

  return (
    <section data-testid="meetings-import-panel" className="space-y-3.5">
      <div>
        <p className={EYEBROW}>{t("meetings.sidebar")}</p>
        <h3 className="mt-1 text-[20px] font-semibold tracking-[-0.02em] text-text">
          {t("meetings.storage.importTab")}
        </h3>
        <p className="mt-1 text-[13px] text-mid-gray">
          {t("meetings.storage.importDescription")}
        </p>
      </div>

      <div className="divide-y divide-mid-gray/20 rounded-[14px] border border-mid-gray/25 bg-background px-4">
        <div className="flex flex-wrap items-center justify-between gap-2 py-3">
          <div>
            <p className="text-[13.5px] font-semibold text-text">
              {t("meetings.storage.chooseImportFiles")}
            </p>
            <p className="text-[12px] text-mid-gray">
              {count > 0 && displayFolder
                ? t("meetings.storage.importFilesFoundIn", {
                    defaultValue: "{{count}} files found in {{folder}}",
                    count,
                    folder: displayFolder,
                  })
                : t("meetings.storage.importFileTypes", ".md and .txt files")}
            </p>
          </div>
          <button
            type="button"
            onClick={onChooseFiles}
            className="rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("meetings.storage.chooseShort", "Choose")}
          </button>
        </div>
        <div className="space-y-2 py-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <p className="text-[13.5px] font-semibold text-text">
                {t(
                  "meetings.storage.importIntoFolderLabel",
                  "Import into folder",
                )}
              </p>
              <p className="text-[12px] text-mid-gray">{targetFolder}</p>
            </div>
            <button
              type="button"
              onClick={() => setChangingFolder((v) => !v)}
              className="rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("meetings.storage.changeFolder", "Change")}
            </button>
          </div>
          {changingFolder && onChangeTargetFolder ? (
            <div className="flex flex-wrap items-center gap-2 pt-1">
              <select
                aria-label={t("meetings.notes.foldersHeading", "Folders")}
                value={targetFolder}
                onChange={(e) => {
                  onChangeTargetFolder(e.target.value);
                  setChangingFolder(false);
                }}
                className="rounded-[8px] border border-mid-gray/35 bg-background px-2.5 py-1 text-[12px] text-text"
              >
                {folderOptions.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
              <input
                type="text"
                value={customFolderDraft}
                onChange={(e) => setCustomFolderDraft(e.target.value)}
                placeholder={t(
                  "meetings.notes.newFolderPlaceholder",
                  "New folder",
                )}
                aria-label={t(
                  "meetings.notes.newFolderPlaceholder",
                  "New folder",
                )}
                className="rounded-[8px] border border-mid-gray/35 bg-background px-2.5 py-1 text-[12px] text-text"
              />
              <button
                type="button"
                onClick={() => {
                  const clean = customFolderDraft.trim();
                  if (!clean) return;
                  onChangeTargetFolder(clean);
                  setCustomFolderDraft("");
                  setChangingFolder(false);
                }}
                className="rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer"
              >
                {t("meetings.notes.createFolder", "Add")}
              </button>
            </div>
          ) : null}
        </div>
      </div>

      <div className="flex items-center">
        <button
          type="button"
          disabled={count === 0}
          onClick={() => onImport(selectedPaths, targetFolder)}
          className="inline-flex min-h-[34px] items-center rounded-[9px] bg-logo-primary px-4 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90 disabled:opacity-50"
        >
          {t("meetings.storage.importAction", { count })}
        </button>
      </div>

      {resultSummary ? (
        <div
          role="status"
          data-testid="import-result-summary"
          className="rounded-[12px] border border-mid-gray/25 bg-background p-3.5 space-y-2 text-[12.5px]"
        >
          <p className="font-medium text-text">
            {t("meetings.storage.importCompletedSummary", {
              defaultValue:
                "Imported {{imported}}. Skipped {{skipped}} already imported.",
              imported: resultSummary.imported_count,
              skipped: resultSummary.skipped_count,
            })}
          </p>
          {resultSummary.failed.length > 0 ? (
            <ul className="space-y-1 text-mid-gray">
              {resultSummary.failed.map((f, idx) => (
                <li key={`${f.name}-${idx}`}>
                  {t("meetings.storage.importFailedItem", {
                    defaultValue: "{{name}}: {{reason}}",
                    name: f.name,
                    reason: f.reason,
                  })}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </section>
  );
};

export type HistoryHubTab = "all" | "todos" | "trash" | "import";

export const MeetingHistoryHub: React.FC<{
  initialTab?: HistoryHubTab;
  todosLabel?: string;
  todosSlot?: React.ReactNode;
}> = ({ initialTab = "all", todosLabel, todosSlot }) => {
  const { t } = useTranslation();
  const [tab, setTab] = useState<HistoryHubTab>(initialTab);
  const [trashed, setTrashed] = useState<TrashedMeetingItem[]>([]);
  const [transcriptDeleteDays, setTranscriptDeleteDays] = useState<
    number | null
  >(null);
  const [importPaths, setImportPaths] = useState<string[]>([]);
  const [importCount, setImportCount] = useState<number>(0);
  const [importDisplayFolder, setImportDisplayFolder] = useState<string | null>(
    null,
  );
  const [importTargetFolder, setImportTargetFolder] = useState<string>(
    DEFAULT_IMPORT_FOLDER,
  );
  const [folderNames, setFolderNames] = useState<string[]>([
    DEFAULT_IMPORT_FOLDER,
  ]);
  const [importResult, setImportResult] = useState<ImportBatchSummary | null>(
    null,
  );
  const [trashErrorKey, setTrashErrorKey] = useState<string | null>(null);
  const [trashAudioLeftWarning, setTrashAudioLeftWarning] = useState(false);

  useEffect(() => {
    setTrashErrorKey(null);
    setTrashAudioLeftWarning(false);
    let cancelled = false;
    void meetingCommands.listTrashedMeetingsV2().then((res) => {
      if (!cancelled && res.status === "ok") {
        setTrashed(
          res.data.map((r) => ({
            id: r.id,
            title: r.title,
            deletedAt: r.deleted_at,
          })),
        );
      }
    });
    void meetingCommands.meetingRetentionState().then((st) => {
      if (!cancelled) {
        setTranscriptDeleteDays(st.transcript_delete_after_days ?? null);
      }
    });
    void meetingCommands.listMeetingFolders().then((res) => {
      if (!cancelled && res.status === "ok") {
        setFolderNames([DEFAULT_IMPORT_FOLDER, ...res.data.map((f) => f.name)]);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [tab]);

  const handleChooseImportPaths = async () => {
    try {
      const picked = await openDialog({
        multiple: true,
        directory: false,
        filters: [{ name: "Notes", extensions: ["md", "markdown", "txt"] }],
      });
      const list = Array.isArray(picked)
        ? picked
        : typeof picked === "string"
          ? [picked]
          : [];
      if (list.length === 0) return;
      setImportPaths(list);
      setImportResult(null);
      const countRes = await meetingCommands.countImportableNotes(list);
      if (countRes.status === "ok") {
        setImportCount(countRes.data.count);
        setImportDisplayFolder(countRes.data.display_folder);
      } else {
        setImportCount(list.length);
      }
    } catch {
      // User cancelled picker or dialog unavailable.
    }
  };

  const tabBtn = (id: HistoryHubTab, label: string) => {
    const on = tab === id;
    return (
      <button
        key={id}
        type="button"
        aria-pressed={on}
        onClick={() => setTab(id)}
        className={`rounded-full px-3 py-1 text-[12.5px] font-semibold cursor-pointer ${
          on
            ? "bg-logo-primary text-white"
            : "border border-mid-gray/35 text-text/80 hover:bg-mid-gray/10"
        }`}
      >
        {label}
      </button>
    );
  };

  const tabRow = [
    tabBtn("all", t("meetings.list.heading")),
    ...(todosLabel && todosSlot ? [tabBtn("todos", todosLabel)] : []),
    tabBtn("trash", t("meetings.storage.trashTab")),
    tabBtn("import", t("meetings.storage.importTab")),
  ];

  return (
    <div className="max-w-3xl w-full mx-auto space-y-3">
      <div className="flex flex-wrap items-center gap-2">{tabRow}</div>
      {trashAudioLeftWarning && (
        <p
          role="status"
          className="rounded-[10px] border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-text"
        >
          {t("meetings.list.audioLeftWarning")}
        </p>
      )}
      {trashErrorKey !== null && (
        <p role="alert" className="text-sm text-red-600">
          {t(trashErrorKey)}
        </p>
      )}
      {tab === "all" ? <MeetingsList /> : null}
      {tab === "todos" ? todosSlot : null}
      {tab === "trash" ? (
        <MeetingTrashPanel
          items={trashed}
          transcriptDeleteDays={transcriptDeleteDays}
          onRestore={(id) => {
            void restoreTrashedMeetingAction(
              id,
              trashed,
              setTrashed,
              setTrashErrorKey,
            );
          }}
          onDeleteNow={(id) => {
            void deleteTrashedMeetingNowAction(
              id,
              trashed,
              setTrashed,
              setTrashErrorKey,
              setTrashAudioLeftWarning,
            );
          }}
          onChangeTranscriptDeleteDays={(days) => {
            void changeTranscriptKeepDaysAction(
              days,
              transcriptDeleteDays,
              setTranscriptDeleteDays,
              setTrashErrorKey,
            );
          }}
        />
      ) : null}
      {tab === "import" ? (
        <ImportNotesPanel
          selectedPaths={importPaths}
          importableCount={importCount}
          displayFolder={importDisplayFolder}
          targetFolder={importTargetFolder}
          availableFolders={folderNames}
          resultSummary={importResult}
          onChooseFiles={() => void handleChooseImportPaths()}
          onChangeTargetFolder={setImportTargetFolder}
          onImport={(paths, folder) => {
            void meetingCommands
              .importMeetingNotesBatch(paths, folder)
              .then((res) => {
                if (res.status === "ok") {
                  setImportResult(res.data);
                }
              });
          }}
        />
      ) : null}
    </div>
  );
};
