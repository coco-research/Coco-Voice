import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import type { MeetingsUi } from "../meetings/gate";
import { HomeView } from "../home/HomeView";
import {
  SettingsV022Page,
  type SettingsTarget,
} from "../settings/SettingsV022Page";
import { HistorySettings } from "../settings/history/HistorySettings";
import { ModelsSettings } from "../settings/models/ModelsSettings";
import { WordsAndCleanupV022 } from "../settings/post-processing/PostProcessingSettings";
import {
  isMeetingSection,
  SHELL_ROUTES,
  type ShellSectionId,
} from "./shellNavigation";

export function ShellContent({
  section,
  meetingsUi,
  showMeetings,
  onNavigate,
  about,
  debug,
  settingsTarget = null,
}: {
  section: ShellSectionId;
  meetingsUi: MeetingsUi | null;
  showMeetings: boolean;
  onNavigate: (section: ShellSectionId, target?: SettingsTarget) => void;
  about: ReactNode;
  debug: ReactNode;
  settingsTarget?: SettingsTarget;
}) {
  const { t } = useTranslation();
  const { settings } = useSettings();
  if (section === "home")
    return <HomeView showMeetings={showMeetings} onNavigate={onNavigate} />;
  if (section === "voice-history")
    return (
      <section data-route-domain="voice" className="w-full">
        <h1 className="text-2xl font-semibold mb-4">
          {t("sidebar.dictationHistory")}
        </h1>
        <HistorySettings />
      </section>
    );
  if (isMeetingSection(section)) {
    const label =
      SHELL_ROUTES.find((route) => route.id === section)?.labelKey ??
      "meetings.sidebar";
    const hasMeetingsRecordView =
      section === "meetings-record" && showMeetings && meetingsUi !== null;
    return (
      <section
        data-route-domain="meetings"
        className="max-w-3xl w-full mx-auto space-y-3"
      >
        <h1 className="text-2xl font-semibold">{t(label)}</h1>
        {hasMeetingsRecordView ? (
          <meetingsUi.MeetingsView />
        ) : (
          <p role="status" className="text-sm text-mid-gray">
            {t(
              showMeetings
                ? "sidebar.meetingReady"
                : "sidebar.meetingsUnavailable",
            )}
          </p>
        )}
      </section>
    );
  }
  if (section === "voice-profiles")
    return (
      <section
        data-route-domain="voice"
        className="max-w-3xl w-full mx-auto space-y-3"
      >
        <h1 className="text-2xl font-semibold">
          {t("sidebar.perAppProfiles")}
        </h1>
        <p role="status">{t("sidebar.profilesReady")}</p>
      </section>
    );
  if (section === "voice-words") return <WordsAndCleanupV022 />;
  if (section === "models") return <ModelsSettings />;
  if (section === "about") return about;
  if (section === "debug" && settings?.debug_mode) return debug;
  return (
    <SettingsV022Page
      target={settingsTarget}
      onNavigate={(next) => onNavigate(next)}
    />
  );
}
