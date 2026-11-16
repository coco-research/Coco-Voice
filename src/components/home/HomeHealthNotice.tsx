import React from "react";
import { useTranslation } from "react-i18next";
import type { ShellSectionId } from "../layout/shellRoutes";

export type HomeNoticeKind =
  | "stale_accessibility"
  | "shortcut_unset"
  | "typing_off"
  | "cleanup_model_missing"
  | "update_ready";

export interface HomeHealthSnapshot {
  staleAccessibility?: boolean;
  shortcutUnset?: boolean;
  typingOff?: boolean;
  cleanupEnabledWithoutLocalModel?: boolean;
  cleanupNoticeAlreadyShown?: boolean;
  updateReadyVersion?: string | null;
}

export interface ResolvedHomeNotice {
  kind: HomeNoticeKind;
  messageKey: string;
  actionKey: string;
  targetSection?: ShellSectionId;
  version?: string;
}

// Returns at most ONE notice in strict priority order so Home never stacks banners.
// G8: cleanup_model_missing shows at most once and never starts any model download.
export function resolveTopHomeNotice(
  health: HomeHealthSnapshot | null | undefined,
): ResolvedHomeNotice | null {
  if (!health) return null;
  if (health.staleAccessibility) {
    return {
      kind: "stale_accessibility",
      messageKey: "homeNotices.staleAccessibility",
      actionKey: "homeNotices.fixThreeSteps",
      targetSection: "settings",
    };
  }
  if (health.shortcutUnset) {
    return {
      kind: "shortcut_unset",
      messageKey: "homeNotices.shortcutUnset",
      actionKey: "homeNotices.chooseShortcut",
      targetSection: "settings",
    };
  }
  if (health.typingOff) {
    return {
      kind: "typing_off",
      messageKey: "homeNotices.typingOff",
      actionKey: "homeNotices.turnTypingOn",
      targetSection: "settings",
    };
  }
  if (
    health.cleanupEnabledWithoutLocalModel &&
    !health.cleanupNoticeAlreadyShown
  ) {
    return {
      kind: "cleanup_model_missing",
      messageKey: "homeNotices.cleanupModelMissing",
      actionKey: "homeNotices.openModels",
      targetSection: "models",
    };
  }
  if (health.updateReadyVersion) {
    return {
      kind: "update_ready",
      messageKey: "homeNotices.updateReady",
      actionKey: "homeNotices.restartNow",
      version: health.updateReadyVersion,
    };
  }
  return null;
}

export const HomeHealthNoticeBanner: React.FC<{
  notice: ResolvedHomeNotice;
  onAction: (notice: ResolvedHomeNotice) => void;
}> = ({ notice, onAction }) => {
  const { t } = useTranslation();

  return (
    <div
      role="status"
      data-testid={`home-notice-${notice.kind}`}
      className="flex flex-wrap items-center justify-between gap-2.5 rounded-[12px] border border-amber-500/35 bg-amber-500/10 px-3.5 py-2.5"
    >
      <p className="text-[13px] font-medium text-text">
        {t(notice.messageKey, { version: notice.version ?? "" })}
      </p>
      <button
        type="button"
        onClick={() => onAction(notice)}
        className="shrink-0 rounded-[8px] border border-mid-gray/45 bg-background px-3 py-1 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
      >
        {t(notice.actionKey)}
      </button>
    </div>
  );
};
