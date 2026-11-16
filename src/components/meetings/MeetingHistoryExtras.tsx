import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ConsentDialog, formatMeetingClock } from "./MeetingEntryPanels";
import { MeetingsList } from "./MeetingsList";

const DOT_FILLED = "●";
const DEFAULT_IMPORT_FOLDER = "Imported";

export interface TrashedMeetingItem {
  id: number;
  title: string;
  deletedWhen?: string;
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
      className="rounded-[12px] border border-red-500/35 bg-red-500/[0.06] p-3 space-y-1"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1.5 text-[13px] font-semibold text-text">
          <span aria-hidden="true" className="text-red-600">
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
          onClick={onBackToLive}
          className="rounded-[8px] bg-logo-primary px-3 py-1 text-[12px] font-semibold text-white cursor-pointer hover:opacity-90"
        >
          {t("meetings.storage.backToLive")}
        </button>
      </div>
      <p className="text-[12px] text-mid-gray">
        {t("meetings.storage.olderNoteHint", { title: liveTitle })}
      </p>
    </div>
  );
};

// Appending audio to a finished meeting always requires participant consent first.
export const AddAudioControl: React.FC<{
  onConfirmAppendAudio: () => void;
}> = ({ onConfirmAppendAudio }) => {
  const { t } = useTranslation();
  const [consentOpen, setConsentOpen] = useState(false);

  return (
    <div data-testid="add-audio-control" className="space-y-2.5">
      <button
        type="button"
        onClick={() => setConsentOpen(true)}
        className="inline-flex min-h-[30px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
      >
        {t("meetings.storage.addAudio")}
      </button>
      <ConsentDialog
        open={consentOpen}
        onConfirmStart={() => {
          setConsentOpen(false);
          onConfirmAppendAudio();
        }}
        onCancel={() => setConsentOpen(false)}
      />
    </div>
  );
};

export const MeetingTrashPanel: React.FC<{
  items: TrashedMeetingItem[];
  onRestore: (id: number) => void;
}> = ({ items, onRestore }) => {
  const { t } = useTranslation();

  return (
    <section
      data-testid="meetings-trash-panel"
      className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-2.5"
    >
      <h3 className="text-[16px] font-semibold text-text">
        {t("meetings.storage.trashTab")}
      </h3>
      <div className="divide-y divide-mid-gray/20">
        {items.map((item) => (
          <div
            key={item.id}
            className="flex items-center justify-between gap-2 py-2.5"
          >
            <div>
              <p className="text-[13.5px] font-semibold text-text">
                {item.title}
              </p>
              <p className="text-[12px] text-mid-gray">
                {item.deletedWhen ?? t("meetings.storage.kept30Days")}
              </p>
            </div>
            <button
              type="button"
              onClick={() => onRestore(item.id)}
              className="rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("meetings.storage.restore")}
            </button>
          </div>
        ))}
      </div>
    </section>
  );
};

export const ImportNotesPanel: React.FC<{
  selectedPaths?: string[];
  targetFolder?: string;
  onChooseFiles?: () => void;
  onImport: (paths: string[], folder: string) => void;
}> = ({
  selectedPaths = ["weekly-sync.md", "customer-interview.md", "roadmap-q4.txt"],
  targetFolder = DEFAULT_IMPORT_FOLDER,
  onChooseFiles,
  onImport,
}) => {
  const { t } = useTranslation();
  const count = selectedPaths.length;

  return (
    <section
      data-testid="meetings-import-panel"
      className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-3"
    >
      <h3 className="text-[16px] font-semibold text-text">
        {t("meetings.storage.importTab")}
      </h3>
      <p className="text-[13px] text-mid-gray">
        {t("meetings.storage.importDescription")}
      </p>
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-[10px] border border-mid-gray/25 bg-mid-gray/[0.04] px-3 py-2">
        <span className="text-[12.5px] font-medium text-text">
          {t("meetings.storage.importIntoFolder", { folder: targetFolder })}
        </span>
        <button
          type="button"
          onClick={onChooseFiles}
          className="rounded-[8px] border border-mid-gray/45 bg-background px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t("meetings.storage.chooseImportFiles")}
        </button>
      </div>
      <div className="flex justify-end">
        <button
          type="button"
          disabled={count === 0}
          onClick={() => onImport(selectedPaths, targetFolder)}
          className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90 disabled:opacity-50"
        >
          {t("meetings.storage.importAction", { count })}
        </button>
      </div>
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
  const [trashed, setTrashed] = useState<TrashedMeetingItem[]>([
    { id: 91, title: "Design critique" },
  ]);

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
      {tab === "all" ? <MeetingsList /> : null}
      {tab === "todos" ? todosSlot : null}
      {tab === "trash" ? (
        <MeetingTrashPanel
          items={trashed}
          onRestore={(id) =>
            setTrashed((prev) => prev.filter((item) => item.id !== id))
          }
        />
      ) : null}
      {tab === "import" ? <ImportNotesPanel onImport={() => {}} /> : null}
    </div>
  );
};
