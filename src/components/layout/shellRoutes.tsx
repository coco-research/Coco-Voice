import React, { type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import type { MeetingsUi } from "../meetings/gate";
import { CustomWords } from "../settings/CustomWords";
import { Corrections } from "../settings/Corrections";
import { GeneralSettings } from "../settings/general/GeneralSettings";
import { AdvancedSettings } from "../settings/advanced/AdvancedSettings";
import { HistorySettings } from "../settings/history/HistorySettings";
import { ModelsSettings } from "../settings/models/ModelsSettings";
import { PostProcessingSettings } from "../settings/post-processing/PostProcessingSettings";
import { SettingsGroup } from "../ui/SettingsGroup";

export type RouteDomain = "home" | "meetings" | "voice" | "shared";
export type ShellGroup = "top" | "meetings" | "voice" | "bottom";

const r = <const Id extends string>(
  id: Id,
  group: ShellGroup,
  domain: RouteDomain,
  labelKey: string,
  sub = false,
) => ({ id, group, domain, labelKey, sub });

export const SHELL_ROUTES = [
  r("home", "top", "home", "sidebar.home"),
  r("meetings-record", "meetings", "meetings", "sidebar.recordMeeting", true),
  r("meetings-history", "meetings", "meetings", "sidebar.meetingHistory", true),
  r("meetings-ask", "meetings", "meetings", "sidebar.askMeetings", true),
  r(
    "meetings-folders",
    "meetings",
    "meetings",
    "sidebar.foldersAndTemplates",
    true,
  ),
  r("meetings-agent", "meetings", "meetings", "sidebar.agentAccess", true),
  r("voice-history", "voice", "voice", "sidebar.dictationHistory", true),
  r("voice-profiles", "voice", "voice", "sidebar.perAppProfiles", true),
  r("voice-words", "voice", "voice", "sidebar.wordsAndCleanup", true),
  r("models", "bottom", "shared", "sidebar.models"),
  r("settings", "bottom", "shared", "sidebar.settings"),
  r("about", "bottom", "shared", "sidebar.about"),
] as const;

const DEBUG_ROUTE = r("debug", "bottom", "shared", "sidebar.debug");
export type ShellRouteSpec = (typeof SHELL_ROUTES)[number] | typeof DEBUG_ROUTE;
export type ShellSectionId = ShellRouteSpec["id"];

const PH: Partial<Record<ShellSectionId, [string, string, RouteDomain]>> = {
  home: ["sidebar.home", "sidebar.home", "home"],
  "meetings-record": ["meetings.sidebar", "sidebar.recordMeeting", "meetings"],
  "meetings-history": [
    "meetings.sidebar",
    "sidebar.meetingHistory",
    "meetings",
  ],
  "meetings-ask": ["meetings.sidebar", "sidebar.askMeetings", "meetings"],
  "meetings-folders": [
    "meetings.sidebar",
    "sidebar.foldersAndTemplates",
    "meetings",
  ],
  "meetings-agent": ["meetings.sidebar", "sidebar.agentAccess", "meetings"],
  "voice-profiles": ["sidebar.voiceSection", "sidebar.perAppProfiles", "voice"],
  about: ["sidebar.settings", "sidebar.about", "shared"],
};

const ShellPage: React.FC<{
  e: string;
  k: string;
  d: RouteDomain;
  words?: boolean;
}> = (p) => {
  const { t } = useTranslation();
  if (p.words) {
    return (
      <div
        data-route-domain="voice"
        className="max-w-3xl w-full mx-auto space-y-6"
      >
        <SettingsGroup title={t(p.k)}>
          <CustomWords grouped />
          <Corrections grouped />
        </SettingsGroup>
        <PostProcessingSettings />
      </div>
    );
  }
  return (
    <div
      data-route-domain={p.d}
      className="max-w-3xl w-full mx-auto space-y-1.5"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t(p.e)}
      </p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t(p.k)}
      </h2>
      <p className="text-[13px] leading-[18px] text-mid-gray">
        {t("sidebar.sectionPlaceholder")}
      </p>
    </div>
  );
};

export function renderShellSection(
  s: ShellSectionId,
  mUi: MeetingsUi | null,
  ex?: {
    about?: ReactNode;
    debug?: ReactNode;
    onNavigate?: (s: ShellSectionId) => void;
  },
): ReactNode {
  if (s === "meetings-record" && mUi)
    return (
      <div data-route-domain="meetings" className="w-full">
        <mUi.MeetingsView />
      </div>
    );
  if (s === "voice-history")
    return (
      <div data-route-domain="voice" className="w-full">
        <HistorySettings />
      </div>
    );
  if (s === "voice-words")
    return (
      <ShellPage
        e="sidebar.voiceSection"
        k="sidebar.wordsAndCleanup"
        d="voice"
        words
      />
    );
  if (s === "models") return <ModelsSettings />;
  if (s === "about" && ex?.about) return ex.about;
  if (s === "debug") return ex?.debug ?? null;
  if (PH[s]) return <ShellPage e={PH[s]![0]} k={PH[s]![1]} d={PH[s]![2]} />;
  return (
    <div
      data-route-domain="shared"
      className="max-w-3xl w-full mx-auto space-y-6"
    >
      <GeneralSettings />
      <AdvancedSettings />
    </div>
  );
}

const BRAND = "Coco Voice";
const HEAD =
  "mt-2.5 mb-0.5 mx-4 block text-[11px] leading-[14px] font-bold uppercase tracking-[0.06em] text-mid-gray";
const LOGO =
  "M2.6 7a1.1 1.1 0 0 1 1.1 1.1v1.8a1.1 1.1 0 0 1-2.2 0V8.1A1.1 1.1 0 0 1 2.6 7zm3.5-3a1.1 1.1 0 0 1 1.1 1.1v7.8a1.1 1.1 0 0 1-2.2 0V5.1A1.1 1.1 0 0 1 6.1 4zm3.5-2.5a1.1 1.1 0 0 1 1.1 1.1v12.8a1.1 1.1 0 0 1-2.2 0V2.6a1.1 1.1 0 0 1 1.1-1.1zm3.5 3.5a1.1 1.1 0 0 1 1.1 1.1v5.8a1.1 1.1 0 0 1-2.2 0V6.1A1.1 1.1 0 0 1 13.1 5zm3.15 2.5a.75.75 0 0 1 .75.75v1.5a.75.75 0 0 1-1.5 0v-1.5a.75.75 0 0 1 .75-.75z";

export const Sidebar: React.FC<{
  activeSection: ShellSectionId;
  onSectionChange: (s: ShellSectionId) => void;
  appVersion?: string;
}> = (p) => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const btn = (it: ShellRouteSpec) => {
    const on = p.activeSection === it.id;
    const cls = `${it.sub ? "ps-[22px] pe-3" : "px-3"} ${on ? "bg-logo-primary text-white font-semibold" : "text-text/80 font-medium hover:bg-mid-gray/15"}`;
    return (
      <button
        key={it.id}
        type="button"
        data-nav-id={it.id}
        aria-current={on ? "page" : undefined}
        onClick={() => p.onSectionChange(it.id)}
        className={`mx-2.5 my-[1px] flex min-h-[30px] items-center rounded-full text-start text-[13.5px] cursor-pointer ${cls}`}
      >
        <span className="truncate">{t(it.labelKey)}</span>
      </button>
    );
  };
  const grp = (g: ShellGroup) =>
    SHELL_ROUTES.filter((x) => x.group === g).map(btn);
  const topItems = grp("top");
  const meetingItems = grp("meetings");
  const voiceItems = grp("voice");
  const bottomItems = grp("bottom");
  const appVer = p.appVersion ?? "0.10.1";
  return (
    <aside className="flex w-[200px] shrink-0 flex-col h-full overflow-y-auto border-e border-mid-gray/20 bg-mid-gray/[0.06] py-3.5">
      <div className="flex items-center gap-2 px-4 pt-0.5 pb-3 text-[14px] font-bold tracking-[-0.02em] text-text">
        <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
          <path fill="currentColor" d={LOGO} />
        </svg>
        <span>{BRAND}</span>
      </div>
      <nav className="flex flex-col">
        {topItems}
        <span data-nav-heading="meetings" className={HEAD}>
          {t("meetings.sidebar")}
        </span>
        {meetingItems}
        <span data-nav-heading="voice" className={HEAD}>
          {t("sidebar.voiceSection")}
        </span>
        {voiceItems}
        <div data-nav-group="shared" className="mt-3.5 flex flex-col">
          {bottomItems}
          {settings?.debug_mode ? btn(DEBUG_ROUTE) : null}
        </div>
      </nav>
      <div className="mt-auto px-4 pt-2.5 font-mono text-[11px] leading-4 text-mid-gray">
        {appVer}
      </div>
    </aside>
  );
};
