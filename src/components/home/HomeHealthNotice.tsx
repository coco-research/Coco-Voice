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
  shortcutFailureCause?: string | null;
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
    if (health.shortcutFailureCause === "permission") {
      return {
        kind: "shortcut_unset",
        messageKey: "homeNotices.v022ShortcutDescPermission",
        actionKey: "accessibility.openSettings",
      };
    }
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

export const HomeHealthNoticesSection: React.FC<{
  health: HomeHealthSnapshot | null | undefined;
  shortcut?: string;
  onNavigate: (section: ShellSectionId) => void;
  onFixStaleAccessibility?: () => void;
  onOpenSystemSettings?: () => void;
  onTurnTypingOn?: () => void;
  onRestartNow?: () => void;
}> = ({
  health,
  shortcut = "fn",
  onNavigate,
  onFixStaleAccessibility,
  onOpenSystemSettings,
  onTurnTypingOn,
  onRestartNow,
}) => {
  const { t } = useTranslation();
  const topNotice = resolveTopHomeNotice(health);
  if (!health || !topNotice) return null;

  if (topNotice.kind === "cleanup_model_missing") {
    return (
      <HomeHealthNoticeBanner
        notice={topNotice}
        onAction={(n) => {
          if (n.targetSection) onNavigate(n.targetSection);
        }}
      />
    );
  }

  let heading = t(
    "homeNotices.v022StaleHeading",
    "After the update, turn on Coco Voice again in System Settings",
  );
  let body = t(
    "homeNotices.v022StaleBodyKept",
    "macOS kept the old entry. Until it is fixed, your words go to the clipboard.",
  );
  let actionLabel = t("homeNotices.fixThreeSteps", "Fix it (3 steps)");
  let isPrimaryBorder = true;

  if (topNotice.kind === "shortcut_unset") {
    const cause = health.shortcutFailureCause;
    heading = t(
      "homeNotices.v022ShortcutHeading",
      "Your shortcut is not set up",
    );
    if (cause === "permission") {
      body = t(
        "homeNotices.v022ShortcutDescPermission",
        "Turn on Coco Voice in System Settings, then come back. Your shortcut starts by itself.",
      );
      actionLabel = t("accessibility.openSettings", "Open System Settings");
    } else if (cause === "rejected" || cause === "other") {
      body = t("homeNotices.v022ShortcutDescFailed", {
        shortcut,
        defaultValue: "{{shortcut}} couldn't be set up.",
      });
      actionLabel = t("homeNotices.chooseShortcut", "Choose a shortcut");
    } else {
      body = t("homeNotices.v022ShortcutDescWithKey", {
        shortcut,
        defaultValue: "Another app uses {{shortcut}}.",
      });
      actionLabel = t("homeNotices.chooseShortcut", "Choose a shortcut");
    }
    isPrimaryBorder = false;
  } else if (topNotice.kind === "typing_off") {
    heading = t("homeNotices.v022TypingOffHeading", "Typing is off");
    body = t(
      "homeNotices.v022TypingOffDesc",
      "Your words go to the clipboard.",
    );
    actionLabel = t("homeNotices.turnTypingOn", "Turn typing on");
    isPrimaryBorder = false;
  } else if (topNotice.kind === "update_ready") {
    heading = t("homeNotices.updateReady", {
      version: topNotice.version ?? "",
      defaultValue: "Update ready: {{version}}",
    });
    body = t("homeNotices.v022UpdateReadyDesc", "Installs when you restart.");
    actionLabel = t("homeNotices.restartNow", "Restart now");
    isPrimaryBorder = false;
  }

  const handleAction = () => {
    if (topNotice.kind === "stale_accessibility") {
      if (onFixStaleAccessibility) {
        onFixStaleAccessibility();
        return;
      }
      onNavigate("settings");
      return;
    }
    if (
      topNotice.kind === "shortcut_unset" &&
      health.shortcutFailureCause === "permission"
    ) {
      onOpenSystemSettings?.();
      return;
    }
    if (topNotice.kind === "typing_off") {
      if (onTurnTypingOn) {
        onTurnTypingOn();
        return;
      }
      onNavigate("settings");
      return;
    }
    if (topNotice.kind === "update_ready") {
      onRestartNow?.();
      return;
    }
    if (topNotice.targetSection) {
      onNavigate(topNotice.targetSection);
    }
  };

  return (
    <section
      role="status"
      data-testid={`home-notice-${topNotice.kind}`}
      className={`rounded-[14px] bg-background p-4 space-y-2 ${
        isPrimaryBorder
          ? "border-[1.5px] border-logo-primary"
          : "border border-mid-gray/35"
      }`}
    >
      <h3 className="text-[15px] font-semibold text-text">{heading}</h3>
      <p className="text-[13px] leading-[19px] text-mid-gray">{body}</p>
      <div className="pt-0.5">
        <button
          type="button"
          onClick={handleAction}
          className={
            isPrimaryBorder
              ? "inline-flex min-h-[30px] items-center rounded-[9px] bg-logo-primary px-3 text-[12.5px] font-semibold text-white cursor-pointer hover:opacity-90"
              : "inline-flex min-h-[30px] items-center rounded-[8px] border border-mid-gray/45 bg-background px-3 text-[12px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          }
        >
          {actionLabel}
        </button>
      </div>
    </section>
  );
};
