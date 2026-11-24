import React, { type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { HomeView } from "../home/HomeView";
import {
  AskMeetingsView,
  MeetingTodosPanel,
} from "../meetings/AskMeetingsView";
import { FoldersAndTemplatesView } from "../meetings/FoldersAndTemplatesView";
import { MeetingHistoryHub } from "../meetings/MeetingHistoryExtras";
import { queueMeetingCitationJump } from "../meetings/MeetingsList";
import { RecordMeetingPage } from "../meetings/RecordMeetingPage";
import type { MeetingsUi } from "../meetings/gate";
import { AppProfilesView } from "../settings/AppProfilesView";
import { AboutSettings } from "../settings/about/AboutSettings";
import {
  SettingsV022Page,
  type SettingsTarget,
} from "../settings/SettingsV022Page";
import CocoVoiceIcon from "../icons/CocoVoiceIcon";
import { HistorySettings } from "../settings/history/HistorySettings";
import { ModelsSettings } from "../settings/models/ModelsSettings";
import { WordsAndCleanupV022 } from "../settings/post-processing/PostProcessingSettings";

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
  "meetings-record": ["meetings.sidebar", "sidebar.recordMeeting", "meetings"],
  "meetings-ask": ["meetings.sidebar", "sidebar.askMeetings", "meetings"],
  "meetings-folders": [
    "meetings.sidebar",
    "sidebar.foldersAndTemplates",
    "meetings",
  ],
  "voice-profiles": ["sidebar.voiceSection", "sidebar.perAppProfiles", "voice"],
  about: ["sidebar.settings", "sidebar.about", "shared"],
};

const ShellPage: React.FC<{
  e: string;
  k: string;
  d: RouteDomain;
}> = (p) => {
  const { t } = useTranslation();
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

const TodosHistoryMount: React.FC = () => {
  const { t } = useTranslation();
  return (
    <MeetingHistoryHub
      todosLabel={t("meetings.ask.todosTab")}
      todosSlot={<MeetingTodosPanel />}
    />
  );
};

export function renderShellSection(
  s: ShellSectionId,
  mUi: MeetingsUi | null,
  ex?: {
    about?: ReactNode;
    debug?: ReactNode;
    showMeetings?: boolean;
    onNavigate?: (s: ShellSectionId, target?: SettingsTarget) => void;
    onFixStaleAccessibility?: () => void;
    settingsTarget?: SettingsTarget;
  },
): ReactNode {
  if (s === "home")
    return (
      <HomeView
        showMeetings={ex?.showMeetings}
        onNavigate={ex?.onNavigate}
        onFixStaleAccessibility={ex?.onFixStaleAccessibility}
      />
    );
  if (s === "meetings-record")
    return (
      <div data-route-domain="meetings" className="w-full">
        {mUi ? <mUi.MeetingsView /> : <RecordMeetingPage />}
      </div>
    );
  if (s === "meetings-history")
    return (
      <div data-route-domain="meetings" className="w-full">
        <TodosHistoryMount />
      </div>
    );
  if (s === "meetings-ask")
    return (
      <div data-route-domain="meetings" className="w-full">
        <AskMeetingsView
          onJumpToCitation={(citation) => {
            if (typeof citation.meetingId !== "number" || !ex?.onNavigate)
              return;
            queueMeetingCitationJump({
              meetingId: citation.meetingId,
              startMs: citation.start_ms,
            });
            ex.onNavigate("meetings-history");
          }}
        />
      </div>
    );
  if (s === "meetings-folders")
    return (
      <div data-route-domain="meetings" className="w-full">
        <FoldersAndTemplatesView />
      </div>
    );
  if (s === "voice-profiles")
    return (
      <div data-route-domain="voice" className="w-full">
        <AppProfilesView onNavigate={ex?.onNavigate} />
      </div>
    );
  if (s === "voice-history")
    return (
      <div data-route-domain="voice" className="w-full">
        <HistorySettings />
      </div>
    );
  if (s === "voice-words") return <WordsAndCleanupV022 />;
  if (s === "models") return <ModelsSettings />;
  if (s === "about")
    return ex?.about ?? <AboutSettings onNavigate={ex?.onNavigate} />;
  if (s === "debug") return ex?.debug ?? null;
  if (PH[s]) return <ShellPage e={PH[s]![0]} k={PH[s]![1]} d={PH[s]![2]} />;
  return (
    <SettingsV022Page
      target={ex?.settingsTarget ?? null}
      onNavigate={ex?.onNavigate}
    />
  );
}

const BRAND = "Coco Voice";
const HEAD =
  "mt-2.5 mb-0.5 mx-4 block text-[11px] leading-[14px] font-bold uppercase tracking-[0.06em] text-mid-gray";

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
  const appVer = p.appVersion ?? "";
  return (
    <aside className="flex w-[200px] shrink-0 flex-col h-full overflow-y-auto border-e border-mid-gray/20 bg-mid-gray/[0.06] py-3.5">
      <div className="flex items-center gap-2 px-4 pt-0.5 pb-3 text-[14px] font-bold tracking-[-0.02em] text-text">
        <CocoVoiceIcon width={18} height={18} />
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
