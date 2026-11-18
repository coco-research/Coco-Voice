import React, { useState } from "react";
import { useTranslation } from "react-i18next";

const DOT_FILLED = "●";
const DOT_OPEN = "○";
const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";

export interface LocalMeetingFolder {
  id: string;
  name: string;
  open: boolean;
  meetings: Array<{ id: number; title: string; dateLabel: string }>;
}

export interface MeetingNoteTemplate {
  id: string;
  name: string;
  sections: string[];
}

// Local folders are strictly flat (one level only); slash separators are flattened.
export function sanitizeFlatFolderName(raw: string): string {
  return raw
    .replace(/[\\/]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

const DEFAULT_FOLDERS: LocalMeetingFolder[] = [
  {
    id: "customers",
    name: "Customers",
    open: true,
    meetings: [
      { id: 101, title: "Renewal review", dateLabel: "Tue 14:00 · 28 min" },
      {
        id: 102,
        title: "Onboarding walkthrough",
        dateLabel: "Mon 11:30 · 42 min",
      },
    ],
  },
  {
    id: "team",
    name: "Team",
    open: false,
    meetings: [
      { id: 103, title: "Weekly planning", dateLabel: "Mon 09:30 · 30 min" },
    ],
  },
  {
    id: "interviews",
    name: "Interviews",
    open: false,
    meetings: [],
  },
];

const DEFAULT_TEMPLATES: MeetingNoteTemplate[] = [
  {
    id: "one-to-one",
    name: "One-to-one",
    sections: ["Wins and blockers", "Decisions", "Follow-ups"],
  },
  {
    id: "customer-call",
    name: "Customer call",
    sections: ["Goals", "Pain points", "Agreed next steps"],
  },
  {
    id: "interview",
    name: "Interview",
    sections: ["Role fit", "Examples shared", "Open questions"],
  },
];

export interface FoldersAndTemplatesSectionProps {
  folders: LocalMeetingFolder[];
  templates: MeetingNoteTemplate[];
  selectedTemplateId: string;
  onToggleFolder: (folderId: string) => void;
  onAddFolder: (name: string) => void;
  onSelectTemplate: (templateId: string) => void;
  onStartWithTemplate?: (templateId: string) => void;
}

export const FoldersAndTemplatesSection: React.FC<
  FoldersAndTemplatesSectionProps
> = ({
  folders,
  templates,
  selectedTemplateId,
  onToggleFolder,
  onAddFolder,
  onSelectTemplate,
  onStartWithTemplate,
}) => {
  const { t } = useTranslation();
  const [folderDraft, setFolderDraft] = useState("");
  const [editingTemplates, setEditingTemplates] = useState(false);

  const handleCreateFolder = () => {
    const clean = sanitizeFlatFolderName(folderDraft);
    if (!clean) return;
    onAddFolder(clean);
    setFolderDraft("");
  };

  return (
    <div
      data-testid="frame-11-folders-templates"
      className="max-w-3xl w-full mx-auto space-y-4"
    >
      <p className={EYEBROW}>{t("meetings.sidebar")}</p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t("sidebar.foldersAndTemplates")}
      </h2>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-3">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-[15px] font-semibold text-text">
            {t("meetings.notes.foldersHeading")}
          </h3>
          <span className="text-[12px] text-mid-gray">
            {t("meetings.notes.flatFoldersHint")}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <input
            type="text"
            value={folderDraft}
            onChange={(e) => setFolderDraft(e.target.value)}
            placeholder={t("meetings.notes.newFolderPlaceholder")}
            aria-label={t("meetings.notes.foldersHeading")}
            className="min-w-0 flex-1 rounded-[9px] border border-mid-gray/35 bg-background px-3 py-1.5 text-[13px] text-text"
          />
          <button
            type="button"
            onClick={handleCreateFolder}
            className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[12.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("meetings.notes.createFolder")}
          </button>
        </div>

        <div className="divide-y divide-mid-gray/20">
          {folders.map((folder) => {
            const dot = folder.open ? DOT_FILLED : DOT_OPEN;
            const stateLabel = folder.open
              ? t("meetings.notes.openFolder")
              : t("meetings.notes.closedFolder");
            return (
              <div key={folder.id} className="py-2.5 space-y-2">
                <button
                  type="button"
                  onClick={() => onToggleFolder(folder.id)}
                  className="flex w-full items-center justify-between text-start cursor-pointer"
                >
                  <span className="text-[14px] font-semibold text-text">
                    {folder.name}
                  </span>
                  <span className="inline-flex items-center gap-1 text-[12px] font-medium text-mid-gray">
                    <span aria-hidden="true">{dot}</span>
                    <span>{stateLabel}</span>
                  </span>
                </button>
                {folder.open && folder.meetings.length > 0 ? (
                  <ul className="space-y-1 ps-3 border-s-2 border-mid-gray/20">
                    {folder.meetings.map((m) => (
                      <li
                        key={m.id}
                        className="flex items-center justify-between text-[13px]"
                      >
                        <span className="font-medium text-text">{m.title}</span>
                        <span className="font-mono text-[11.5px] text-mid-gray">
                          {m.dateLabel}
                        </span>
                      </li>
                    ))}
                  </ul>
                ) : null}
              </div>
            );
          })}
        </div>
      </section>

      <section className="rounded-[14px] border border-mid-gray/25 bg-background p-4 space-y-3">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-[15px] font-semibold text-text">
            {t("meetings.notes.templatesHeading")}
          </h3>
          <button
            type="button"
            onClick={() => setEditingTemplates((v) => !v)}
            className="rounded-[8px] border border-mid-gray/40 px-2.5 py-1 text-[12px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("meetings.notes.editTemplates")}
          </button>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-2.5">
          {templates.map((tpl) => {
            const active = tpl.id === selectedTemplateId;
            const sep = " · ";
            const preview = tpl.sections.join(sep);
            return (
              <button
                key={tpl.id}
                type="button"
                aria-pressed={active}
                onClick={() => onSelectTemplate(tpl.id)}
                className={`rounded-[11px] border p-3 text-start cursor-pointer space-y-1 ${
                  active
                    ? "border-logo-primary bg-logo-primary/[0.06]"
                    : "border-mid-gray/30 bg-background hover:bg-mid-gray/[0.05]"
                }`}
              >
                <div className="text-[13.5px] font-semibold text-text">
                  {tpl.name}
                </div>
                <div className="text-[12px] text-mid-gray">{preview}</div>
              </button>
            );
          })}
        </div>

        {editingTemplates ? (
          <p className="text-[12px] text-mid-gray">
            {t("meetings.notes.citeRule")}
          </p>
        ) : null}

        <div className="flex justify-end">
          <button
            type="button"
            onClick={() => onStartWithTemplate?.(selectedTemplateId)}
            className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
          >
            {t("meetings.offer.start")}
          </button>
        </div>
      </section>
    </div>
  );
};

export const FoldersAndTemplatesView: React.FC = () => {
  const [folders, setFolders] = useState<LocalMeetingFolder[]>(DEFAULT_FOLDERS);
  const [templates] = useState<MeetingNoteTemplate[]>(DEFAULT_TEMPLATES);
  const [selectedTemplateId, setSelectedTemplateId] = useState("one-to-one");

  return (
    <FoldersAndTemplatesSection
      folders={folders}
      templates={templates}
      selectedTemplateId={selectedTemplateId}
      onToggleFolder={(id) =>
        setFolders((prev) =>
          prev.map((f) => (f.id === id ? { ...f, open: !f.open } : f)),
        )
      }
      onAddFolder={(name) =>
        setFolders((prev) => [
          ...prev,
          {
            id: name.toLowerCase().replace(/\s+/g, "-"),
            name,
            open: true,
            meetings: [],
          },
        ])
      }
      onSelectTemplate={setSelectedTemplateId}
    />
  );
};
