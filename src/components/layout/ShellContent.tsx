import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import type { MeetingsUi } from "../meetings/gate";
import { HomeView } from "../home/HomeView";
import { GeneralSettings } from "../settings/general/GeneralSettings";
import { AdvancedSettings } from "../settings/advanced/AdvancedSettings";
import { HistorySettings } from "../settings/history/HistorySettings";
import { ModelsSettings } from "../settings/models/ModelsSettings";
import { CustomWords } from "../settings/CustomWords";
import { Corrections } from "../settings/Corrections";
import { PostProcessingSettings } from "../settings/post-processing/PostProcessingSettings";
import { SettingsGroup } from "../ui/SettingsGroup";
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
}: {
  section: ShellSectionId;
  meetingsUi: MeetingsUi | null;
  showMeetings: boolean;
  onNavigate: (section: ShellSectionId) => void;
  about: ReactNode;
  debug: ReactNode;
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
  if (section === "voice-words")
    return (
      <section
        data-route-domain="voice"
        className="max-w-3xl w-full mx-auto space-y-6"
      >
        <SettingsGroup title={t("sidebar.wordsAndCleanup")}>
          <CustomWords grouped />
          <Corrections grouped />
        </SettingsGroup>
        {settings?.post_process_enabled ? (
          <PostProcessingSettings />
        ) : (
          <button
            type="button"
            onClick={() => onNavigate("settings")}
            className="text-logo-primary underline focus-visible:ring-2"
          >
            {t("sidebar.cleanupSettings")}
          </button>
        )}
      </section>
    );
  if (section === "models") return <ModelsSettings />;
  if (section === "about") return about;
  if (section === "debug" && settings?.debug_mode) return debug;
  return (
    <section
      data-route-domain="shared"
      className="max-w-3xl w-full mx-auto space-y-6"
    >
      <GeneralSettings />
      <AdvancedSettings />
    </section>
  );
}
