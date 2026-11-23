import React, { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../../hooks/useSettings";
import { findReleaseNoteToShow } from "./releaseNotes";
import type { ReleaseNote } from "./releaseNotes";
import {
  openRollbackReport,
  resolveWhatsNewGateDecision,
} from "./UpdateAndTraySurfaces";
import { WhatsNewModal } from "./WhatsNewModal";

function buildCurrentNote(
  currentVersion: string,
  lastSeenVersion: string,
): ReleaseNote {
  const matched = findReleaseNoteToShow({ currentVersion, lastSeenVersion });
  if (matched && matched.version === currentVersion) {
    return matched;
  }
  return {
    version: currentVersion,
    markdown: "",
  };
}

export const WhatsNewGate: React.FC = () => {
  const { settings, isLoading, updateSetting } = useSettings();
  const [note, setNote] = useState<ReleaseNote | null>(null);
  const [rolledBackFrom, setRolledBackFrom] = useState<string | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const dismissedVersionRef = useRef<string | null>(null);

  useEffect(() => {
    const handleManualOpen = () => {
      void (async () => {
        const currentVersion = await getVersion()
          .then((v) => v?.trim() ?? "")
          .catch(() => "");
        if (!currentVersion) return;
        setNote(buildCurrentNote(currentVersion, ""));
        setIsOpen(true);
      })();
    };

    window.addEventListener("coco-open-whats-new", handleManualOpen);
    return () => {
      window.removeEventListener("coco-open-whats-new", handleManualOpen);
    };
  }, []);

  useEffect(() => {
    if (isLoading || !settings) {
      setIsOpen(false);
      setNote(null);
      return;
    }

    let cancelled = false;

    const loadReleaseNote = async () => {
      try {
        const currentVersion = await getVersion()
          .then((v) => v?.trim() ?? "")
          .catch(() => "");
        if (!currentVersion || cancelled) return;

        const rawHealth = await invoke<Record<string, unknown>>(
          "home_health_status",
        )
          .catch(() => invoke<Record<string, unknown>>("get_home_health"))
          .catch(() => null);

        if (cancelled) return;

        const healthRollbackVer =
          rawHealth && typeof rawHealth === "object"
            ? ((rawHealth.rolledBackFromVersion as string | null | undefined) ??
              null)
            : null;

        const lastSeen = settings.whats_new_last_seen_version ?? "";
        const decision = resolveWhatsNewGateDecision({
          currentVersion,
          lastSeenVersion: lastSeen,
          showWhatsNewOnUpdate: Boolean(settings.show_whats_new_on_update),
          healthRolledBackFrom: healthRollbackVer,
        });

        setRolledBackFrom(decision.rolledBackFrom);

        if (
          !decision.shouldShow ||
          dismissedVersionRef.current === currentVersion
        ) {
          setIsOpen(false);
          setNote(null);
          return;
        }

        setNote(buildCurrentNote(currentVersion, lastSeen));
        setIsOpen(true);
      } catch (error) {
        console.error("Failed to load release notes:", error);
      }
    };

    void loadReleaseNote();

    return () => {
      cancelled = true;
    };
  }, [
    isLoading,
    settings,
    settings?.show_whats_new_on_update,
    settings?.whats_new_last_seen_version,
  ]);

  const dismiss = () => {
    if (!note) return;

    dismissedVersionRef.current = note.version;
    setIsOpen(false);
    void updateSetting("whats_new_last_seen_version", note.version);
  };

  if (!note) return null;

  return (
    <WhatsNewModal
      note={note}
      open={isOpen}
      onDismiss={dismiss}
      rolledBackFromVersion={rolledBackFrom}
      onReportRollback={
        rolledBackFrom
          ? () => {
              void openRollbackReport();
            }
          : undefined
      }
    />
  );
};
