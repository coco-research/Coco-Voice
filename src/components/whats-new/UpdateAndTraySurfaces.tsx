import React from "react";
import { useTranslation } from "react-i18next";

export interface TrayMenuEntry {
  id:
    | "start_meeting"
    | "paste_last_take"
    | "copy_last_take"
    | "open_app"
    | "check_updates"
    | "quit";
  label: string;
  disabled?: boolean;
}

export function buildTrayMenuEntries(
  t: (key: string) => string,
  hasLastTake = true,
): TrayMenuEntry[] {
  return [
    { id: "start_meeting", label: t("tray.startRecordingMeeting") },
    {
      id: "paste_last_take",
      label: t("tray.pasteLastTake"),
      disabled: !hasLastTake,
    },
    {
      id: "copy_last_take",
      label: t("tray.copyLastTake"),
      disabled: !hasLastTake,
    },
    { id: "open_app", label: t("tray.openApp") },
    { id: "check_updates", label: t("tray.checkUpdates") },
    { id: "quit", label: t("tray.quit") },
  ];
}

export const TrayMenuSurface: React.FC<{
  hasLastTake?: boolean;
  onSelect?: (id: TrayMenuEntry["id"]) => void;
}> = ({ hasLastTake = true, onSelect }) => {
  const { t } = useTranslation();
  const entries = buildTrayMenuEntries(t, hasLastTake);

  return (
    <div
      role="menu"
      data-testid="tray-menu-surface"
      className="w-64 rounded-[12px] border border-mid-gray/35 bg-background p-1.5 shadow-md divide-y divide-mid-gray/15"
    >
      {entries.map((entry) => (
        <button
          key={entry.id}
          type="button"
          role="menuitem"
          disabled={entry.disabled}
          onClick={() => onSelect?.(entry.id)}
          className="flex w-full items-center justify-between rounded-[7px] px-2.5 py-1.5 text-start text-[13px] font-medium text-text cursor-pointer hover:bg-mid-gray/10 disabled:opacity-45"
        >
          <span>{entry.label}</span>
        </button>
      ))}
    </div>
  );
};

export function shouldShowWhatsNewOnce(
  currentVersion: string,
  lastSeenVersion: string | null | undefined,
): boolean {
  const cleanCurrent = currentVersion.trim();
  if (!cleanCurrent) return false;
  return cleanCurrent !== (lastSeenVersion ?? "").trim();
}

export const UpdateRollbackNotice: React.FC<{
  failedVersion: string;
  currentVersion: string;
  onReport?: () => void;
}> = ({ failedVersion, currentVersion, onReport }) => {
  const { t } = useTranslation();

  return (
    <section
      role="status"
      data-testid="update-rollback-notice"
      className="rounded-[12px] border border-amber-500/35 bg-amber-500/10 p-3.5 space-y-2"
    >
      <h4 className="text-[14px] font-semibold text-text">
        {t("whatsNew.rollbackTitle")}
      </h4>
      <p className="text-[13px] leading-[19px] text-text">
        {t("whatsNew.rollbackMessage", { failedVersion, currentVersion })}
      </p>
      <button
        type="button"
        onClick={onReport}
        className="rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
      >
        {t("whatsNew.reportIt")}
      </button>
    </section>
  );
};
