import React from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";

export const UPDATE_ROLLBACK_REPORT_URL =
  "https://github.com/coco-research/Coco-Voice/issues/new?template=bug_report.md";

export function buildRollbackReportUrl(): string {
  return UPDATE_ROLLBACK_REPORT_URL;
}

export async function openRollbackReport(
  opener: (url: string) => Promise<void> = (u) => openUrl(u),
): Promise<string> {
  const url = buildRollbackReportUrl();
  try {
    await opener(url);
  } catch (error) {
    console.error("Failed to open rollback report URL:", error);
  }
  return url;
}

export function parseSemverTriplet(
  version: string,
): [number, number, number] | null {
  const clean = version.trim().replace(/^v/i, "");
  if (!clean) return null;
  const core = clean.split(/[-+]/)[0] ?? clean;
  const parts = core.split(".");
  const major = Number(parts[0]);
  const minor = Number(parts[1] ?? 0);
  const patch = Number(parts[2] ?? 0);
  if (
    !Number.isInteger(major) ||
    !Number.isInteger(minor) ||
    !Number.isInteger(patch) ||
    major < 0 ||
    minor < 0 ||
    patch < 0
  ) {
    return null;
  }
  return [major, minor, patch];
}

export function compareSemver(a: string, b: string): number | null {
  const pa = parseSemverTriplet(a);
  const pb = parseSemverTriplet(b);
  if (!pa || !pb) return null;
  for (let i = 0; i < 3; i += 1) {
    if (pa[i] !== pb[i]) return pa[i] > pb[i] ? 1 : -1;
  }
  return 0;
}

export type ClientUpdateNoticeState =
  | { kind: "none" }
  | { kind: "whats_new"; version: string }
  | { kind: "went_back"; rolledBackFrom: string; currentVersion: string };

export function evaluateClientUpdateNotice(
  currentVersion: string,
  lastSeenVersion: string | null | undefined,
  showWhatsNewOnUpdate: boolean,
): ClientUpdateNoticeState {
  const current = currentVersion.trim();
  const stored = (lastSeenVersion ?? "").trim();
  if (!current || stored === current) {
    return { kind: "none" };
  }
  if (stored) {
    const cmp = compareSemver(stored, current);
    if (cmp !== null && cmp > 0) {
      // Plan A rule: went-back card is unused until R2 (rendered from 36b fixture only).
      return { kind: "none" };
    }
  }
  if (showWhatsNewOnUpdate) {
    return { kind: "whats_new", version: current };
  }
  return { kind: "none" };
}

export interface WhatsNewGateDecision {
  shouldShow: boolean;
  rolledBackFrom: string | null;
  noticeKind: "none" | "whats_new" | "went_back";
}

export function resolveWhatsNewGateDecision(input: {
  currentVersion: string;
  lastSeenVersion: string | null | undefined;
  showWhatsNewOnUpdate: boolean;
  healthRolledBackFrom?: string | null;
}): WhatsNewGateDecision {
  const clientNotice = evaluateClientUpdateNotice(
    input.currentVersion,
    input.lastSeenVersion,
    input.showWhatsNewOnUpdate,
  );
  // Plan A rule: went-back card is unused until R2 (rendered from 36b fixture only).
  if (clientNotice.kind === "whats_new") {
    return {
      shouldShow: true,
      rolledBackFrom: null,
      noticeKind: "whats_new",
    };
  }
  return {
    shouldShow: false,
    rolledBackFrom: null,
    noticeKind: "none",
  };
}

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
  showTitle?: boolean;
  onReport?: () => void;
}> = ({ failedVersion, currentVersion, showTitle = true, onReport }) => {
  const { t } = useTranslation();
  const handleReport = () => {
    if (onReport) {
      onReport();
      return;
    }
    void openRollbackReport();
  };

  return (
    <section
      role="status"
      data-testid="update-rollback-notice"
      className="rounded-[12px] border border-mid-gray/35 bg-background p-3.5 space-y-2"
    >
      {showTitle ? (
        <h4 className="text-[14px] font-semibold text-text">
          {t("whatsNew.rollbackTitle")}
        </h4>
      ) : null}
      <p className="text-[13px] leading-[19px] text-mid-gray">
        {t("whatsNew.rollbackMessage", {
          failedVersion,
          currentVersion,
          defaultValue:
            "{{failedVersion}} did not start, so Coco Voice went back to {{currentVersion}}. Your settings and history are unchanged.",
        })}
      </p>
      <button
        type="button"
        onClick={handleReport}
        className="rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
      >
        {t("whatsNew.reportIt", "Report it")}
      </button>
    </section>
  );
};
