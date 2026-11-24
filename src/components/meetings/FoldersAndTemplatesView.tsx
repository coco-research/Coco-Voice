import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type MeetingFolderView,
  type MeetingTemplateView,
} from "../../bindings.meetings";
import { formatShortDateDddDDMon } from "./UpcomingMeetings";

const DOT_FILLED = "●";
const DOT_OPEN = "○";
const MID_SEP = " · ";
const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";

export interface LocalMeetingFolder {
  id: string;
  numericId?: number;
  name: string;
  open: boolean;
  lastTemplateId?: string | null;
  meetings: Array<{
    id: number;
    title: string;
    dateLabel: string;
    notesReady?: boolean;
  }>;
}

export interface MeetingNoteTemplate {
  id: string;
  numericId?: number;
  name: string;
  sections: string[];
  body?: string;
}

// Local folders are strictly flat (one level only); slash separators are flattened.
export function sanitizeFlatFolderName(raw: string): string {
  return raw
    .replace(/[\\/]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 60);
}

export function parseTemplateSectionsFromBody(body: string): string[] {
  return body
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("#"))
    .map((l) => l.replace(/^#+\s*/, "").trim())
    .filter(Boolean);
}

export function buildTemplateBodyFromSections(sections: string[]): string {
  return sections
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => `# ${s}`)
    .join("\n");
}

function formatFolderMeetingDate(
  startedAt: number,
  endedAt: number | null,
  locale = "en",
): string {
  const dateStr = formatShortDateDddDDMon(startedAt, locale);
  if (endedAt !== null && endedAt >= startedAt) {
    const mins = Math.max(1, Math.round((endedAt - startedAt) / 60));
    return `${dateStr}${MID_SEP}${mins} min`;
  }
  return dateStr;
}

function toLocalFolder(
  row: MeetingFolderView,
  openId: string | null,
): LocalMeetingFolder {
  const idStr = String(row.id);
  return {
    id: idStr,
    numericId: row.id,
    name: row.name,
    open: openId === idStr,
    lastTemplateId:
      row.last_template_id !== null ? String(row.last_template_id) : null,
    meetings: row.meetings.map((m) => ({
      id: m.id,
      title: m.title,
      dateLabel: formatFolderMeetingDate(m.started_at, m.ended_at),
      notesReady: m.notes_ready,
    })),
  };
}

function toLocalTemplate(row: MeetingTemplateView): MeetingNoteTemplate {
  return {
    id: String(row.id),
    numericId: row.id,
    name: row.name,
    sections: parseTemplateSectionsFromBody(row.body),
    body: row.body,
  };
}

export interface FoldersAndTemplatesSectionProps {
  folders: LocalMeetingFolder[];
  templates: MeetingNoteTemplate[];
  selectedTemplateId: string;
  templateDialogOpen?: boolean;
  templateMeetingTitle?: string;
  onOpenTemplateDialog?: () => void;
  onCloseTemplateDialog?: () => void;
  onToggleFolder: (folderId: string) => void;
  onAddFolder: (name: string) => void;
  onRenameFolder?: (folderId: string, nextName: string) => void;
  onDeleteFolder?: (folderId: string) => void;
  onSelectTemplate: (templateId: string) => void;
  onStartWithTemplate?: (templateId: string) => void;
  onCreateTemplate?: (name: string, sections: string[]) => void;
  onDeleteTemplate?: (templateId: string) => void;
}

export const FoldersAndTemplatesSection: React.FC<
  FoldersAndTemplatesSectionProps
> = ({
  folders,
  templates,
  selectedTemplateId,
  templateDialogOpen = true,
  templateMeetingTitle,
  onOpenTemplateDialog,
  onCloseTemplateDialog,
  onToggleFolder,
  onAddFolder,
  onRenameFolder,
  onDeleteFolder,
  onSelectTemplate,
  onStartWithTemplate,
  onCreateTemplate,
  onDeleteTemplate,
}) => {
  const { t } = useTranslation();
  const [folderDraft, setFolderDraft] = useState("");
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [editingTemplates, setEditingTemplates] = useState(false);
  const [newTplName, setNewTplName] = useState("");
  const [newTplSections, setNewTplSections] = useState("");

  const activeFolder = folders.find((f) => f.open) ?? folders[0];

  const handleCreateFolder = () => {
    const clean = sanitizeFlatFolderName(folderDraft);
    if (!clean) return;
    onAddFolder(clean);
    setFolderDraft("");
  };

  const commitRenameFolder = (folderId: string) => {
    const clean = sanitizeFlatFolderName(renameDraft);
    setRenamingId(null);
    if (clean && onRenameFolder) {
      onRenameFolder(folderId, clean);
    }
  };

  const handleCreateTemplate = () => {
    const name = sanitizeFlatFolderName(newTplName);
    const sections = newTplSections
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    if (!name || sections.length === 0 || !onCreateTemplate) return;
    onCreateTemplate(name, sections);
    setNewTplName("");
    setNewTplSections("");
  };

  return (
    <div
      data-testid="frame-11-folders-templates"
      className="max-w-3xl w-full mx-auto space-y-4"
    >
      <div>
        <p className={EYEBROW}>{t("meetings.sidebar")}</p>
        <h2 className="mt-1 text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("sidebar.foldersAndTemplates")}
        </h2>
      </div>

      <section className="grid grid-cols-1 md:grid-cols-[230px_1fr] rounded-[14px] border border-mid-gray/25 bg-background overflow-hidden">
        <div className="border-b md:border-b-0 md:border-e border-mid-gray/20 p-3.5 space-y-3 bg-mid-gray/[0.03]">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-[13.5px] font-semibold text-text">
              {t("meetings.notes.foldersHeading", "Folders")}
            </h3>
            <span className="text-[11px] text-mid-gray">
              {t("meetings.notes.flatFoldersHint", "Flat list")}
            </span>
          </div>

          <div className="space-y-1">
            {folders.map((folder) => {
              const isOpen = folder.id === activeFolder?.id || folder.open;
              const dot = isOpen ? DOT_FILLED : DOT_OPEN;
              const stateLabel = isOpen
                ? t("meetings.notes.openFolder", "Open")
                : t("meetings.notes.closedFolder", "Closed");
              return (
                <div key={folder.id} className="space-y-1">
                  {renamingId === folder.id ? (
                    <div className="flex items-center gap-1">
                      <input
                        type="text"
                        value={renameDraft}
                        onChange={(e) => setRenameDraft(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitRenameFolder(folder.id);
                          if (e.key === "Escape") setRenamingId(null);
                        }}
                        aria-label={t(
                          "meetings.notes.renameFolder",
                          "Rename folder",
                        )}
                        className="min-w-0 flex-1 rounded-[7px] border border-logo-primary bg-background px-2 py-1 text-[12px] text-text"
                      />
                    </div>
                  ) : (
                    <button
                      type="button"
                      onClick={() => onToggleFolder(folder.id)}
                      className={`flex w-full items-center justify-between rounded-[9px] px-2.5 py-1.5 text-start cursor-pointer ${
                        isOpen
                          ? "bg-logo-primary/10 text-text"
                          : "hover:bg-mid-gray/10 text-text/85"
                      }`}
                    >
                      <span className="truncate text-[13px] font-semibold">
                        {folder.name}
                      </span>
                      <span className="inline-flex shrink-0 items-center gap-1 text-[11.5px] font-medium text-mid-gray">
                        <span aria-hidden="true">{dot}</span>
                        <span>{stateLabel}</span>
                      </span>
                    </button>
                  )}
                </div>
              );
            })}
            {folders.length === 0 ? (
              <p className="text-[12px] text-mid-gray">
                {t("meetings.notes.noFoldersYet", "No folders yet.")}
              </p>
            ) : null}
          </div>

          <div className="flex items-center gap-1.5 pt-1">
            <input
              type="text"
              value={folderDraft}
              onChange={(e) => setFolderDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreateFolder();
              }}
              placeholder={t(
                "meetings.notes.newFolderPlaceholder",
                "New folder",
              )}
              aria-label={t("meetings.notes.foldersHeading", "Folders")}
              className="min-w-0 flex-1 rounded-[8px] border border-mid-gray/35 bg-background px-2.5 py-1 text-[12px] text-text"
            />
            <button
              type="button"
              onClick={handleCreateFolder}
              className="inline-flex min-h-[28px] items-center rounded-[8px] border border-mid-gray/45 bg-background px-2 text-[11.5px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
            >
              {t("meetings.notes.createFolder", "Add")}
            </button>
          </div>
        </div>

        <div className="p-4 space-y-2.5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h4 className="text-[14px] font-semibold text-text">
              {activeFolder
                ? t("meetings.notes.folderWithCount", {
                    defaultValue: "{{name}} · {{count}} meetings",
                    name: activeFolder.name,
                    count: activeFolder.meetings.length,
                  })
                : t("meetings.list.heading")}
            </h4>
            <div className="flex items-center gap-1.5">
              {activeFolder && onOpenTemplateDialog ? (
                <button
                  type="button"
                  data-testid="open-note-template-dialog"
                  onClick={onOpenTemplateDialog}
                  className="rounded-[7px] bg-logo-primary px-2.5 py-0.5 text-[11.5px] font-semibold text-white cursor-pointer hover:opacity-90"
                >
                  {t("meetings.notes.noteTemplateDialogTitle", "Note template")}
                </button>
              ) : null}
              {activeFolder && onRenameFolder ? (
                <button
                  type="button"
                  onClick={() => {
                    setRenamingId(activeFolder.id);
                    setRenameDraft(activeFolder.name);
                  }}
                  className="rounded-[7px] border border-mid-gray/35 px-2 py-0.5 text-[11.5px] font-medium text-mid-gray cursor-pointer hover:text-text"
                >
                  {t("meetings.notes.renameFolder", "Rename")}
                </button>
              ) : null}
              {activeFolder && onDeleteFolder ? (
                <button
                  type="button"
                  onClick={() => onDeleteFolder(activeFolder.id)}
                  className="rounded-[7px] border border-mid-gray/35 px-2 py-0.5 text-[11.5px] font-medium text-mid-gray cursor-pointer hover:text-red-600"
                >
                  {t("meetings.list.delete")}
                </button>
              ) : null}
            </div>
          </div>
          {activeFolder && activeFolder.meetings.length > 0 ? (
            <ul className="divide-y divide-mid-gray/15">
              {activeFolder.meetings.map((m) => {
                const ready = m.notesReady !== false;
                return (
                  <li
                    key={m.id}
                    className="flex flex-wrap items-center justify-between gap-2 py-2 text-[13px]"
                  >
                    <div>
                      <p className="font-semibold text-text">{m.title}</p>
                      <p className="text-[11.5px] text-mid-gray">
                        {m.dateLabel}
                      </p>
                    </div>
                    <span
                      className={`inline-flex items-center gap-1 text-[11.5px] font-medium ${
                        ready ? "text-emerald-600" : "text-mid-gray"
                      }`}
                    >
                      <span aria-hidden="true">
                        {ready ? DOT_FILLED : DOT_OPEN}
                      </span>
                      <span>
                        {ready
                          ? t("meetings.list.notesReady")
                          : t("meetings.list.transcriptOnly")}
                      </span>
                    </span>
                  </li>
                );
              })}
            </ul>
          ) : (
            <p className="text-[12.5px] text-mid-gray">
              {t("meetings.list.emptyList")}
            </p>
          )}
        </div>
      </section>

      {templateDialogOpen ? (
        <div
          className={
            onCloseTemplateDialog
              ? "fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-[1px]"
              : ""
          }
        >
          <section
            role="dialog"
            aria-modal={onCloseTemplateDialog ? "true" : undefined}
            data-testid="note-template-dialog"
            aria-label={t(
              "meetings.notes.noteTemplateDialogTitle",
              "Note template",
            )}
            className={`w-full rounded-[16px] border border-mid-gray/30 bg-background p-4 shadow-sm space-y-3 ${
              onCloseTemplateDialog ? "max-w-[460px] shadow-xl p-5" : ""
            }`}
          >
            <div className="flex items-start justify-between gap-2">
              <div>
                <h3 className="text-[16px] font-semibold text-text">
                  {t("meetings.notes.noteTemplateDialogTitle", "Note template")}
                </h3>
                <p className="text-[12.5px] text-mid-gray">
                  {templateMeetingTitle?.trim()
                    ? t("meetings.notes.noteTemplateDialogForMeeting", {
                        defaultValue: "Starting notes for {{meeting}}.",
                        meeting: templateMeetingTitle.trim(),
                      })
                    : t(
                        "meetings.notes.noteTemplateDialogSubtitleGeneric",
                        "Starting notes for this meeting.",
                      )}
                </p>
              </div>
              {onCloseTemplateDialog ? (
                <button
                  type="button"
                  onClick={onCloseTemplateDialog}
                  className="rounded-[7px] border border-mid-gray/35 px-2 py-0.5 text-[11.5px] font-medium text-mid-gray cursor-pointer hover:text-text"
                >
                  {t("meetings.consent.cancel")}
                </button>
              ) : null}
            </div>

            <div className="divide-y divide-mid-gray/15 rounded-[12px] border border-mid-gray/25 bg-mid-gray/[0.03]">
              {templates.map((tpl) => {
                const active = tpl.id === selectedTemplateId;
                const preview = tpl.sections.join(MID_SEP);
                return (
                  <div
                    key={tpl.id}
                    className="flex items-center justify-between gap-2 px-3.5 py-2.5"
                  >
                    <button
                      type="button"
                      aria-pressed={active}
                      onClick={() => onSelectTemplate(tpl.id)}
                      className="flex min-w-0 flex-1 items-center justify-between gap-3 text-start cursor-pointer hover:opacity-90"
                    >
                      <div>
                        <div className="text-[13.5px] font-semibold text-text">
                          {tpl.name}
                        </div>
                        <div className="text-[12px] text-mid-gray">
                          {preview}
                        </div>
                      </div>
                      <span
                        className={`inline-flex shrink-0 items-center gap-1 text-[12px] font-semibold ${
                          active ? "text-logo-primary" : "text-mid-gray"
                        }`}
                      >
                        <span aria-hidden="true">
                          {active ? DOT_FILLED : DOT_OPEN}
                        </span>
                        <span>
                          {active
                            ? t("meetings.notes.templateSelected", "Selected")
                            : t(
                                "meetings.notes.templateNotSelected",
                                "Not selected",
                              )}
                        </span>
                      </span>
                    </button>
                    {editingTemplates && onDeleteTemplate ? (
                      <button
                        type="button"
                        onClick={() => onDeleteTemplate(tpl.id)}
                        className="shrink-0 rounded-[7px] border border-mid-gray/35 px-2 py-0.5 text-[11px] font-medium text-mid-gray cursor-pointer hover:text-red-600"
                      >
                        {t("meetings.list.delete")}
                      </button>
                    ) : null}
                  </div>
                );
              })}
              {templates.length === 0 ? (
                <p className="px-3.5 py-2.5 text-[12.5px] text-mid-gray">
                  {t("meetings.notes.noTemplatesYet", "No templates yet.")}
                </p>
              ) : null}
            </div>

            {editingTemplates ? (
              <div className="space-y-2 rounded-[10px] border border-mid-gray/20 bg-mid-gray/[0.03] p-3">
                <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
                  <input
                    type="text"
                    value={newTplName}
                    onChange={(e) => setNewTplName(e.target.value)}
                    placeholder={t(
                      "meetings.notes.templateNamePlaceholder",
                      "Template name",
                    )}
                    aria-label={t(
                      "meetings.notes.templateNamePlaceholder",
                      "Template name",
                    )}
                    className="rounded-[8px] border border-mid-gray/35 bg-background px-2.5 py-1.5 text-[12.5px] text-text"
                  />
                  <input
                    type="text"
                    value={newTplSections}
                    onChange={(e) => setNewTplSections(e.target.value)}
                    placeholder={t(
                      "meetings.notes.templateSectionsPlaceholder",
                      "Headings separated by commas",
                    )}
                    aria-label={t(
                      "meetings.notes.templateSectionsPlaceholder",
                      "Headings separated by commas",
                    )}
                    className="rounded-[8px] border border-mid-gray/35 bg-background px-2.5 py-1.5 text-[12.5px] text-text"
                  />
                </div>
                <div className="flex items-center justify-between gap-2">
                  <p className="text-[12px] text-mid-gray">
                    {t("meetings.notes.citeRule")}
                  </p>
                  {onCreateTemplate ? (
                    <button
                      type="button"
                      onClick={handleCreateTemplate}
                      className="rounded-[8px] border border-mid-gray/45 px-2.5 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                    >
                      {t("meetings.notes.addTemplate", "Add template")}
                    </button>
                  ) : null}
                </div>
              </div>
            ) : null}

            <div className="flex flex-wrap items-center gap-2 pt-0.5">
              <button
                type="button"
                onClick={() => onStartWithTemplate?.(selectedTemplateId)}
                className="inline-flex min-h-[32px] items-center rounded-[9px] bg-logo-primary px-3.5 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
              >
                {t("meetings.offer.start")}
              </button>
              <button
                type="button"
                onClick={() => setEditingTemplates((v) => !v)}
                className="inline-flex min-h-[32px] items-center rounded-[9px] border border-mid-gray/45 px-3 text-[12.5px] font-medium text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("meetings.notes.editTemplates", "Edit templates")}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </div>
  );
};

export const FoldersAndTemplatesView: React.FC = () => {
  const [folders, setFolders] = useState<LocalMeetingFolder[]>([]);
  const [templates, setTemplates] = useState<MeetingNoteTemplate[]>([]);
  const [openFolderId, setOpenFolderId] = useState<string | null>(null);
  const [selectedTemplateId, setSelectedTemplateId] = useState<string>("");
  const [templateDialogOpen, setTemplateDialogOpen] = useState(false);

  const reload = useCallback(
    async (preferredOpenId?: string | null) => {
      const [folderRes, tplRes] = await Promise.all([
        commands.listMeetingFolders(),
        commands.listMeetingTemplates(),
      ]);
      if (tplRes.status === "ok") {
        const mapped = tplRes.data.map(toLocalTemplate);
        setTemplates(mapped);
        setSelectedTemplateId((prev) => prev || mapped[0]?.id || "");
      }
      if (folderRes.status === "ok") {
        const targetOpen =
          preferredOpenId ??
          openFolderId ??
          (folderRes.data[0] ? String(folderRes.data[0].id) : null);
        setOpenFolderId(targetOpen);
        const mappedFolders = folderRes.data.map((row) =>
          toLocalFolder(row, targetOpen),
        );
        setFolders(mappedFolders);
        const active = mappedFolders.find((f) => f.id === targetOpen);
        if (active?.lastTemplateId) {
          setSelectedTemplateId(active.lastTemplateId);
        }
      }
    },
    [openFolderId],
  );

  useEffect(() => {
    void reload();
  }, [reload]);

  const activeFolder = folders.find((f) => f.open) ?? folders[0];
  const dialogMeetingTitle =
    activeFolder?.meetings[0]?.title ?? activeFolder?.name;

  return (
    <FoldersAndTemplatesSection
      folders={folders}
      templates={templates}
      selectedTemplateId={selectedTemplateId}
      templateDialogOpen={templateDialogOpen}
      templateMeetingTitle={dialogMeetingTitle}
      onOpenTemplateDialog={() => setTemplateDialogOpen(true)}
      onCloseTemplateDialog={() => setTemplateDialogOpen(false)}
      onToggleFolder={(id) => {
        setOpenFolderId(id);
        setFolders((prev) => prev.map((f) => ({ ...f, open: f.id === id })));
        const picked = folders.find((f) => f.id === id);
        if (picked?.lastTemplateId) {
          setSelectedTemplateId(picked.lastTemplateId);
        }
      }}
      onAddFolder={(name) => {
        void commands.createMeetingFolder(name).then((res) => {
          if (res.status === "ok") {
            void reload(String(res.data.id));
          }
        });
      }}
      onRenameFolder={(folderId, nextName) => {
        const num = Number(folderId);
        if (!Number.isFinite(num)) return;
        void commands.renameMeetingFolder(num, nextName).then(() => {
          void reload(folderId);
        });
      }}
      onDeleteFolder={(folderId) => {
        const num = Number(folderId);
        if (!Number.isFinite(num)) return;
        void commands.deleteMeetingFolder(num).then(() => {
          void reload(null);
        });
      }}
      onSelectTemplate={(tplId) => {
        setSelectedTemplateId(tplId);
        const active = folders.find((f) => f.open);
        const fid = active?.numericId;
        const tid = Number(tplId);
        if (fid !== undefined && Number.isFinite(tid)) {
          void commands.setFolderLastTemplate(fid, tid);
        }
      }}
      onStartWithTemplate={(tplId) => {
        const active = folders.find((f) => f.open);
        const fid = active?.numericId;
        const tid = Number(tplId);
        const validTid =
          tplId.trim() !== "" && Number.isFinite(tid) && tid > 0 ? tid : null;
        if (fid !== undefined && validTid !== null) {
          void commands.setFolderLastTemplate(fid, validTid);
        }
        void commands.openMeetingConsent(validTid);
      }}
      onCreateTemplate={(name, sections) => {
        const body = buildTemplateBodyFromSections(sections);
        void commands.createMeetingTemplate(name, body).then((res) => {
          if (res.status === "ok") {
            void reload(openFolderId);
            setSelectedTemplateId(String(res.data.id));
          }
        });
      }}
      onDeleteTemplate={(tplId) => {
        const num = Number(tplId);
        if (!Number.isFinite(num)) return;
        void commands.deleteMeetingTemplate(num).then(() => {
          void reload(openFolderId);
        });
      }}
    />
  );
};
