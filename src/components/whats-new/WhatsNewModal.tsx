import React from "react";
import { useTranslation } from "react-i18next";
import { MarkdownContent } from "./MarkdownContent";
import type { ReleaseNote } from "./releaseNotes";
import { UpdateRollbackNotice } from "./UpdateAndTraySurfaces";

export interface WhatsNewModalProps {
  note: ReleaseNote;
  open: boolean;
  onDismiss: () => void;
  rolledBackFromVersion?: string | null;
  onReportRollback?: () => void;
}

export const WhatsNewModal: React.FC<WhatsNewModalProps> = ({
  note,
  open,
  onDismiss,
  rolledBackFromVersion = null,
  onReportRollback,
}) => {
  const { t } = useTranslation();

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/35 p-6 backdrop-blur-[1px]"
      onClick={(e) => {
        if (e.target === e.currentTarget) onDismiss();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="whats-new-heading"
        data-testid="whats-new-modal"
        className="w-full max-w-[540px] overflow-hidden rounded-[16px] border border-mid-gray/30 bg-background shadow-xl"
      >
        <div className="p-6 space-y-3.5">
          <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
            {t("whatsNew.eyebrow", {
              version: note.version,
              defaultValue: "What's new in {{version}}",
            })}
          </p>
          <h2
            id="whats-new-heading"
            className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text"
          >
            {t("whatsNew.v020Heading", "Meetings, and a new layout")}
          </h2>
          <p className="text-[13.5px] leading-[20px] text-mid-gray">
            {t(
              "whatsNew.v020Body",
              "Record a meeting from Home. Meetings and dictation now have their own sections.",
            )}
          </p>
          {note.markdown.trim().length > 0 ? (
            <div className="max-h-52 overflow-y-auto pt-1">
              <MarkdownContent markdown={note.markdown} />
            </div>
          ) : null}
          <div className="pt-1">
            <button
              type="button"
              onClick={onDismiss}
              className="inline-flex min-h-[34px] items-center rounded-[10px] bg-logo-primary px-4 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              {t("common.close", "Close")}
            </button>
          </div>
          {rolledBackFromVersion ? (
            <div className="pt-2">
              <UpdateRollbackNotice
                failedVersion={rolledBackFromVersion}
                currentVersion={note.version}
                showTitle={false}
                onReport={onReportRollback}
              />
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
};
